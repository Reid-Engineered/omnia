//! T3.2: sheet goldens. Loads each fixture sheet from `tests/golden/<name>.ron`, renders it
//! with `render_to_image` (G12) in both themes at scale factors 1.0, 1.25 and 1.5, and
//! compares each image with `tests/golden/<name>-<theme>-<scale>.png` under the golden rule
//! in `omnia_golden` (at most 0.1% of pixels differ by more than 2/255 per channel).
//! `cargo xtask golden --update` rewrites the PNGs.
//!
//! Skips under OMNIA_HEADLESS_CI and on Linux/FreeBSD (no `render_to_image`, T0.8), like the
//! other window tests.

use std::path::Path;

use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Window, point, px, size};
use omnia_canvas::{Camera, SheetView, sheet::SheetDocument};
use omnia_doc::{Doc, GRID_NM, Pt};
use omnia_golden::{Hooks, SCALE_FACTORS, Shot, Suite, scale_tag};
use omnia_theme::{GRAPHITE, GRAY, Theme};

/// A fixture sheet and how to frame it.
struct Fixture {
    /// `tests/golden/<name>.ron`.
    name: &'static str,
    /// Logical size of the view.
    size: (f32, f32),
    /// The sheet point at the centre of the view, in grid steps.
    center: (f64, f64),
    zoom: f64,
}

const FIXTURES: &[Fixture] = &[
    // An inverting amplifier using every built-in symbol, two orientations besides the
    // identity, T-junctions, and a supply rail across the whole view.
    Fixture {
        name: "inverting-amp",
        size: (640.0, 400.0),
        center: (0.0, 0.0),
        zoom: 1.0,
    },
    // T3.3: Wien bridge oscillator rebuilt from `design/build-omnia.mjs` lines 159-234
    // (1366x718 sheet, 20 px = 1 grid step, origin shifted by (-690, -370) px so mockup grid
    // dots at 10 + 20k land on world GRID_NM multiples and canvas (mx, my) lands at screen (mx, my)).
    Fixture {
        name: "wien-oscillator",
        size: (1366.0, 718.0),
        center: (-0.35, -0.55),
        zoom: 1.0,
    },
];

const THEMES: [(&str, &Theme); 2] = [("graphite", &GRAPHITE), ("gray", &GRAY)];

struct Probe {
    documents: Vec<SheetDocument>,
    current: usize,
    camera: Camera,
    theme: &'static Theme,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SheetView::with_document(
            self.camera.clone(),
            self.theme,
            self.documents[self.current].clone(),
        )
    }
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let suite = Suite::new(
        dir,
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-sheet"),
    );
    // Every fixture shares one hidden window sized to hold the largest shot; each shot sets
    // its own logical content box and crops to it.
    let content = FIXTURES.iter().fold(size(px(0.0), px(0.0)), |s, f| {
        size(s.width.max(px(f.size.0)), s.height.max(px(f.size.1)))
    });

    let mut docs = Vec::new();
    let mut shots = Vec::new();
    for (index, fixture) in FIXTURES.iter().enumerate() {
        let path = dir.join(format!("{}.ron", fixture.name));
        let file =
            std::fs::File::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        docs.push(Doc::load(file).unwrap_or_else(|e| panic!("load {}: {e}", path.display())));
        let shot_size = size(px(fixture.size.0), px(fixture.size.1));
        let viewport = gpui::Bounds::new(point(px(0.0), px(0.0)), shot_size);
        let grid = f64::from(GRID_NM);
        let center = Pt {
            x: (fixture.center.0 * grid).round() as i32,
            y: (fixture.center.1 * grid).round() as i32,
        };
        for (theme_name, theme) in THEMES {
            for scale_factor in SCALE_FACTORS {
                let camera = Camera::new(center, fixture.zoom, viewport);
                shots.push(Shot {
                    name: format!("{}-{theme_name}-{}", fixture.name, scale_tag(scale_factor)),
                    scale_factor,
                    content: Some(shot_size),
                    prepare: Box::new(move |probe: &mut Probe| {
                        probe.current = index;
                        probe.camera = camera;
                        probe.theme = theme;
                    }),
                });
            }
        }
    }

    let build = move |_: &mut Window, cx: &mut App| -> Entity<Probe> {
        cx.text_system()
            .add_fonts(vec![
                std::borrow::Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-Medium.ttf").as_slice(),
                ),
                std::borrow::Cow::Borrowed(
                    include_bytes!("../../../vendor/fonts/inter/Inter-SemiBold.ttf").as_slice(),
                ),
            ])
            .expect("register bundled Inter fonts");
        let documents = docs
            .into_iter()
            .map(|doc| SheetDocument::new(cx.new(|_| doc)))
            .collect();
        let viewport = gpui::Bounds::new(point(px(0.0), px(0.0)), content);
        cx.new(|_| Probe {
            documents,
            current: 0,
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            theme: &GRAPHITE,
        })
    };
    omnia_golden::run(
        "sheet_goldens_match",
        &suite,
        content,
        Hooks {
            set_scale_factor: |window, sf| window.set_scale_factor(sf),
            render_to_image: |window| window.render_to_image().map_err(|e| e.to_string()),
        },
        build,
        shots,
    );
}
