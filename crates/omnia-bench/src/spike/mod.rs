//! Day-one rendering probes. Counts denote labels (six glyphs each) for S5,
//! wire quads for S1/S6, and instances for the other scenes.
use crate::{
    alloc::not_ours,
    driver::{Camera, Script},
    harness::{self, RunConfig},
    scene::{BenchScene, PAPER, Sheet},
};
use gpui::{
    App, Bounds, Corners, Font, FontWeight, Hsla, PathBuilder, Pixels, ShapedLine, SharedString,
    TextAlign, TextRun, TransformationMatrix, Window, fill, font, point, px, rgb, rgba, size,
};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub scene: u8,
    pub count: usize,
    pub mode: u8,
}
impl Config {
    pub fn parse(name: &str, count: usize, mode: &str) -> Result<Self, String> {
        let scene = match name {
            "s1" => 1,
            "s2" => 2,
            "s3" => 3,
            "s4" => 4,
            "s5" => 5,
            "s6" => 6,
            "s8" => 8,
            _ => return Err("expected s1..s6 or s8".into()),
        };
        if scene == 3 && mode == "quality" && count != 0 && count != 240 {
            return Err("quality grid contains exactly 240 symbols".into());
        }
        let count = if scene == 3 && mode == "quality" {
            240
        } else if count == 0 {
            match scene {
                1 => 5_000,
                2 | 3 => 1_000,
                4 => 100,
                5 => 2_000,
                6 => 20_000,
                _ => 0,
            }
        } else {
            count
        };
        if count > 100_000 {
            return Err("count must be <= 100000".into());
        }
        let allowed: &[&str] = match scene {
            1 => &["fixed", "quads-only"],
            2 => &["fixed", "square", "rounded", "sprite"],
            3 => &["fixed", "sweep", "quality"],
            4 => &["fixed"],
            8 => &["fixed", "monitors", "displays"],
            5 => &["fixed", "sweep", "direct", "direct-sweep"],
            6 => &["fixed", "repaint", "cached"],
            _ => unreachable!(),
        };
        if !allowed.contains(&mode) {
            return Err(format!("mode {mode} is not valid for {name}"));
        }
        if scene == 8 && count != 0 {
            return Err("s8 has no instance count".into());
        }
        let mode = match mode {
            "fixed" | "square" | "repaint" => 0,
            "rounded" | "sweep" | "cached" => 1,
            "sprite" | "direct" => 2,
            "quality" => 4,
            "monitors" => 5,
            "displays" => 6,
            "direct-sweep" | "quads-only" => 3,
            _ => return Err("unknown spike mode".into()),
        };
        Ok(Self { scene, count, mode })
    }
    pub fn json(self) -> Value {
        json!({"scene":format!("s{}",self.scene),"count":self.count,"mode":self.mode,"reference_machine":false})
    }
}
#[cfg(all(feature = "readback", windows))]
mod readback;

pub fn run_scene(
    config: Config,
    frames: u32,
    warmup: u32,
    size: (f32, f32),
) -> Result<Value, String> {
    #[cfg(target_os = "linux")]
    if config.scene == 8 && config.mode == 0 {
        let mut report = json!({
            "schema": crate::compare::SCHEMA,
            "kind": "spike-platform-support",
            "spike": config.json(),
            "readback": {"supported": false,
                "reason": "Pinned GPUI Linux PlatformWindow does not implement render_to_image"}
        });
        for (name, args) in [
            ("start", vec!["start", "--runs", "10"]),
            ("idle", vec!["idle", "--seconds", "10"]),
        ] {
            let output =
                std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                    .args(args)
                    .args(["--size", &format!("{}x{}", size.0, size.1)])
                    .output()
                    .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(format!(
                    "S8 {name}: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            report[name] = serde_json::from_slice(&output.stdout)
                .map_err(|e| format!("S8 {name} JSON: {e}"))?;
        }
        return Ok(report);
    }
    if config.scene == 8 && config.mode == 6 {
        #[cfg(all(feature = "readback", windows))]
        return readback::displays();
        #[cfg(not(all(feature = "readback", windows)))]
        return Err("display readback requires Windows and --features readback".into());
    }
    if (config.scene == 8 && config.mode == 0) || (config.scene == 3 && config.mode == 4) {
        #[cfg(all(feature = "readback", windows))]
        {
            let mut report = readback::run(config, size)?;
            if config.scene == 8 {
                for (name, args) in [
                    ("start", vec!["start", "--runs", "10"]),
                    ("idle", vec!["idle", "--seconds", "10"]),
                ] {
                    let output = std::process::Command::new(
                        std::env::current_exe().map_err(|e| e.to_string())?,
                    )
                    .args(args)
                    .args(["--size", &format!("{}x{}", size.0, size.1)])
                    .output()
                    .map_err(|e| e.to_string())?;
                    if !output.status.success() {
                        return Err(format!(
                            "S8 {name}: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ));
                    }
                    report[name] = serde_json::from_slice(&output.stdout)
                        .map_err(|e| format!("S8 {name} JSON: {e}"))?;
                }
            }
            return Ok(report);
        }
        #[cfg(not(all(feature = "readback", windows)))]
        return Err("capture requires Windows and --features readback".into());
    }
    let mut report = harness::run(RunConfig {
        sheet: Sheet::Spike(config),
        script: Script::Pan,
        frames,
        warmup,
        size,
    })?;
    report["kind"] = json!("spike");
    report["config"]["spike"] = config.json();
    Ok(report)
}
pub struct Scene {
    config: Config,
    symbols: Vec<(SharedString, Vec<u8>)>,
    lines: Vec<Vec<ShapedLine>>,
    frame: usize,
    trace: Vec<(usize, f32, f64, u64, u64)>,
    scale_trace: Vec<f32>,
    pages: std::collections::BTreeMap<(u8, u32), (i32, i32)>,
    atlas_log: Vec<Value>,
}
impl Scene {
    pub fn new(config: Config) -> Self {
        let mut files: Vec<_> = std::fs::read_dir(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vendor/electronic-symbols/SVG"
        ))
        .expect("vendored symbols")
        .map(|p| p.expect("symbol path").path())
        .filter(|p| p.extension().is_some_and(|e| e == "svg"))
        .collect();
        files.sort();
        let symbols = files
            .into_iter()
            .take(30)
            .map(|p| {
                (
                    p.to_string_lossy().to_string().into(),
                    std::fs::read(p).expect("symbol bytes"),
                )
            })
            .collect();
        Self {
            config,
            symbols,
            lines: (0..22).map(|_| Vec::new()).collect(),
            frame: 0,
            trace: Vec::with_capacity(4096),
            scale_trace: Vec::with_capacity(4096),
            pages: Default::default(),
            atlas_log: Vec::with_capacity(100),
        }
    }
    fn prepare_text(&mut self, bucket: usize, window: &Window) {
        if !self.lines[bucket].is_empty() {
            return;
        }
        let font_size = px(6.5 + bucket as f32 * 0.5);
        let mut lines = Vec::with_capacity(self.config.count);
        for i in 0..self.config.count {
            let text: SharedString = format!("R{i:05}").into();
            let run = TextRun {
                len: 6,
                font: Font {
                    weight: FontWeight::SEMIBOLD,
                    ..font("Inter")
                },
                color: rgb(0x1d1d1f).into(),
                ..Default::default()
            };
            lines.push(not_ours(|| {
                window
                    .text_system()
                    .shape_line(text, font_size, &[run], None)
            }));
        }
        self.lines[bucket] = lines;
    }
}
/// A device-space affine matrix about the sprite centre. Reflection precedes rotation.
pub fn orientation(bounds: Bounds<Pixels>, scale: f32, orient: usize) -> TransformationMatrix {
    let (s, c) = (orient as f32 % 4. * std::f32::consts::FRAC_PI_2).sin_cos();
    let mirror = if orient >= 4 { -1. } else { 1. };
    let a = [[c * mirror, -s], [s * mirror, c]];
    let x = f32::from(bounds.center().x) * scale;
    let y = f32::from(bounds.center().y) * scale;
    TransformationMatrix {
        rotation_scale: a,
        translation: [x - a[0][0] * x - a[0][1] * y, y - a[1][0] * x - a[1][1] * y],
    }
}
impl BenchScene for Scene {
    fn after_frame(&mut self, window: &Window) {
        #[cfg(feature = "scene-counts")]
        if self.config.scene == 3 || self.config.scene == 5 {
            let scene = window.rendered_scene();
            for tile in scene
                .monochrome_sprites
                .iter()
                .map(|s| s.tile)
                .chain(scene.subpixel_sprites.iter().map(|s| s.tile))
            {
                // DirectX and wgpu create max(1024, requested tile size) pages (pinned
                // gpui_wgpu/src/wgpu_atlas.rs:188-201). In these probes
                // only SVGs/glyphs allocate monochrome entries and none are removed.
                self.pages
                    .entry((tile.texture_id.kind as u8, tile.texture_id.index))
                    .or_insert((
                        1024.max(tile.bounds.size.width.0),
                        1024.max(tile.bounds.size.height.0),
                    ));
            }
            if self.frame.is_multiple_of(8) {
                self.atlas_log.push(json!({"frame":self.frame,"pages":self.pages.len(),
                    "bytes":self.pages.iter().map(|((kind,_),(w,h))|i64::from(*w)*i64::from(*h)*if *kind==0 {1}else{4}).sum::<i64>()}));
            }
        }
        #[cfg(not(feature = "scene-counts"))]
        let _ = window;
    }
    fn details(&self) -> Value {
        json!({"atlas_pages":self.pages.len(),"atlas":self.atlas_log,"atlas_readout":"page sizes inferred from first tile and pinned DirectX/wgpu allocation rule; requires scene-counts", "paint_calls":self.frame,"paint_trace_columns":["frame","size_logical","ms","ours_allocs","total_allocs"],"paint_trace":self.trace,"scale_trace":self.scale_trace})
    }

    fn paint(&mut self, bounds: Bounds<Pixels>, camera: Camera, window: &mut Window, cx: &mut App) {
        let started = std::time::Instant::now();
        let before = crate::alloc::AllocCounts::now();
        let config = self.config;
        let scale = not_ours(|| window.scale_factor());
        if config.scene == 8 && self.scale_trace.len() < self.scale_trace.capacity() {
            self.scale_trace.push(scale);
        }
        let (cam, _) = camera.logical(scale);
        let w = f32::from(bounds.size.width);
        let h = f32::from(bounds.size.height);
        let ink: Hsla = rgb(0x1d1d1f).into();
        not_ours(|| window.paint_quad(fill(bounds, rgb(PAPER))));
        let bucket = if config.mode == 1 || config.mode == 3 {
            (self.frame / 10) % 22
        } else {
            21
        };
        if config.scene == 5 {
            self.prepare_text(bucket, window);
        }
        let symbol_size = if config.mode == 1 && config.scene == 3 {
            if (self.frame / 8).is_multiple_of(39) {
                4.
            } else {
                80. * 2_f32.powf(((self.frame / 8) % 39) as f32 / 4. - 4.5)
            }
        } else {
            24.
        };
        for i in 0..config.count {
            // Wrap the camera so all requested primitives stay in the viewport throughout.
            let x = ((i % 200) as f32 * (w - 100.) / 200. - cam).rem_euclid(w - 100.);
            let y = (i / 200) as f32 * (h - 100.) / ((config.count.div_ceil(200)) as f32);
            let origin = bounds.origin + point(px(x), px(y));
            let rect = Bounds::new(origin, size(px(symbol_size), px(symbol_size)));
            match config.scene {
                1 | 6 => {
                    let sz = if i % 2 == 0 {
                        size(px(32.), px(2. / scale))
                    } else {
                        size(px(2. / scale), px(24.))
                    };
                    not_ours(|| window.paint_quad(fill(Bounds::new(origin, sz), ink)));
                }
                2 if config.mode != 2 => {
                    let mut q = fill(Bounds::new(origin, size(px(3.), px(3.))), ink);
                    if config.mode == 1 {
                        q.corner_radii = Corners::all(px(1.5));
                    }
                    not_ours(|| window.paint_quad(q));
                }
                2 => {
                    let dot=b"<svg xmlns='http://www.w3.org/2000/svg' width='8' height='8'><circle cx='4' cy='4' r='4'/></svg>";
                    not_ours(|| {
                        window.paint_svg(
                            Bounds::new(origin, size(px(3.), px(3.))),
                            "spike-dot".into(),
                            Some(dot),
                            TransformationMatrix::unit(),
                            ink,
                            cx,
                        )
                    })
                    .expect("dot");
                }
                3 => {
                    let (key, data) = &self.symbols[i % 30];
                    let mut transform = orientation(rect, scale, (i / 30) % 8);
                    if config.mode == 1 {
                        // Sweep the raster cache independently of overdraw: preserve a
                        // 24px projected footprint while requesting every K4/K11 bucket.
                        let factor = 24. / symbol_size;
                        for row in &mut transform.rotation_scale {
                            for v in row {
                                *v *= factor;
                            }
                        }
                        let a = transform.rotation_scale;
                        let center = rect.center();
                        let cx = f32::from(center.x) * scale;
                        let cy = f32::from(center.y) * scale;
                        transform.translation = [
                            (f32::from(origin.x) + 12.) * scale - a[0][0] * cx - a[0][1] * cy,
                            (f32::from(origin.y) + 12.) * scale - a[1][0] * cx - a[1][1] * cy,
                        ];
                    }
                    not_ours(|| {
                        window.paint_svg(rect, key.clone(), Some(data), transform, ink, cx)
                    })
                    .expect("symbol");
                }
                4 => {
                    let path = not_ours(|| {
                        let mut p = PathBuilder::stroke(px(2.));
                        p.move_to(origin);
                        p.line_to(origin + point(px(5.), px(0.)));
                        for n in 0..6 {
                            p.line_to(
                                origin
                                    + point(
                                        px(6. + n as f32 * 3.),
                                        px(if n % 2 == 0 { 5. } else { -5. }),
                                    ),
                            );
                        }
                        p.line_to(origin + point(px(30.), px(0.)));
                        p.build().expect("path")
                    });
                    not_ours(|| window.paint_path(path, ink));
                }
                5 => {
                    let line = &self.lines[bucket][i];
                    if config.mode >= 2 {
                        for run in &line.runs {
                            for glyph in &run.glyphs {
                                not_ours(|| {
                                    window.paint_glyph(
                                        origin
                                            + point(
                                                px(0.),
                                                (px(24.) - line.ascent - line.descent) / 2.
                                                    + line.ascent,
                                            )
                                            + glyph.position,
                                        run.font_id,
                                        glyph.id,
                                        line.font_size,
                                        ink,
                                    )
                                })
                                .expect("glyph");
                            }
                        }
                    } else {
                        not_ours(|| line.paint(origin, px(24.), TextAlign::Left, None, window, cx))
                            .expect("label");
                    }
                }
                _ => {}
            }
        }
        if config.scene == 1 && config.mode != 3 {
            let mut y = 0.;
            while y < h {
                let mut x = (-cam).rem_euclid(20.);
                while x < w {
                    not_ours(|| {
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin + point(px(x), px(y)),
                                size(px(1. / scale), px(1. / scale)),
                            ),
                            rgba(0x00000022),
                        ))
                    });
                    x += 20.;
                }
                y += 20.;
            }
        }
        let counts = crate::alloc::AllocCounts::now().since(before);
        if self.trace.len() < self.trace.capacity() {
            self.trace.push((
                self.frame,
                if config.scene == 5 {
                    6.5 + bucket as f32 * 0.5
                } else {
                    symbol_size
                },
                started.elapsed().as_secs_f64() * 1000.,
                counts.ours_allocs,
                counts.total_allocs,
            ));
        }
        self.frame += 1;
    }
}

pub mod s7;

/// Why a spike command failed: bad arguments (exit 2) or a failed run (exit 3).
#[derive(Debug)]
pub enum SpikeError {
    Usage(String),
    Run(String),
}

/// Runs `bench spike <scene> [options]`; `args` starts at the scene name.
pub fn run(args: Vec<String>) -> Result<(), SpikeError> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("s7") => s7::main(args.collect()),
        Some(other) => Err(SpikeError::Usage(format!("unknown spike scene `{other}`"))),
        None => Err(SpikeError::Usage("spike needs a scene name".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn orientation_preserves_pivot_and_reflects_only_last_four() {
        let b = Bounds::new(point(px(10.), px(20.)), size(px(48.), px(48.)));
        for scale in [1., 1.25, 1.5] {
            let mut orbit = Vec::new();
            for o in 0..8 {
                let m = orientation(b, scale, o);
                let a = m.rotation_scale;
                let x = f32::from(b.center().x) * scale;
                let y = f32::from(b.center().y) * scale;
                assert!((a[0][0] * x + a[0][1] * y + m.translation[0] - x).abs() < 0.001);
                assert!((a[1][0] * x + a[1][1] * y + m.translation[1] - y).abs() < 0.001);
                orbit.push((
                    ((a[0][0] * (x + 7.) + a[0][1] * (y + 3.) + m.translation[0]) * 100.).round()
                        as i32,
                    ((a[1][0] * (x + 7.) + a[1][1] * (y + 3.) + m.translation[1]) * 100.).round()
                        as i32,
                ));
                let determinant = a[0][0] * a[1][1] - a[0][1] * a[1][0];
                assert!((determinant - if o < 4 { 1. } else { -1. }).abs() < 0.001);
            }
            orbit.sort_unstable();
            orbit.dedup();
            assert_eq!(orbit.len(), 8);
        }
    }
    #[test]
    fn rejects_irrelevant_modes_and_oversized_loads() {
        assert!(Config::parse("s1", 5000, "direct").is_err());
        assert!(Config::parse("s8", 100, "fixed").is_err());
        assert!(Config::parse("s3", 100001, "fixed").is_err());
        assert_eq!(Config::parse("s5", 0, "sweep").unwrap().count, 2000);
    }
}
