//! W2.3: follow, detach, and refusal overlay painting during move drags (`WD11`, `K6`, `K9`,
//! `INV6`, `INV11`, `B10`).
//!
//! Renders directly inside [`super::OverlayView`] above the cached [`crate::SheetView`] using two
//! GPUI paint layers:
//! - Layer 1: translucent halos behind newly routed `plan.add` wires, dragged item halos (or
//!   `mark/conflict` fills when refused), and conflict blocker fill.
//! - Layer 2: surviving unaffected wires and restored `K6` junction dots in full `stage/ink`,
//!   planned `plan.add` follow wires in ghost `stage/ink`, dragged item ghosts/outlines (or
//!   `mark/conflict` when refused), and conflict blocker outline (`Blocker`).
//!
//! All colours come strictly from `omnia-theme` tokens (`INV6`), no `paint_path` calls are used,
//! and the warm paint path performs zero Omnia heap allocations (`B10`).

use super::{
    DevRect,
    ghosts::{
        BLOCKER_PIN_HALF_PX, BLOCKER_WIRE_PAD_PX, GhostLabel, GhostPaintParams, GhostPaintStyle,
        blocker_dev_rect, paint_drag_items_fg, paint_drag_items_halo, paint_ghost_label_fg,
        paint_ghost_label_halo, resolve_drag_wire, transform_pt, wire_bounds,
    },
    gpui_color, label_rect_dev, padded_rect_dev, paint_quad_scoped, paint_solid_outline, run_gpui,
    run_omnia, wire_rect_dev,
};
use crate::{
    sheet::lod,
    snap_px::{centered_square, stroke_device_px},
};
use gpui::{App, Hsla, Window, fill};
use omnia_doc::{FollowError, Isometry, ItemId, LabelId, Pt, Wire, WireId, route::Blocker};

/// Resolves the device-pixel conflict rectangle for `blocker`, applying `transform` if the blocker
/// belongs to a moved item in `items` (`WD11`, `B10`).
fn blocker_dev_rect_for_drag(
    blocker: Blocker,
    items: &[ItemId],
    transform: Isometry,
    params: &GhostPaintParams<'_>,
) -> Option<(DevRect, Option<Pt>)> {
    match blocker {
        Blocker::Pin { part, pin } if items.contains(&ItemId::Part(part)) => {
            let doc_pin = *params.doc.pins(part)?.get(pin)?;
            let at = transform_pt(transform, doc_pin).pt;
            if !params.visible_padded.contains(at) {
                return None;
            }
            label_rect_dev(
                at,
                BLOCKER_PIN_HALF_PX,
                params.camera,
                params.sf,
                params.clip,
            )
            .map(|rect| (rect, Some(at)))
        }
        Blocker::WireEnd(id) | Blocker::WireBody(id) | Blocker::Overlap(id)
            if items.contains(&ItemId::Wire(id)) =>
        {
            let moved = resolve_drag_wire(params.doc, id, transform)?;
            if !moved.bounds.intersects(params.visible_padded) {
                return None;
            }
            padded_rect_dev(
                moved.bounds,
                BLOCKER_WIRE_PAD_PX,
                params.camera,
                params.sf,
                params.clip,
            )
            .map(|rect| (rect, None))
        }
        _ => blocker_dev_rect(blocker, params),
    }
}

/// Paints precomputed `K6` junction dots matching `SheetView` LOD rules (`sheet.rs`, `B10`):
/// circular alpha-mask sprite at >= 3 device px and centered square below 3 device px.
fn paint_junction_dots(
    points: &[Pt],
    color: Hsla,
    params: &GhostPaintParams<'_>,
    window: &mut Window,
    cx: &App,
) {
    if points.is_empty() {
        return;
    }
    let Some(diameter) = lod::junction_diameter(params.camera) else {
        return;
    };
    let side = stroke_device_px(diameter, params.sf);
    for &p in points {
        if !params.visible_padded.contains(p) {
            continue;
        }
        let center = params.camera.world_to_screen(p);
        if side < 3 {
            paint_quad_scoped(
                window,
                fill(centered_square(center, side, params.sf), color),
                params.scope,
            );
        } else {
            let shape = params.masks.dot(center.x, center.y, diameter, params.sf);
            let (bounds, matrix) = shape.sprite(params.sf);
            let key = shape.mask.key.clone();
            run_gpui(params.scope, || {
                if let Err(error) =
                    window.paint_svg(bounds, key, Some(&shape.mask.svg), matrix, color, cx)
                {
                    eprintln!("follow dot: {error}");
                }
            });
        }
    }
}

/// Parameters borrowed from `GhostPreview::Follow` for layer painting.
pub(super) struct FollowPreviewData<'a> {
    pub items: &'a [ItemId],
    pub transform: Isometry,
    pub affected_wires: &'a [WireId],
    pub affected_labels: &'a [LabelId],
    pub plan_add: &'a [Wire],
    pub plan_remove_sorted: &'a [WireId],
    pub plan_labels_sorted: &'a [(LabelId, Pt)],
    pub preview_junctions: &'a [Pt],
    pub refusal: Option<FollowError>,
}

/// Paints the follow/detach/refused drag preview inside two GPUI paint layers (`B10`, `INV6`).
pub(super) fn paint_follow_preview(
    data: FollowPreviewData<'_>,
    style: GhostPaintStyle,
    params: &GhostPaintParams<'_>,
    window: &mut Window,
    cx: &App,
) {
    let GhostPaintStyle { palette, sizes } = style;
    let full_ink = gpui_color(params.theme.stage_ink);
    let ink_secondary = gpui_color(params.theme.stage_ink_secondary);
    let refused_blocker = data
        .refusal
        .map(|FollowError::NoRoute { blocker, .. }| blocker);
    let is_refused = refused_blocker.is_some();

    // Layer 1: translucent wire/label halos behind planned follow segments, dragged item halos
    // (or conflict fill on refusal), and blocker conflict highlight (`INV6`).
    window.paint_layer(|window| {
        run_omnia(params.scope, || {
            if !is_refused {
                for &wire in data.plan_add {
                    if !wire_bounds(wire).intersects(params.visible_padded) {
                        continue;
                    }
                    if let Some(rect) = wire_rect_dev(
                        wire,
                        params.camera,
                        params.sf,
                        sizes.wire_halo_dev,
                        params.clip,
                    ) {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(params.sf), palette.wire_halo),
                            params.scope,
                        );
                    }
                }
                for &(_, moved_at) in data.plan_labels_sorted {
                    paint_ghost_label_halo(
                        GhostLabel {
                            at: moved_at,
                            in_bounds: moved_at.is_in_bounds(),
                        },
                        params,
                        palette,
                        window,
                    );
                }
            }
            paint_drag_items_halo(
                data.items,
                data.transform,
                is_refused,
                style,
                params,
                window,
            );
            if let Some(blocker) = refused_blocker
                && let Some((rect, _)) =
                    blocker_dev_rect_for_drag(blocker, data.items, data.transform, params)
            {
                paint_quad_scoped(
                    window,
                    fill(rect.bounds(params.sf), palette.conflict_halo),
                    params.scope,
                );
            }
        });
    });

    // Layer 2: surviving original affected wires + K6 junction dots in full `stage/ink`, newly
    // routed follow wires in ghost ink, moved rigid-run corner labels (or stationary anchor dot
    // for already-hidden rigid labels when refused/identity), dragged items (conflict on refusal),
    // and blocker outline. Labels on non-rigid runs stay in `SheetView` with their text intact.
    window.paint_layer(|window| {
        run_omnia(params.scope, || {
            for &id in data.affected_wires {
                if !is_refused
                    && data
                        .plan_remove_sorted
                        .binary_search_by_key(&id.0, |w| w.0)
                        .is_ok()
                {
                    continue;
                }
                if let Some(&wire) = params.doc.wire(id)
                    && wire_bounds(wire).intersects(params.visible_padded)
                    && let Some(rect) = wire_rect_dev(
                        wire,
                        params.camera,
                        params.sf,
                        sizes.wire_width_dev,
                        params.clip,
                    )
                {
                    paint_quad_scoped(window, fill(rect.bounds(params.sf), full_ink), params.scope);
                }
            }
            if !is_refused {
                for &wire in data.plan_add {
                    if !wire_bounds(wire).intersects(params.visible_padded) {
                        continue;
                    }
                    if let Some(rect) = wire_rect_dev(
                        wire,
                        params.camera,
                        params.sf,
                        sizes.wire_width_dev,
                        params.clip,
                    ) {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(params.sf), palette.ink),
                            params.scope,
                        );
                    }
                }
            }
            paint_junction_dots(data.preview_junctions, full_ink, params, window, cx);
            if !is_refused {
                for &(_, moved_at) in data.plan_labels_sorted {
                    paint_ghost_label_fg(
                        GhostLabel {
                            at: moved_at,
                            in_bounds: moved_at.is_in_bounds(),
                        },
                        sizes.outline_dev,
                        sizes.label_dot_dev,
                        params,
                        palette,
                        window,
                    );
                }
            }
            for &id in data.affected_labels {
                if (is_refused
                    || data
                        .plan_labels_sorted
                        .binary_search_by_key(&id.0, |(lid, _)| lid.0)
                        .is_err())
                    && let Some(label) = params.doc.label(id)
                    && params.visible_padded.contains(label.at)
                {
                    let screen = params.camera.world_to_screen(label.at);
                    paint_quad_scoped(
                        window,
                        fill(
                            centered_square(screen, sizes.label_dot_dev, params.sf),
                            ink_secondary,
                        ),
                        params.scope,
                    );
                }
            }
            paint_drag_items_fg(
                data.items,
                data.transform,
                is_refused,
                style,
                params,
                window,
                cx,
            );
            if let Some(blocker) = refused_blocker
                && let Some((rect, pin_at)) =
                    blocker_dev_rect_for_drag(blocker, data.items, data.transform, params)
            {
                paint_solid_outline(
                    rect,
                    sizes.blocker_outline_dev,
                    palette.conflict_outline,
                    params.sf,
                    params.scope,
                    window,
                );
                if let Some(at) = pin_at {
                    let screen = params.camera.world_to_screen(at);
                    paint_quad_scoped(
                        window,
                        fill(
                            centered_square(screen, sizes.blocker_pin_dot_dev, params.sf),
                            palette.conflict_outline,
                        ),
                        params.scope,
                    );
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use crate::{
        camera::Camera,
        overlay::{OverlayPaintScope, OverlayView},
        sheet::replace_alloc_counter,
        tool::FollowPreview,
    };
    use gpui::{
        AppContext, Bounds, Context, IntoElement, Render, TestAppContext, Window, point, px, size,
    };
    use omnia_doc::{Command, Doc, GRID_NM, ItemId, Label, Orient, Part, PartId, Pt, Wire};
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::{GRAPHITE, GRAY};
    use std::{cell::Cell, rc::Rc};

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    struct FollowAllocScope {
        counting: Cell<bool>,
        omnia_allocs: Cell<usize>,
        omnia_calls: Cell<u32>,
    }

    impl FollowAllocScope {
        fn new() -> Self {
            Self {
                counting: Cell::new(false),
                omnia_allocs: Cell::new(0),
                omnia_calls: Cell::new(0),
            }
        }
    }

    impl OverlayPaintScope for FollowAllocScope {
        fn omnia(&self, work: &mut dyn FnMut()) {
            self.omnia_calls.set(self.omnia_calls.get() + 1);
            if !self.counting.get() {
                work();
                return;
            }
            let prev = replace_alloc_counter(Some(0));
            work();
            let used = replace_alloc_counter(prev).unwrap_or(0);
            self.omnia_allocs.set(self.omnia_allocs.get() + used);
        }

        fn gpui(&self, work: &mut dyn FnMut()) {
            if !self.counting.get() {
                work();
                return;
            }
            let paused = replace_alloc_counter(None);
            work();
            replace_alloc_counter(paused);
        }
    }

    fn place_resistor(doc: &mut Doc, at: Pt, des: &str) -> PartId {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(res);
        doc.apply(Command::PlacePart {
            part: Part {
                symbol: res,
                at,
                orient: Orient::IDENTITY,
                designator: des.into(),
                value: "10k".into(),
            },
            local_pins: def.pins.iter().map(|p| p.at).collect(),
            local_bounds: def.bounds,
        })
        .unwrap()
        .added
        .into_iter()
        .find_map(|id| match id {
            ItemId::Part(p) => Some(p),
            _ => None,
        })
        .unwrap()
    }

    /// Doc with R1 wired through a 3-way junction at (0, 0) plus a 4-pin-blocked cage around
    /// `(10G, 0)` so `(10G, 0)` refuses with `Blocker::Pin`.
    fn follow_paint_doc() -> (Doc, PartId) {
        let mut doc = Doc::new();
        let r1 = place_resistor(&mut doc, pt(-6, 0), "R1");
        let _r_top = place_resistor(&mut doc, pt(10, 4), "RT");
        // Wire from R1 right pin (-4, 0) to 3-way junction at (0, 0).
        doc.apply(Command::AddWires(vec![
            Wire {
                a: pt(-4, 0),
                b: pt(0, 0),
            },
            Wire {
                a: pt(0, 0),
                b: pt(0, 6),
            },
            Wire {
                a: pt(0, 0),
                b: pt(4, 0),
            },
        ]))
        .unwrap();
        // Surround (10, -2) on all 4 adjacent grid points so moving R1's right pin to (10, -2)
        // refuses every WD4 candidate with `Blocker::Pin`.
        for (idx, pin_target) in [pt(11, -2), pt(9, -2), pt(10, -1), pt(10, -3)]
            .into_iter()
            .enumerate()
        {
            let _ = place_resistor(
                &mut doc,
                Pt {
                    x: pin_target.x + 2 * G,
                    y: pin_target.y,
                },
                &format!("B{idx}"),
            );
        }
        (doc, r1)
    }

    /// B10: proves the warm follow-preview overlay paint path (both accepted with K6 junction dots
    /// and refused with conflict ghosts + blocker outline, at normal and far LOD) allocates zero
    /// bytes on the Omnia heap (`ours == 0`).
    #[gpui::test]
    fn w2_3_b10_warm_follow_paint_makes_zero_omnia_allocations(cx: &mut TestAppContext) {
        let (doc_val, r1) = follow_paint_doc();
        let doc = cx.new(|_| doc_val);
        let scope = Rc::new(FollowAllocScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));
        let doc_clone = doc.clone();
        let scope_clone = scope.clone();

        struct Host(gpui::Entity<OverlayView>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.clone()
            }
        }

        let (host, cx) = cx.add_window_view(move |_, cx| {
            let overlay = cx.new(|cx| {
                OverlayView::new(
                    doc_clone,
                    Camera::new(Pt::ZERO, 1.0, viewport),
                    &GRAPHITE,
                    cx,
                )
                .with_paint_scope(scope_clone)
            });
            Host(overlay)
        });
        let overlay = host.read_with(cx, |h, _| h.0.clone());

        // Warm accepted follow preview (normal zoom with sprite + junction dot sprite).
        overlay.update(cx, |o, cx| {
            assert!(o.set_move_preview([ItemId::Part(r1)], pt(1, 2), Orient::IDENTITY, false, cx));
        });
        cx.run_until_parked();

        scope.omnia_allocs.set(0);
        scope.omnia_calls.set(0);
        scope.counting.set(true);
        for step in 1..=5 {
            overlay.update(cx, |o, cx| {
                assert!(o.set_move_preview(
                    [ItemId::Part(r1)],
                    pt(step, 2),
                    Orient::IDENTITY,
                    false,
                    cx,
                ));
            });
            cx.run_until_parked();
        }
        scope.counting.set(false);
        assert!(scope.omnia_calls.get() >= 5);
        assert_eq!(
            scope.omnia_allocs.get(),
            0,
            "B10: warm accepted follow overlay paint must make 0 Omnia allocations"
        );

        // Warm refused follow preview (`R1` right pin (-4, 0) moved to blocked cage at (10, -2) =>
        // delta = (14, -2)), then verify 0 Omnia allocations across repeated repaints.
        overlay.update(cx, |o, cx| {
            assert!(o.set_move_preview(
                [ItemId::Part(r1)],
                pt(14, -2),
                Orient::IDENTITY,
                false,
                cx,
            ));
            let crate::tool::ToolState::Dragging(drag) = o.tool_state() else {
                panic!("expected Dragging state");
            };
            assert!(matches!(drag.preview(), FollowPreview::Refused(_)));
        });
        cx.run_until_parked();

        scope.omnia_allocs.set(0);
        scope.omnia_calls.set(0);
        scope.counting.set(true);
        for theme in [&GRAY, &GRAPHITE, &GRAY, &GRAPHITE] {
            overlay.update(cx, |o, cx| {
                o.set_theme(theme, cx);
            });
            cx.run_until_parked();
        }
        scope.counting.set(false);
        assert!(scope.omnia_calls.get() >= 4);
        assert_eq!(
            scope.omnia_allocs.get(),
            0,
            "B10: warm refused follow overlay paint must make 0 Omnia allocations"
        );

        // Far-zoom LOD box + square dot path.
        overlay.update(cx, |o, cx| {
            o.set_camera(Camera::new(Pt::ZERO, 0.05, viewport), cx);
            assert!(o.set_move_preview([ItemId::Part(r1)], pt(1, 2), Orient::IDENTITY, false, cx));
        });
        cx.run_until_parked();

        scope.omnia_allocs.set(0);
        scope.omnia_calls.set(0);
        scope.counting.set(true);
        for step in 1..=3 {
            overlay.update(cx, |o, cx| {
                assert!(o.set_move_preview(
                    [ItemId::Part(r1)],
                    pt(step, 2),
                    Orient::IDENTITY,
                    false,
                    cx,
                ));
            });
            cx.run_until_parked();
        }
        scope.counting.set(false);
        assert!(scope.omnia_calls.get() >= 3);
        assert_eq!(
            scope.omnia_allocs.get(),
            0,
            "B10: far-zoom LOD follow overlay paint must make 0 Omnia allocations"
        );
    }

    /// B10: rigid-run corner labels (moved in `plan_labels_sorted` or stationary when refused)
    /// paint with zero warm Omnia heap allocations.
    #[gpui::test]
    fn w2_3_b10_warm_rigid_label_follow_paint_makes_zero_omnia_allocations(
        cx: &mut TestAppContext,
    ) {
        let mut doc_val = Doc::new();
        let r1 = place_resistor(&mut doc_val, pt(-6, 0), "R1");
        let r2 = place_resistor(&mut doc_val, pt(6, 4), "R2");
        doc_val
            .apply(Command::AddWires(vec![
                Wire {
                    a: pt(-4, 0),
                    b: pt(4, 0),
                },
                Wire {
                    a: pt(4, 0),
                    b: pt(4, 4),
                },
            ]))
            .unwrap();
        doc_val
            .apply(Command::AddLabel(Label {
                at: pt(4, 0),
                text: "MID".into(),
            }))
            .unwrap();
        // Surround (10, -2) on all 4 cardinal neighbours so dragging R1's right pin (-4, 0) by
        // (14, -2) to (10, -2) refuses every WD4 candidate with `Blocker::Pin`.
        for (idx, pin_target) in [pt(11, -2), pt(9, -2), pt(10, -1), pt(10, -3)]
            .into_iter()
            .enumerate()
        {
            let _ = place_resistor(
                &mut doc_val,
                Pt {
                    x: pin_target.x + 2 * G,
                    y: pin_target.y,
                },
                &format!("C{idx}"),
            );
        }

        let doc = cx.new(|_| doc_val);
        let scope = Rc::new(FollowAllocScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));
        let doc_clone = doc.clone();
        let scope_clone = scope.clone();

        struct Host(gpui::Entity<OverlayView>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.clone()
            }
        }

        let (host, cx) = cx.add_window_view(move |_, cx| {
            let overlay = cx.new(|cx| {
                OverlayView::new(
                    doc_clone,
                    Camera::new(Pt::ZERO, 1.0, viewport),
                    &GRAPHITE,
                    cx,
                )
                .with_paint_scope(scope_clone)
            });
            Host(overlay)
        });
        let overlay = host.read_with(cx, |h, _| h.0.clone());

        // Warm moved rigid label path (`pt(2, -3)` avoids `RB` at `(0..4, 2)`).
        overlay.update(cx, |o, cx| {
            assert!(o.set_move_preview(
                [ItemId::Part(r1), ItemId::Part(r2)],
                pt(2, -3),
                Orient::IDENTITY,
                false,
                cx,
            ));
        });
        cx.run_until_parked();

        scope.omnia_allocs.set(0);
        scope.omnia_calls.set(0);
        scope.counting.set(true);
        for (delta, expect_refused) in [
            (pt(1, 2), false),
            // `(14, -2)` puts R1's right pin at `(10, -2)` inside the 4-pin blocker cage.
            (pt(14, -2), true),
            (pt(2, 2), false),
        ] {
            overlay.update(cx, |o, cx| {
                assert!(o.set_move_preview(
                    [ItemId::Part(r1), ItemId::Part(r2)],
                    delta,
                    Orient::IDENTITY,
                    false,
                    cx,
                ));
                let crate::tool::ToolState::Dragging(drag) = o.tool_state() else {
                    panic!("expected Dragging");
                };
                assert_eq!(drag.is_refused(), expect_refused);
                assert!(!drag.affected_labels().is_empty());
            });
            cx.run_until_parked();
        }
        scope.counting.set(false);
        assert!(scope.omnia_calls.get() >= 3);
        assert_eq!(
            scope.omnia_allocs.get(),
            0,
            "B10: warm rigid-label follow overlay paint must make 0 Omnia allocations"
        );
    }

    /// W3.6 / B10 / WINV5: rerouted-run corner labels and pin-riding labels in `plan_labels_sorted`
    /// draw during a non-rigid follow drag with zero warm Omnia heap allocations.
    #[gpui::test]
    fn w3_6_b10_warm_rerouted_and_pin_label_follow_paint_makes_zero_omnia_allocations(
        cx: &mut TestAppContext,
    ) {
        let mut doc_val = Doc::new();
        let r1 = place_resistor(&mut doc_val, pt(-6, 0), "R1");
        let _r2 = place_resistor(&mut doc_val, pt(6, 4), "R2");
        doc_val
            .apply(Command::AddWires(vec![
                Wire {
                    a: pt(-4, 0),
                    b: pt(4, 0),
                },
                Wire {
                    a: pt(4, 0),
                    b: pt(4, 4),
                },
            ]))
            .unwrap();
        let ItemId::Label(corner_lid) = doc_val
            .apply(Command::AddLabel(Label {
                at: pt(4, 0),
                text: "CORNER".into(),
            }))
            .unwrap()
            .added[0]
        else {
            unreachable!();
        };
        let ItemId::Label(pin_lid) = doc_val
            .apply(Command::AddLabel(Label {
                at: pt(-4, 0),
                text: "PIN".into(),
            }))
            .unwrap()
            .added[0]
        else {
            unreachable!();
        };

        let doc = cx.new(|_| doc_val);
        let scope = Rc::new(FollowAllocScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));
        let doc_clone = doc.clone();
        let scope_clone = scope.clone();

        struct Host(gpui::Entity<OverlayView>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.clone()
            }
        }

        let (host, cx) = cx.add_window_view(move |_, cx| {
            let overlay = cx.new(|cx| {
                OverlayView::new(
                    doc_clone,
                    Camera::new(Pt::ZERO, 1.0, viewport),
                    &GRAPHITE,
                    cx,
                )
                .with_paint_scope(scope_clone)
            });
            Host(overlay)
        });
        let overlay = host.read_with(cx, |h, _| h.0.clone());

        // Warm non-rigid drag of only `R1`: reroutes the L-run, moves `corner_lid` to (4, 2)
        // and moves `pin_lid` with R1's right pin to (-3, 2).
        overlay.update(cx, |o, cx| {
            assert!(o.set_move_preview([ItemId::Part(r1)], pt(1, 2), Orient::IDENTITY, false, cx));
            let crate::tool::ToolState::Dragging(drag) = o.tool_state() else {
                panic!("expected Dragging");
            };
            assert!(drag.affected_labels().is_empty());
            assert_eq!(
                drag.plan_labels_sorted(),
                &[(corner_lid, pt(4, 2)), (pin_lid, pt(-3, 2))]
            );
        });
        cx.run_until_parked();

        scope.omnia_allocs.set(0);
        scope.omnia_calls.set(0);
        scope.counting.set(true);
        for (step, theme) in [(2, &GRAY), (3, &GRAPHITE), (1, &GRAY)] {
            overlay.update(cx, |o, cx| {
                o.set_theme(theme, cx);
                assert!(o.set_move_preview(
                    [ItemId::Part(r1)],
                    pt(step, 2),
                    Orient::IDENTITY,
                    false,
                    cx,
                ));
            });
            cx.run_until_parked();
        }
        scope.counting.set(false);
        assert!(scope.omnia_calls.get() >= 3);
        assert_eq!(
            scope.omnia_allocs.get(),
            0,
            "B10: warm rerouted/pin-label follow overlay paint must make 0 Omnia allocations"
        );
    }
}
