//! Golden-image harness (T3.2, G12; shared with the shell goldens of ui-plan.md UD21).
//!
//! A suite renders views in a hidden window with `Window::render_to_image`, at each scale
//! factor it asks for, and compares every image with a committed PNG. The rule: an image
//! passes when at most 0.1% of its pixels differ from the golden by more than 2/255 in any
//! channel ([`CHANNEL_TOLERANCE`], [`MAX_DIFFERING_PER_THOUSAND`]). `cargo xtask golden
//! --update` (or [`UPDATE_ENV`] set to `1`) rewrites the PNGs instead of comparing.
//!
//! The image `render_to_image` returns is the platform window at its real DPI, while the scene
//! is drawn at the overridden scale factor. The harness therefore lays the view out in a fixed
//! logical `content` box at the top left of a window big enough to hold that box at the largest
//! scale factor on any display of 100% or more, and keeps only the box's device pixels. The
//! goldens are then the same size on every machine.
//!
//! The library uses GPUI without its `test-support` feature, so building the workspace does
//! not turn that feature on for the app or the bench. A test target enables it as a
//! dev-dependency and passes the two calls that need it through [`Hooks`].
//!
//! Window tests skip themselves when `OMNIA_HEADLESS_CI` is set (hosted CI has no desktop),
//! and on Linux and FreeBSD, whose platform window does not implement `render_to_image` at the
//! pinned GPUI (T0.8). Goldens render on Windows development machines.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Pixels, Render, Styled,
    Window, WindowBounds, WindowHandle, WindowOptions, div, point, px, size,
};
pub use image::RgbaImage;

/// A channel may differ from the golden by this much (out of 255) without the pixel counting
/// as different.
pub const CHANNEL_TOLERANCE: u8 = 2;
/// At most this many pixels per thousand may differ: 0.1%.
pub const MAX_DIFFERING_PER_THOUSAND: u64 = 1;
/// The scale factors every golden suite renders at.
pub const SCALE_FACTORS: [f32; 3] = [1.0, 1.25, 1.5];
/// Set to `1` to rewrite the goldens instead of comparing (`cargo xtask golden --update`).
pub const UPDATE_ENV: &str = "OMNIA_GOLDEN_UPDATE";
/// A stalled window test still exits by itself.
const WATCHDOG: Duration = Duration::from_secs(120);

/// The result of comparing one image with its golden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub pixels: u64,
    /// Pixels where some channel differs by more than [`CHANNEL_TOLERANCE`].
    pub differing: u64,
    /// The largest channel difference anywhere in the image.
    pub max_delta: u8,
}

impl Comparison {
    /// The most differing pixels an image of this size may have.
    pub fn limit(&self) -> u64 {
        self.pixels * MAX_DIFFERING_PER_THOUSAND / 1000
    }

    pub fn passes(&self) -> bool {
        self.differing <= self.limit()
    }
}

/// Compares `actual` with `expected` under the golden rule. Images of different sizes fail.
pub fn compare(expected: &RgbaImage, actual: &RgbaImage) -> Result<Comparison, String> {
    if expected.dimensions() != actual.dimensions() {
        return Err(format!(
            "image is {}x{}, golden is {}x{}",
            actual.width(),
            actual.height(),
            expected.width(),
            expected.height()
        ));
    }
    let mut differing = 0;
    let mut max_delta = 0;
    for (e, a) in expected.pixels().zip(actual.pixels()) {
        let delta = (0..4).map(|c| e.0[c].abs_diff(a.0[c])).max().unwrap_or(0);
        max_delta = max_delta.max(delta);
        differing += u64::from(delta > CHANNEL_TOLERANCE);
    }
    Ok(Comparison {
        pixels: u64::from(expected.width()) * u64::from(expected.height()),
        differing,
        max_delta,
    })
}

/// Why window tests cannot run here, if they cannot.
pub fn skip_reason() -> Option<&'static str> {
    if cfg!(any(target_os = "linux", target_os = "freebsd")) {
        return Some("render_to_image is not implemented by this platform's window (T0.8)");
    }
    if std::env::var_os("OMNIA_HEADLESS_CI").is_some() {
        return Some("OMNIA_HEADLESS_CI is set (no interactive desktop)");
    }
    None
}

/// Whether this run rewrites the goldens.
pub fn update_requested() -> bool {
    std::env::var_os(UPDATE_ENV).is_some_and(|v| v == "1")
}

/// `1.25` → `"125"`: the scale factor as a percentage, for file names.
pub fn scale_tag(scale_factor: f32) -> String {
    format!("{}", (scale_factor * 100.0).round() as u32)
}

/// The size in device pixels of a logical box at a scale factor.
pub fn device_size(content: gpui::Size<Pixels>, scale_factor: f32) -> (u32, u32) {
    let device = |v: Pixels| (f32::from(v) * scale_factor).round() as u32;
    (device(content.width), device(content.height))
}

/// A directory of committed goldens, and where failing runs leave their evidence.
#[derive(Clone, Debug)]
pub struct Suite {
    dir: PathBuf,
    out: PathBuf,
}

impl Suite {
    /// `dir` holds `<name>.png`; a failure writes `<name>.actual.png` and `<name>.diff.png`
    /// to `out`.
    pub fn new(dir: impl Into<PathBuf>, out: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            out: out.into(),
        }
    }

    pub fn golden_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.png"))
    }

    /// Compares one image with its golden, or rewrites the golden when `update` is set.
    /// Returns a report line, or the failure.
    pub fn check(&self, name: &str, actual: &RgbaImage, update: bool) -> Result<String, String> {
        let path = self.golden_path(name);
        if update {
            save_png(&path, actual)?;
            return Ok(format!(
                "{name}: wrote {}x{} golden",
                actual.width(),
                actual.height()
            ));
        }
        if !path.exists() {
            self.keep_evidence(name, actual, None)?;
            return Err(format!(
                "{name}: no golden at {}; run `cargo xtask golden --update` and review the PNG",
                path.display()
            ));
        }
        let expected = image::open(&path)
            .map_err(|e| format!("{name}: read {}: {e}", path.display()))?
            .to_rgba8();
        let comparison = compare(&expected, actual).map_err(|e| {
            let _ = self.keep_evidence(name, actual, None);
            format!("{name}: {e}")
        })?;
        let line = format!(
            "{name}: {} of {} pixels differ by more than {CHANNEL_TOLERANCE}/255 (limit {}), \
             largest channel difference {}",
            comparison.differing,
            comparison.pixels,
            comparison.limit(),
            comparison.max_delta
        );
        if comparison.passes() {
            Ok(line)
        } else {
            let evidence = self.keep_evidence(name, actual, Some(&expected))?;
            Err(format!("{line}; FAILED, see {}", evidence.display()))
        }
    }

    /// Writes the actual image and, against a golden of the same size, a diff that marks
    /// differing pixels and dims the rest.
    fn keep_evidence(
        &self,
        name: &str,
        actual: &RgbaImage,
        expected: Option<&RgbaImage>,
    ) -> Result<PathBuf, String> {
        save_png(&self.out.join(format!("{name}.actual.png")), actual)?;
        if let Some(expected) = expected {
            let diff = RgbaImage::from_fn(actual.width(), actual.height(), |x, y| {
                let (e, a) = (expected.get_pixel(x, y).0, actual.get_pixel(x, y).0);
                let delta = (0..4).map(|c| e[c].abs_diff(a[c])).max().unwrap_or(0);
                if delta > CHANNEL_TOLERANCE {
                    image::Rgba([255, 0, 0, 255])
                } else {
                    let v = ((u16::from(e[0]) + u16::from(e[1]) + u16::from(e[2])) / 6) as u8;
                    image::Rgba([v, v, v, 255])
                }
            });
            save_png(&self.out.join(format!("{name}.diff.png")), &diff)?;
        }
        Ok(self.out.clone())
    }
}

fn save_png(path: &Path, image: &RgbaImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    }
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// The two `Window` calls that need GPUI's `test-support` feature, passed in by a test target
/// that enables it, for example:
///
/// ```ignore
/// Hooks {
///     set_scale_factor: |window, sf| window.set_scale_factor(sf),
///     render_to_image: |window| window.render_to_image().map_err(|e| e.to_string()),
/// }
/// ```
#[derive(Clone, Copy)]
pub struct Hooks {
    pub set_scale_factor: fn(&mut Window, f32),
    pub render_to_image: fn(&Window) -> Result<RgbaImage, String>,
}

/// One image: a golden name, a scale factor, an optional per-shot logical box size, and the
/// change to make to the view first (theme, document, camera).
pub struct Shot<V> {
    pub name: String,
    pub scale_factor: f32,
    /// Logical box size for this shot; `None` uses the suite's `content` size.
    pub content: Option<gpui::Size<Pixels>>,
    pub prepare: Box<dyn FnOnce(&mut V)>,
}

/// Lays the view out in the fixed logical box at the top left of the window.
struct Frame<V: 'static> {
    inner: Entity<V>,
    content: gpui::Size<Pixels>,
}

impl<V: 'static + Render> Render for Frame<V> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(self.content.width)
            .h(self.content.height)
            .overflow_hidden()
            .child(self.inner.clone())
    }
}

/// Opens one hidden window, renders every shot in order and returns each one's image, cropped
/// to each shot's logical box (`Shot::content`, or `content` when `None`) at its scale factor.
/// Runs the GPUI application, so call it once per process, from the main thread.
pub fn capture<V: 'static + Render>(
    content: gpui::Size<Pixels>,
    hooks: Hooks,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    shots: Vec<Shot<V>>,
) -> Result<Vec<(String, RgbaImage)>, String> {
    let mut max_w = f32::from(content.width);
    let mut max_h = f32::from(content.height);
    for shot in &shots {
        let box_size = shot.content.unwrap_or(content);
        max_w = max_w.max(f32::from(box_size.width) * shot.scale_factor);
        max_h = max_h.max(f32::from(box_size.height) * shot.scale_factor);
    }
    let window_size = size(px(max_w.ceil()), px(max_h.ceil()));
    let outcome = Rc::new(RefCell::new(None));
    let slot = outcome.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        let bounds = Bounds::new(point(px(0.0), px(0.0)), window_size);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                // Keeps the desktop from clamping a window larger than the display.
                window_min_size: Some(window_size),
                show: false,
                focus: false,
                ..Default::default()
            },
            |window, cx| {
                window.resize(window_size);
                let inner = build(window, cx);
                cx.new(|_| Frame { inner, content })
            },
        );
        let win: WindowHandle<Frame<V>> = match opened {
            Ok(win) => win,
            Err(e) => {
                *slot.borrow_mut() = Some(Err(format!("open hidden window: {e}")));
                cx.quit();
                return;
            }
        };
        cx.spawn(async move |cx| {
            let result = cx.update(|cx| shoot(cx, win, content, hooks, shots));
            *slot.borrow_mut() = Some(result);
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    let result = outcome.borrow_mut().take();
    result.unwrap_or_else(|| Err("the application exited before rendering".to_owned()))
}

fn shoot<V: 'static + Render>(
    cx: &mut App,
    win: WindowHandle<Frame<V>>,
    content: gpui::Size<Pixels>,
    hooks: Hooks,
    shots: Vec<Shot<V>>,
) -> Result<Vec<(String, RgbaImage)>, String> {
    let mut images = Vec::with_capacity(shots.len());
    for Shot {
        name,
        scale_factor,
        content: shot_content,
        prepare,
    } in shots
    {
        let shot_content = shot_content.unwrap_or(content);
        win.update(cx, |frame, _, cx| {
            frame.content = shot_content;
            frame.inner.update(cx, |view, cx| {
                prepare(view);
                cx.notify();
            });
            cx.notify();
        })
        .map_err(|e| format!("{name}: update view: {e}"))?;
        let image = cx
            .update_window(win.into(), |_, window, cx| {
                (hooks.set_scale_factor)(window, scale_factor);
                window.draw(cx).clear(cx);
                if window.scale_factor() != scale_factor {
                    return Err(format!(
                        "scale factor is {}, not {scale_factor}",
                        window.scale_factor()
                    ));
                }
                (hooks.render_to_image)(window)
            })
            .map_err(|e| format!("{name}: draw: {e}"))?
            .map_err(|e| format!("{name}: render_to_image: {e}"))?;
        let (w, h) = device_size(shot_content, scale_factor);
        if image.width() < w || image.height() < h {
            return Err(format!(
                "{name}: the window rendered {}x{} device pixels, smaller than the {w}x{h} \
                 content box",
                image.width(),
                image.height()
            ));
        }
        images.push((
            name,
            image::imageops::crop_imm(&image, 0, 0, w, h).to_image(),
        ));
    }
    Ok(images)
}

/// Runs a golden suite as a `harness = false` test: skips where window tests cannot run,
/// renders every shot, compares or rewrites each golden, prints one line per image, and panics
/// if any image fails. Updating where window tests cannot run is an error, not a skip.
pub fn run<V: 'static + Render>(
    test: &str,
    suite: &Suite,
    content: gpui::Size<Pixels>,
    hooks: Hooks,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    shots: Vec<Shot<V>>,
) {
    let update = update_requested();
    if let Some(reason) = skip_reason() {
        assert!(!update, "{test}: cannot update goldens here: {reason}");
        println!("test {test} ... skipped: {reason}");
        return;
    }
    let label = test.to_owned();
    std::thread::spawn(move || {
        std::thread::sleep(WATCHDOG);
        eprintln!("{label} did not finish within {} s", WATCHDOG.as_secs());
        std::process::exit(3);
    });

    let images = capture(content, hooks, build, shots).unwrap_or_else(|e| panic!("{test}: {e}"));
    let mut failures = Vec::new();
    for (name, image) in &images {
        match suite.check(name, image, update) {
            Ok(line) => println!("  {line}"),
            Err(failure) => {
                println!("  {failure}");
                failures.push(failure);
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{test}: {} of {} images failed the golden rule:\n{}",
        failures.len(),
        images.len(),
        failures.join("\n")
    );
    let verb = if update { "updated" } else { "matched" };
    println!("test {test} ... ok ({} images {verb})", images.len());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, v: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([v, v, v, 255]))
    }

    #[test]
    fn t3_2_identical_images_pass() {
        let c = compare(&solid(100, 50, 40), &solid(100, 50, 40)).unwrap();
        assert_eq!((c.pixels, c.differing, c.max_delta), (5000, 0, 0));
        assert!(c.passes());
    }

    #[test]
    fn t3_2_two_of_255_per_channel_is_not_a_difference() {
        let mut actual = solid(10, 10, 100);
        actual.put_pixel(3, 3, image::Rgba([102, 98, 100, 253]));
        let c = compare(&solid(10, 10, 100), &actual).unwrap();
        assert_eq!((c.differing, c.max_delta), (0, 2));
        actual.put_pixel(4, 4, image::Rgba([100, 100, 100, 252]));
        let c = compare(&solid(10, 10, 100), &actual).unwrap();
        assert_eq!(
            (c.differing, c.max_delta),
            (1, 3),
            "alpha counts as a channel"
        );
    }

    #[test]
    fn t3_2_at_most_one_pixel_in_a_thousand_may_differ() {
        // 2,000 pixels: two may differ, three may not.
        let expected = solid(100, 20, 0);
        let mut actual = expected.clone();
        for x in 0..2 {
            actual.put_pixel(x, 0, image::Rgba([3, 0, 0, 255]));
        }
        let c = compare(&expected, &actual).unwrap();
        assert_eq!((c.differing, c.limit()), (2, 2));
        assert!(c.passes());
        actual.put_pixel(2, 0, image::Rgba([0, 0, 3, 255]));
        assert!(!compare(&expected, &actual).unwrap().passes());
    }

    #[test]
    fn t3_2_size_mismatch_fails() {
        assert!(compare(&solid(10, 10, 0), &solid(10, 11, 0)).is_err());
    }

    #[test]
    fn t3_2_check_writes_then_matches_then_reports_failure() {
        let root = std::env::temp_dir().join(format!("omnia-golden-{}", std::process::id()));
        let suite = Suite::new(root.join("goldens"), root.join("out"));
        let image = solid(40, 25, 200);
        assert!(
            suite.check("case", &image, false).is_err(),
            "missing golden"
        );
        suite.check("case", &image, true).unwrap();
        suite.check("case", &image, false).unwrap();
        let mut moved = image.clone();
        for x in 0..40 {
            moved.put_pixel(x, 7, image::Rgba([0, 0, 0, 255]));
        }
        let failure = suite.check("case", &moved, false).unwrap_err();
        assert!(failure.contains("40 of 1000 pixels"), "{failure}");
        assert!(root.join("out/case.diff.png").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn t3_2_scale_tags_and_device_sizes() {
        assert_eq!(SCALE_FACTORS.map(scale_tag), ["100", "125", "150"]);
        let content = size(px(640.0), px(400.0));
        assert_eq!(device_size(content, 1.25), (800, 500));
        assert_eq!(device_size(content, 1.5), (960, 600));
    }
}
