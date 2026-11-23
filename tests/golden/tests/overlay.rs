//! T4.3: overlay goldens (selected, hovered, and rubber-band states in both themes at 100%,
//! 125% and 150% scaling).
//!
//! Renders `OverlayView` over `tests/golden/inverting-amp.ron` using `omnia_golden` (`G12`,
//! `render_to_image`). `cargo xtask golden --update` rewrites the PNGs. Skips under
//! `OMNIA_HEADLESS_CI` and on Linux/FreeBSD (no `render_to_image`, T0.8).

use std::path::Path;

use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Window, point, px, size};
use omnia_canvas::{Camera, OverlayView};
use omnia_doc::{Doc, ItemId, LabelId, PartId, Pt, WireId, prefs::SelectMode};
use omnia_golden::{Hooks, SCALE_FACTORS, Shot, Suite, scale_tag};
use omnia_theme::{GRAPHITE, GRAY, Theme};

const CONTENT: (f32, f32) = (640.0, 400.0);
const THEMES: [(&str, &Theme); 2] = [("graphite", &GRAPHITE), ("gray", &GRAY)];

#[derive(Clone)]
struct OverlayCase {
    name: &'static str,
    selection: &'static [ItemId],
    hovered: Option<ItemId>,
    rubber_band: Option<(Pt, Pt, SelectMode)>,
}

const CASES: &[OverlayCase] = &[
    // Selected state: op-amp U1, horizontal wire, and IN label.
    OverlayCase {
        name: "overlay-selected",
        selection: &[
            ItemId::Part(PartId(0)),
            ItemId::Wire(WireId(4)),
            ItemId::Label(LabelId(0)),
        ],
        hovered: None,
        rubber_band: None,
    },
    // Hovered state: resistor R1 hovered with nothing selected.
    OverlayCase {
        name: "overlay-hovered",
        selection: &[],
        hovered: Some(ItemId::Part(PartId(1))),
        rubber_band: None,
    },
    // Rubber-band state: R1 selected while dragging an Intersecting box across D1 and wires.
    OverlayCase {
        name: "overlay-rubber-band",
        selection: &[ItemId::Part(PartId(1))],
        hovered: None,
        rubber_band: Some((
            Pt {
                x: -25_400_000,
                y: -10_160_000,
            },
            Pt {
                x: -5_080_000,
                y: 7_620_000,
            },
            SelectMode::Intersecting,
        )),
    },
];

struct Probe {
    doc: Entity<Doc>,
    camera: Camera,
    theme: &'static Theme,
    case: OverlayCase,
    dirty: bool,
    overlay: Option<Entity<OverlayView>>,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dirty || self.overlay.is_none() {
            let doc = self.doc.clone();
            let camera = self.camera.clone();
            let theme = self.theme;
            let case = self.case.clone();
            let view = cx.new(|cx| {
                let mut overlay = OverlayView::new(doc, camera, theme, cx);
                if !case.selection.is_empty() {
                    overlay.set_selection(case.selection.iter().copied(), cx);
                }
                if let Some(hovered) = case.hovered {
                    overlay.set_hovered(Some(hovered), cx);
                }
                if let Some((anchor, current, mode)) = case.rubber_band {
                    overlay.set_rubber_band_preview(anchor, current, mode, cx);
                }
                overlay
            });
            self.overlay = Some(view);
            self.dirty = false;
        }
        self.overlay.clone().unwrap()
    }
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let suite = Suite::new(
        dir,
        Path::new(env!("CARGO_TARGET_TMPDIR")).join("golden-overlay"),
    );
    let content = size(px(CONTENT.0), px(CONTENT.1));
    let viewport = gpui::Bounds::new(point(px(0.0), px(0.0)), content);

    let path = dir.join("inverting-amp.ron");
    let file =
        std::fs::File::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let doc = Doc::load(file).unwrap_or_else(|e| panic!("load {}: {e}", path.display()));

    let mut shots = Vec::new();
    for case in CASES {
        for (theme_name, theme) in THEMES {
            for scale_factor in SCALE_FACTORS {
                let case = case.clone();
                let camera = Camera::new(Pt::ZERO, 1.0, viewport);
                shots.push(Shot {
                    name: format!("{}-{theme_name}-{}", case.name, scale_tag(scale_factor)),
                    scale_factor,
                    content: Some(content),
                    prepare: Box::new(move |probe: &mut Probe| {
                        probe.camera = camera;
                        probe.theme = theme;
                        probe.case = case;
                        probe.dirty = true;
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
        let doc = cx.new(|_| doc);
        cx.new(|_| Probe {
            doc,
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            theme: &GRAPHITE,
            case: CASES[0].clone(),
            dirty: true,
            overlay: None,
        })
    };

    omnia_golden::run(
        "overlay_goldens_match",
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
