//! T4.3: overlay goldens (selected, hovered, and rubber-band states in both themes at 100%,
//! 125% and 150% scaling).
//!
//! Renders `OverlayView` over `tests/golden/inverting-amp.ron` using `omnia_golden` (`G12`,
//! `render_to_image`). `cargo xtask golden --update` rewrites the PNGs. Skips under
//! `OMNIA_HEADLESS_CI` and on Linux/FreeBSD (no `render_to_image`, T0.8).

use std::path::Path;

use gpui::{App, AppContext, Context, Entity, IntoElement, Render, Window, point, px, size};
use omnia_canvas::{
    Camera, OverlayView,
    tool::{FollowPreview, ToolState},
};
use omnia_doc::{
    Command, Doc, FollowError, GRID_NM, ItemId, LabelId, Orient, PIN_LATTICE_NM, Part, PartId, Pt,
    Rect, Snap, WireId,
    prefs::SelectMode,
    route::{Axis, Blocker},
    snap,
};
use omnia_golden::{Hooks, SCALE_FACTORS, Shot, Suite, scale_tag};
use omnia_theme::{GRAPHITE, GRAY, Theme};

const CONTENT: (f32, f32) = (640.0, 400.0);
const THEMES: [(&str, &Theme); 2] = [("graphite", &GRAPHITE), ("gray", &GRAY)];

#[derive(Clone, Copy)]
struct WirePreviewCase {
    start: Option<Pt>,
    cursor: Pt,
    expected_snap: Snap,
    first_leg: Option<Axis>,
}

#[derive(Clone, Copy)]
struct MovePreviewCase {
    items: &'static [ItemId],
    delta: Pt,
    by: Orient,
    alt: bool,
    expect_refused: bool,
    cage_around: Option<Pt>,
}

#[derive(Clone)]
struct OverlayCase {
    name: &'static str,
    selection: &'static [ItemId],
    hovered: Option<ItemId>,
    rubber_band: Option<(Pt, Pt, SelectMode)>,
    segment_drag: Option<(WireId, i32)>,
    wire_preview: Option<WirePreviewCase>,
    move_preview: Option<MovePreviewCase>,
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
        segment_drag: None,
        wire_preview: None,
        move_preview: None,
    },
    // Hovered state: resistor R1 hovered with nothing selected.
    OverlayCase {
        name: "overlay-hovered",
        selection: &[],
        hovered: Some(ItemId::Part(PartId(1))),
        rubber_band: None,
        segment_drag: None,
        wire_preview: None,
        move_preview: None,
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
        segment_drag: None,
        wire_preview: None,
        move_preview: None,
    },
    // Net-highlight state (T4.4): hovering Wire(0) highlights the entire 6-wire IN net.
    OverlayCase {
        name: "overlay-net-highlight",
        selection: &[],
        hovered: Some(ItemId::Wire(WireId(0))),
        rubber_band: None,
        segment_drag: None,
        wire_preview: None,
        move_preview: None,
    },
    // Refused segment drag state (W3.3, WD7, WD11): dragging horizontal VCC wire WireId(21)
    // down by +2 grid steps across WireId(11)'s corner is refused in `mark/conflict` with
    // blocking wire WireId(11) outlined.
    OverlayCase {
        name: "overlay-segment-drag-refused",
        selection: &[ItemId::Wire(WireId(21))],
        hovered: None,
        rubber_band: None,
        segment_drag: Some((WireId(21), 2 * GRID_NM)),
        wire_preview: None,
        move_preview: None,
    },
    // Wire tool L-route preview ending on a Pin target (T6.2, WD5, WD6): horizontal-first L from
    // (27.94 mm, -12.70 mm) snapped onto U1's output pin (19.05 mm, 0 mm).
    OverlayCase {
        name: "overlay-wire-preview-pin",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: Some(WirePreviewCase {
            start: Some(Pt {
                x: 27_940_000,
                y: -12_700_000,
            }),
            cursor: Pt {
                x: 19_050_000,
                y: 0,
            },
            expected_snap: Snap::Pin {
                part: PartId(0),
                pin: 2,
                at: Pt {
                    x: 19_050_000,
                    y: 0,
                },
            },
            first_leg: Some(Axis::Horizontal),
        }),
        move_preview: None,
    },
    // Wire tool WireEnd snap marker (T6.2, WD5): snapped onto the corner endpoint (-30.48 mm, -17.78 mm).
    OverlayCase {
        name: "overlay-wire-snap-wire-end",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: Some(WirePreviewCase {
            start: None,
            cursor: Pt {
                x: -30_480_000,
                y: -17_780_000,
            },
            expected_snap: Snap::WireEnd {
                wire: WireId(11),
                at: Pt {
                    x: -30_480_000,
                    y: -17_780_000,
                },
            },
            first_leg: None,
        }),
        move_preview: None,
    },
    // Wire tool Junction snap marker (T6.2, WD5): snapped onto the 3-wire junction at (25.40 mm, 0 mm).
    OverlayCase {
        name: "overlay-wire-snap-junction",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: Some(WirePreviewCase {
            start: None,
            cursor: Pt {
                x: 25_400_000,
                y: 0,
            },
            expected_snap: Snap::Junction {
                at: Pt {
                    x: 25_400_000,
                    y: 0,
                },
            },
            first_leg: None,
        }),
        move_preview: None,
    },
    // Wire tool WireBody snap marker (T6.2, WD5): snapped onto vertical wire 18's interior at (25.40 mm, 10.16 mm).
    OverlayCase {
        name: "overlay-wire-snap-wire-body",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: Some(WirePreviewCase {
            start: None,
            cursor: Pt {
                x: 25_400_000,
                y: 10_160_000,
            },
            expected_snap: Snap::WireBody {
                wire: WireId(18),
                at: Pt {
                    x: 25_400_000,
                    y: 10_160_000,
                },
            },
            first_leg: None,
        }),
        move_preview: None,
    },
    // Wire tool flipped vertical-first L-preview with Grid snap marker (T6.2, WD5, WD6).
    OverlayCase {
        name: "overlay-wire-snap-grid",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: Some(WirePreviewCase {
            start: Some(Pt {
                x: 22_860_000,
                y: -15_240_000,
            }),
            cursor: Pt {
                x: 33_020_000,
                y: -7_620_000,
            },
            expected_snap: Snap::Grid(Pt {
                x: 33_020_000,
                y: -7_620_000,
            }),
            first_leg: Some(Axis::Vertical),
        }),
        move_preview: None,
    },
    // W2.3 accepted move-follow preview: dragging R1 down by +2 grid steps reroutes both attached
    // runs and restores the hidden K6 junction dots while showing R1 and its follow wires as ghosts.
    OverlayCase {
        name: "overlay-follow-preview",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: None,
        move_preview: Some(MovePreviewCase {
            items: &[ItemId::Part(PartId(1))],
            delta: Pt {
                x: 0,
                y: 2 * GRID_NM,
            },
            by: Orient::IDENTITY,
            alt: false,
            expect_refused: false,
            cage_around: None,
        }),
    },
    // W2.3 refused move-follow preview (`TC-move-13`, `WD11`): dragging R1 down by +5 grid steps
    // into a 4-pin cage around its moved left pin (-9G, 6.5G) turns the moved ghost to `mark/conflict`
    // and outlines the blocking pin.
    OverlayCase {
        name: "overlay-follow-refused",
        selection: &[],
        hovered: None,
        rubber_band: None,
        segment_drag: None,
        wire_preview: None,
        move_preview: Some(MovePreviewCase {
            items: &[ItemId::Part(PartId(1))],
            delta: Pt {
                x: 0,
                y: 5 * GRID_NM,
            },
            by: Orient::IDENTITY,
            alt: false,
            expect_refused: true,
            cage_around: Some(Pt {
                x: -9 * GRID_NM,
                y: 16_510_000,
            }),
        }),
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
            let doc = if let Some(center) = self.case.move_preview.and_then(|m| m.cage_around) {
                let mut forked = self.doc.read(cx).fork();
                let gnd_sym = forked.part(PartId(4)).unwrap().symbol;
                for (dx, dy) in [(GRID_NM, 0), (-GRID_NM, 0), (0, -GRID_NM), (0, GRID_NM)] {
                    forked
                        .apply(Command::PlacePart {
                            part: Part {
                                symbol: gnd_sym,
                                at: Pt {
                                    x: center.x + dx,
                                    y: center.y + dy,
                                },
                                orient: Orient::IDENTITY,
                                designator: "".into(),
                                value: "".into(),
                            },
                            local_pins: vec![Pt::ZERO],
                            local_bounds: Rect {
                                min: Pt {
                                    x: -3_048_000,
                                    y: 0,
                                },
                                max: Pt {
                                    x: 3_048_000,
                                    y: 6_096_000,
                                },
                            },
                        })
                        .unwrap();
                }
                cx.new(|_| forked)
            } else {
                self.doc.clone()
            };
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
                if let Some((wire, offset)) = case.segment_drag {
                    assert!(
                        overlay.set_segment_drag_preview(wire, offset, cx),
                        "golden segment drag wire {:?} must exist",
                        wire
                    );
                    assert!(
                        matches!(
                            overlay.tool_state(),
                            omnia_canvas::tool::ToolState::DraggingSegment(seg)
                                if seg.blocker() == Some(omnia_doc::route::Blocker::WireEnd(WireId(11)))
                        ),
                        "golden segment drag case {} must be refused with WireEnd(11)",
                        case.name
                    );
                }
                if let Some(wp) = case.wire_preview {
                    let mut scratch = Vec::new();
                    let resolved = snap(
                        overlay.doc().read(cx),
                        wp.cursor,
                        GRID_NM,
                        PIN_LATTICE_NM,
                        &mut scratch,
                    );
                    assert_eq!(
                        resolved, wp.expected_snap,
                        "golden wire case {} must resolve expected WD5 snap",
                        case.name
                    );
                    overlay.set_wire_preview(wp.start, resolved, wp.first_leg, cx);
                }
                if let Some(mp) = case.move_preview {
                    assert!(
                        overlay.set_move_preview(
                            mp.items.iter().copied(),
                            mp.delta,
                            mp.by,
                            mp.alt,
                            cx,
                        ),
                        "golden move preview case {} must activate Dragging",
                        case.name
                    );
                    let ToolState::Dragging(drag) = overlay.tool_state() else {
                        panic!(
                            "golden move preview case {} must be in Dragging state",
                            case.name
                        );
                    };
                    if mp.expect_refused {
                        assert!(
                            matches!(
                                drag.preview(),
                                FollowPreview::Refused(FollowError::NoRoute {
                                    blocker: Blocker::Pin { .. },
                                    ..
                                })
                            ),
                            "golden move preview case {} must be refused with Blocker::Pin, got {:?}",
                            case.name,
                            drag.preview()
                        );
                    } else {
                        assert_eq!(
                            drag.preview(),
                            FollowPreview::Accepted,
                            "golden move preview case {} must accept follow routes",
                            case.name
                        );
                        assert!(
                            !drag.plan_add().is_empty(),
                            "golden move preview case {} must show planned wires",
                            case.name
                        );
                    }
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
