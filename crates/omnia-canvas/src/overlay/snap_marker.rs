//! T6.2: wire-tool L-route preview and WD5 snap-target markers in `stage/snap` (D7, INV6, INV11,
//! B10, WD5, WD6).
//!
//! Renders purely inside [`super::OverlayView`], above the cached sheet, so moving the cursor or
//! flipping the corner never notifies `SheetView` (INV11). Every colour comes exclusively from
//! `omnia-theme` tokens (`stage/snap` for snap markers and preview halos, `stage/ink` for preview
//! wire strokes, INV6). All geometry uses device-pixel-snapped quads (`paint_quad` / `quad`) with
//! zero warm heap allocations (B10).
//!
//! Distinct marker shapes per WD5 [`Snap`] class (all in `stage/snap`):
//! - [`Snap::Pin`]: circle ring + translucent fill + center dot.
//! - [`Snap::WireEnd`]: hollow square frame.
//! - [`Snap::Junction`]: solid filled circle + outer circle ring.
//! - [`Snap::WireBody`]: bold `+` crosshair over a soft circular halo.
//! - [`Snap::Grid`]: subtle small square dot.

use super::{
    DevRect, OverlayPaintScope, WIRE_WIDTH_NM, gpui_color, paint_quad_scoped, paint_solid_outline,
    run_omnia, wire_rect_dev,
};
use crate::{
    camera::Camera,
    snap_px::{
        centered_square, device_rect, device_to_logical, round_device, stroke_device_px, to_device,
    },
    tool::WireGesture,
};
use gpui::{BorderStyle, Bounds, Corners, Edges, Hsla, Pixels, Point, Window, fill, px, quad};
use omnia_doc::{Pt, Rect, Snap, Wire};
use omnia_theme::Theme;

/// Extra logical thickness for the translucent `stage/snap` halo behind L-route preview wires.
const WIRE_PREVIEW_HALO_EXTRA_PX: f64 = 5.0;
/// Preview wire ink opacity (visually distinct from committed sheet ink, INV6).
const WIRE_PREVIEW_INK_ALPHA: f32 = 0.78;
/// Preview wire halo opacity in `stage/snap`.
const WIRE_PREVIEW_HALO_ALPHA: f32 = 0.28;
/// Logical side of the start-anchor square dot at `WireGesture::start`.
const START_ANCHOR_PX: f64 = 4.5;

/// Logical dimensions for each WD5 snap marker class.
const PIN_RING_DIAMETER_PX: f64 = 12.0;
const PIN_RING_STROKE_PX: f64 = 2.0;
const PIN_CENTER_DOT_PX: f64 = 4.0;
const PIN_FILL_ALPHA: f32 = 0.18;

const WIRE_END_SQUARE_PX: f64 = 10.0;
const WIRE_END_STROKE_PX: f64 = 2.0;

const JUNCTION_OUTER_RING_PX: f64 = 14.0;
const JUNCTION_RING_STROKE_PX: f64 = 1.5;
const JUNCTION_INNER_DOT_PX: f64 = 8.0;
const JUNCTION_HALO_ALPHA: f32 = 0.18;

const WIRE_BODY_CROSS_SPAN_PX: f64 = 12.0;
const WIRE_BODY_CROSS_STROKE_PX: f64 = 2.0;
const WIRE_BODY_HALO_PX: f64 = 12.0;
const WIRE_BODY_HALO_ALPHA: f32 = 0.22;

const GRID_DOT_PX: f64 = 4.0;
const GRID_DOT_ALPHA: f32 = 0.75;

/// Shared geometry, theme, and allocation-accounting parameters for [`paint_wire_preview`].
pub(super) struct WirePaintParams<'a> {
    pub camera: &'a Camera,
    pub bounds: Bounds<Pixels>,
    pub visible_padded: Rect,
    pub sf: f32,
    pub clip: (f64, f64, f64, f64),
    pub theme: &'static Theme,
    pub scope: Option<&'a dyn OverlayPaintScope>,
}

#[inline]
fn wire_bounds(wire: Wire) -> Rect {
    Rect {
        min: Pt {
            x: wire.a.x.min(wire.b.x),
            y: wire.a.y.min(wire.b.y),
        },
        max: Pt {
            x: wire.a.x.max(wire.b.x),
            y: wire.a.y.max(wire.b.y),
        },
    }
}

/// Adjusts `size_dev` by at most `+1` device pixel so its parity matches `reference_dev`,
/// keeping concentric markers centred on the exact same device pixel at fractional scale factors.
#[inline]
fn match_parity(size_dev: u32, reference_dev: u32) -> u32 {
    if size_dev % 2 == reference_dev % 2 {
        size_dev
    } else {
        size_dev.saturating_add(1)
    }
}

/// Circle stroke/fill parameters for [`paint_circle`].
#[derive(Clone, Copy)]
struct CircleStyle {
    diameter_dev: u32,
    fill_color: Hsla,
    border_dev: u32,
    border_color: Hsla,
}

/// Paints a device-pixel-centered circular ring/disc using a single GPUI `quad` (zero allocations).
fn paint_circle(
    center: Point<f64>,
    style: CircleStyle,
    params: &WirePaintParams<'_>,
    window: &mut Window,
) {
    let bounds = centered_square(center, style.diameter_dev, params.sf);
    let radius = device_to_logical(f64::from(style.diameter_dev) / 2.0, params.sf);
    let border = if style.border_dev == 0 {
        px(0.0)
    } else {
        device_to_logical(f64::from(style.border_dev), params.sf)
    };
    paint_quad_scoped(
        window,
        quad(
            bounds,
            Corners::all(radius),
            style.fill_color,
            Edges::all(border),
            style.border_color,
            BorderStyle::Solid,
        ),
        params.scope,
    );
}

/// Computes a centered whole-device-pixel [`DevRect`] of size `side_dev × side_dev` around `center`.
fn centered_dev_rect(
    center: Point<f64>,
    side_dev: u32,
    sf: f32,
    clip: (f64, f64, f64, f64),
) -> Option<DevRect> {
    let half = f64::from(side_dev) / 2.0;
    let left = round_device(to_device(center.x, sf) - half);
    let top = round_device(to_device(center.y, sf) - half);
    DevRect::from_edges(
        left,
        top,
        left + f64::from(side_dev),
        top + f64::from(side_dev),
        clip,
    )
}

/// Paints a device-pixel-centered `+` crosshair (`span_dev` long, `thick_dev` wide) in two quads.
fn paint_crosshair(
    center: Point<f64>,
    span_dev: u32,
    thick_dev: u32,
    color: Hsla,
    sf: f32,
    scope: Option<&dyn OverlayPaintScope>,
    window: &mut Window,
) {
    let cx = to_device(center.x, sf);
    let cy = to_device(center.y, sf);
    let half_span = f64::from(span_dev) / 2.0;
    let half_thick = f64::from(thick_dev) / 2.0;

    // Horizontal arm.
    let h_left = round_device(cx - half_span);
    let h_top = round_device(cy - half_thick);
    paint_quad_scoped(
        window,
        fill(device_rect(h_left, h_top, span_dev, thick_dev, sf), color),
        scope,
    );

    // Vertical arm.
    let v_left = round_device(cx - half_thick);
    let v_top = round_device(cy - half_span);
    paint_quad_scoped(
        window,
        fill(device_rect(v_left, v_top, thick_dev, span_dev, sf), color),
        scope,
    );
}

/// Paints the active wire-tool L-route preview and WD5 snap marker inside its own GPUI paint
/// layers.
///
/// Must be called from `paint_overlay` inside its content-masked `run_gpui` block.
pub(super) fn paint_wire_preview(
    gesture: &WireGesture,
    params: &WirePaintParams<'_>,
    window: &mut Window,
) {
    if (gesture.start.is_none() && gesture.snap.is_none())
        || params.bounds.size.width <= px(0.0)
        || params.bounds.size.height <= px(0.0)
    {
        return;
    }

    let snap_color = gpui_color(params.theme.stage_snap);
    let transparent = snap_color.opacity(0.0);
    let preview_ink = gpui_color(params.theme.stage_ink).opacity(WIRE_PREVIEW_INK_ALPHA);
    let preview_halo = snap_color.opacity(WIRE_PREVIEW_HALO_ALPHA);

    let wires = gesture.preview_wires();
    let has_wires = wires[0].is_some() || wires[1].is_some();
    let visible_snap = gesture
        .snap
        .filter(|s| params.visible_padded.contains(s.at()));

    let wire_base_px = WIRE_WIDTH_NM * params.camera.scale();
    let wire_width_dev = stroke_device_px(wire_base_px, params.sf);
    let wire_halo_dev = stroke_device_px(wire_base_px + WIRE_PREVIEW_HALO_EXTRA_PX, params.sf);

    let pin_ring_dev = stroke_device_px(PIN_RING_DIAMETER_PX, params.sf);
    let pin_stroke_dev = stroke_device_px(PIN_RING_STROKE_PX, params.sf);
    let pin_dot_dev = match_parity(stroke_device_px(PIN_CENTER_DOT_PX, params.sf), pin_ring_dev);

    let end_square_dev = stroke_device_px(WIRE_END_SQUARE_PX, params.sf);
    let end_stroke_dev = stroke_device_px(WIRE_END_STROKE_PX, params.sf);

    let junc_ring_dev = stroke_device_px(JUNCTION_OUTER_RING_PX, params.sf);
    let junc_stroke_dev = stroke_device_px(JUNCTION_RING_STROKE_PX, params.sf);
    let junc_dot_dev = match_parity(
        stroke_device_px(JUNCTION_INNER_DOT_PX, params.sf),
        junc_ring_dev,
    );

    let body_span_dev = stroke_device_px(WIRE_BODY_CROSS_SPAN_PX, params.sf);
    let body_stroke_dev = match_parity(
        stroke_device_px(WIRE_BODY_CROSS_STROKE_PX, params.sf),
        body_span_dev,
    );
    let body_halo_dev = match_parity(
        stroke_device_px(WIRE_BODY_HALO_PX, params.sf),
        body_span_dev,
    );

    let grid_dot_dev = stroke_device_px(GRID_DOT_PX, params.sf);
    let start_dot_dev = stroke_device_px(START_ANCHOR_PX, params.sf);

    // Layer 1: translucent wire-preview halos and snap-marker background fills.
    if has_wires
        || matches!(
            visible_snap,
            Some(Snap::Pin { .. } | Snap::Junction { .. } | Snap::WireBody { .. })
        )
    {
        window.paint_layer(|window| {
            run_omnia(params.scope, || {
                for wire in wires.into_iter().flatten() {
                    if !wire_bounds(wire).intersects(params.visible_padded) {
                        continue;
                    }
                    if let Some(rect) =
                        wire_rect_dev(wire, params.camera, params.sf, wire_halo_dev, params.clip)
                    {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(params.sf), preview_halo),
                            params.scope,
                        );
                    }
                }
                if let Some(target) = visible_snap {
                    let screen = params.camera.world_to_screen(target.at());
                    match target {
                        Snap::Pin { .. } => {
                            paint_circle(
                                screen,
                                CircleStyle {
                                    diameter_dev: pin_ring_dev,
                                    fill_color: snap_color.opacity(PIN_FILL_ALPHA),
                                    border_dev: 0,
                                    border_color: transparent,
                                },
                                params,
                                window,
                            );
                        }
                        Snap::Junction { .. } => {
                            paint_circle(
                                screen,
                                CircleStyle {
                                    diameter_dev: junc_ring_dev,
                                    fill_color: snap_color.opacity(JUNCTION_HALO_ALPHA),
                                    border_dev: 0,
                                    border_color: transparent,
                                },
                                params,
                                window,
                            );
                        }
                        Snap::WireBody { .. } => {
                            paint_circle(
                                screen,
                                CircleStyle {
                                    diameter_dev: body_halo_dev,
                                    fill_color: snap_color.opacity(WIRE_BODY_HALO_ALPHA),
                                    border_dev: 0,
                                    border_color: transparent,
                                },
                                params,
                                window,
                            );
                        }
                        Snap::WireEnd { .. } | Snap::Grid(_) => {}
                    }
                }
            });
        });
    }

    // Layer 2: crisp L-route preview wire strokes, start anchor dot, and WD5 snap target marker.
    window.paint_layer(|window| {
        run_omnia(params.scope, || {
            for wire in wires.into_iter().flatten() {
                if !wire_bounds(wire).intersects(params.visible_padded) {
                    continue;
                }
                if let Some(rect) =
                    wire_rect_dev(wire, params.camera, params.sf, wire_width_dev, params.clip)
                {
                    paint_quad_scoped(
                        window,
                        fill(rect.bounds(params.sf), preview_ink),
                        params.scope,
                    );
                }
            }

            if let Some(start) = gesture.start
                && params.visible_padded.contains(start)
            {
                let screen = params.camera.world_to_screen(start);
                paint_quad_scoped(
                    window,
                    fill(
                        centered_square(screen, start_dot_dev, params.sf),
                        snap_color,
                    ),
                    params.scope,
                );
            }

            if let Some(target) = visible_snap {
                let screen = params.camera.world_to_screen(target.at());
                match target {
                    Snap::Pin { .. } => {
                        paint_circle(
                            screen,
                            CircleStyle {
                                diameter_dev: pin_ring_dev,
                                fill_color: transparent,
                                border_dev: pin_stroke_dev,
                                border_color: snap_color,
                            },
                            params,
                            window,
                        );
                        paint_quad_scoped(
                            window,
                            fill(centered_square(screen, pin_dot_dev, params.sf), snap_color),
                            params.scope,
                        );
                    }
                    Snap::WireEnd { .. } => {
                        if let Some(rect) =
                            centered_dev_rect(screen, end_square_dev, params.sf, params.clip)
                        {
                            paint_solid_outline(
                                rect,
                                end_stroke_dev,
                                snap_color,
                                params.sf,
                                params.scope,
                                window,
                            );
                        }
                    }
                    Snap::Junction { .. } => {
                        paint_circle(
                            screen,
                            CircleStyle {
                                diameter_dev: junc_ring_dev,
                                fill_color: transparent,
                                border_dev: junc_stroke_dev,
                                border_color: snap_color,
                            },
                            params,
                            window,
                        );
                        paint_circle(
                            screen,
                            CircleStyle {
                                diameter_dev: junc_dot_dev,
                                fill_color: snap_color,
                                border_dev: 0,
                                border_color: transparent,
                            },
                            params,
                            window,
                        );
                    }
                    Snap::WireBody { .. } => {
                        paint_crosshair(
                            screen,
                            body_span_dev,
                            body_stroke_dev,
                            snap_color,
                            params.sf,
                            params.scope,
                            window,
                        );
                    }
                    Snap::Grid(_) => {
                        paint_quad_scoped(
                            window,
                            fill(
                                centered_square(screen, grid_dot_dev, params.sf),
                                snap_color.opacity(GRID_DOT_ALPHA),
                            ),
                            params.scope,
                        );
                    }
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::{OverlayPaintScope, viewport_clip_dev};
    use gpui::{
        App, Context, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
        LayoutId, Render, Style, TestAppContext, VisualTestContext, point, relative, size,
    };
    use omnia_doc::{GRID_NM, PartId, WireId, route::Axis};
    use omnia_theme::GRAPHITE;
    use std::{cell::Cell, rc::Rc};

    struct CountingScope {
        counting: Cell<bool>,
        omnia_calls: Cell<u64>,
        omnia_allocs: Cell<usize>,
    }

    impl CountingScope {
        fn new() -> Rc<Self> {
            Rc::new(Self {
                counting: Cell::new(false),
                omnia_calls: Cell::new(0),
                omnia_allocs: Cell::new(0),
            })
        }
    }

    impl OverlayPaintScope for CountingScope {
        fn omnia(&self, work: &mut dyn FnMut()) {
            if self.counting.get() {
                self.omnia_calls.set(self.omnia_calls.get() + 1);
                let prev = crate::sheet::replace_alloc_counter(Some(0));
                work();
                let used = crate::sheet::replace_alloc_counter(prev).unwrap_or(0);
                self.omnia_allocs.set(self.omnia_allocs.get() + used);
            } else {
                work();
            }
        }

        fn gpui(&self, work: &mut dyn FnMut()) {
            if self.counting.get() {
                let paused = crate::sheet::replace_alloc_counter(None);
                work();
                crate::sheet::replace_alloc_counter(paused);
            } else {
                work();
            }
        }
    }

    struct WirePreviewHarness {
        camera: Camera,
        gesture: WireGesture,
        scope: Rc<CountingScope>,
        paints: Rc<Cell<u64>>,
    }

    struct WirePreviewElement {
        camera: Camera,
        gesture: WireGesture,
        scope: Rc<CountingScope>,
        paints: Rc<Cell<u64>>,
    }

    impl Element for WirePreviewElement {
        type RequestLayoutState = ();
        type PrepaintState = ();

        fn id(&self) -> Option<ElementId> {
            None
        }

        fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
            None
        }

        fn request_layout(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            window: &mut Window,
            cx: &mut App,
        ) -> (LayoutId, Self::RequestLayoutState) {
            let mut style = Style::default();
            style.size.width = relative(1.0).into();
            style.size.height = relative(1.0).into();
            (window.request_layout(style, [], cx), ())
        }

        fn prepaint(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            _bounds: Bounds<Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _window: &mut Window,
            _cx: &mut App,
        ) -> Self::PrepaintState {
        }

        fn paint(
            &mut self,
            _id: Option<&GlobalElementId>,
            _inspector_id: Option<&InspectorElementId>,
            bounds: Bounds<Pixels>,
            _request_layout: &mut Self::RequestLayoutState,
            _prepaint: &mut Self::PrepaintState,
            window: &mut Window,
            _cx: &mut App,
        ) {
            self.paints.set(self.paints.get() + 1);
            let mut camera = self.camera.clone();
            camera.set_viewport(bounds);
            let sf = window.scale_factor();
            let clip = viewport_clip_dev(bounds, sf, 32.0);
            let visible_padded = camera.visible();
            let params = WirePaintParams {
                camera: &camera,
                bounds,
                visible_padded,
                sf,
                clip,
                theme: &GRAPHITE,
                scope: Some(self.scope.as_ref()),
            };
            paint_wire_preview(&self.gesture, &params, window);
        }
    }

    impl Render for WirePreviewHarness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            WirePreviewElement {
                camera: self.camera.clone(),
                gesture: self.gesture,
                scope: self.scope.clone(),
                paints: self.paints.clone(),
            }
        }
    }

    #[gpui::test]
    fn t6_2_wire_preview_paints_every_snap_class_cleanly(cx: &mut TestAppContext) {
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(400.0)));
        let scope = CountingScope::new();
        let paints = Rc::new(Cell::new(0u64));
        let start = Pt {
            x: -4 * GRID_NM,
            y: -2 * GRID_NM,
        };
        let end = Pt {
            x: 4 * GRID_NM,
            y: 2 * GRID_NM,
        };
        let snaps = [
            Snap::Pin {
                part: PartId(0),
                pin: 0,
                at: end,
            },
            Snap::WireEnd {
                wire: WireId(0),
                at: end,
            },
            Snap::Junction { at: end },
            Snap::WireBody {
                wire: WireId(0),
                at: end,
            },
            Snap::Grid(end),
        ];

        let scope_init = scope.clone();
        let paints_init = paints.clone();
        let (view, cx) = cx.add_window_view(move |_, _| WirePreviewHarness {
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            gesture: WireGesture::for_preview(Some(start), snaps[0], Some(Axis::Horizontal)),
            scope: scope_init,
            paints: paints_init,
        });
        let cx: &mut VisualTestContext = cx;
        cx.run_until_parked();

        scope.omnia_calls.set(0);
        scope.omnia_allocs.set(0);
        scope.counting.set(true);
        for &snap_target in &snaps {
            for leg in [None, Some(Axis::Horizontal), Some(Axis::Vertical)] {
                let s = leg.map(|_| start);
                view.update(cx, |v, cx| {
                    v.gesture = WireGesture::for_preview(s, snap_target, leg);
                    cx.notify();
                });
                cx.run_until_parked();
            }
        }
        scope.counting.set(false);
        assert!(paints.get() >= 15);
        assert!(scope.omnia_calls.get() >= 15);
        assert_eq!(scope.omnia_allocs.get(), 0);
    }
}
