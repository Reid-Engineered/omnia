//! T5.3: ghost rendering for `Tool::Place` previews and drag-move selections (D7, K2, INV6, INV11).
//!
//! Renders transient placement and drag ghosts purely inside [`super::OverlayView`], above the
//! cached sheet. Uses the sheet's shared symbol alpha-mask sprites (`Window::paint_svg`) and
//! device-pixel-snapped quads (`Window::paint_quad`) inside two paint layers:
//! - Layer 1: translucent fills and wire halos (`stage/select-halo`, or `mark/conflict` when
//!   out of K1 sheet bounds).
//! - Layer 2: ghost wires, outlines, pin/label markers, and symbol sprites or LOD boxes
//!   (`stage/ink`, `stage/select`, `stage/snap`, or `mark/conflict`).
//!
//! All colours come strictly from `omnia-theme` tokens (INV6). No `paint_path` or `canvas()`
//! calls are used, and the warm paint path performs no heap allocations (B10).

use super::{
    LABEL_HALF_PX, OverlayPaintScope, PART_FRAME_PAD_PX, SELECT_OUTLINE_PX, gpui_color,
    label_rect_dev, padded_rect_dev, paint_quad_scoped, paint_solid_outline, run_gpui, run_omnia,
    wire_rect_dev,
};
use crate::{
    camera::Camera,
    sheet::{
        lod,
        symbols::{Masks, Shape},
        wires,
    },
    snap_px::{centered_square, match_parity, stroke_device_px},
};
use gpui::{App, Bounds, Hsla, Pixels, Window, fill, point, px};
use omnia_doc::{
    Clip, ClipPart, Doc, Isometry, ItemId, Label, LabelId, Orient, PartId, Pt, Rect,
    SHEET_LIMIT_NM, SymbolId, Wire, WireId, route::Blocker,
};
use omnia_symbols::SymbolDef;
use omnia_theme::Theme;

/// Extra logical thickness for the translucent halo behind dragged ghost wires.
const GHOST_WIRE_HALO_EXTRA_PX: f64 = 5.0;
/// Logical side of the anchor-pin marker (pin 0) on a `Tool::Place` preview.
const ANCHOR_PIN_MARKER_PX: f64 = 5.0;
/// Logical side of secondary pin markers on a `Tool::Place` preview.
const PIN_MARKER_PX: f64 = 3.5;
/// Logical side of the anchor dot on a dragged net-label ghost.
const LABEL_ANCHOR_DOT_PX: f64 = 3.0;
/// Half-extent in logical px of the conflict outline around a blocking pin (`WD7`, `TC-seg-03`).
const BLOCKER_PIN_HALF_PX: f64 = 6.0;
/// Logical side of the center marker square on a blocking pin (`WD7`).
const BLOCKER_PIN_DOT_PX: f64 = 4.0;
/// Logical padding around a blocking wire segment (`WD7`).
const BLOCKER_WIRE_PAD_PX: f64 = 4.0;
/// Logical outline thickness for conflict blocker frames (`WD7`, `WD11`).
const BLOCKER_OUTLINE_PX: f64 = 2.0;

/// Ghost opacity factors applied to `omnia-theme` tokens (INV6).
const GHOST_INK_ALPHA: f32 = 0.55;
const GHOST_SELECT_ALPHA: f32 = 0.85;
const GHOST_HALO_ALPHA: f32 = 0.22;
const GHOST_WIRE_HALO_ALPHA: f32 = 0.35;
const GHOST_SNAP_ALPHA: f32 = 0.90;
const CONFLICT_INK_ALPHA: f32 = 0.75;
const CONFLICT_OUTLINE_ALPHA: f32 = 0.90;
const CONFLICT_HALO_ALPHA: f32 = 0.22;

/// What ghost preview the overlay should draw for the current tool state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GhostPreview<'a> {
    None,
    Place {
        symbol: SymbolId,
        at: Pt,
        orient: Orient,
    },
    Drag {
        items: &'a [ItemId],
        transform: Isometry,
    },
    /// T7.1: copied-circuit paste preview (`D2`, placed like a move).
    Paste {
        clip: &'a Clip,
        at: Pt,
        orient: Orient,
    },
    /// W3.3: planned or refused wire-segment drag preview (`WD7`, `WD11`).
    Segment {
        wires: &'a [Wire],
        blocker: Option<Blocker>,
    },
}

impl GhostPreview<'_> {
    /// Returns `true` when there is a ghost preview to paint.
    pub(super) fn is_active(&self) -> bool {
        match self {
            Self::None => false,
            Self::Place { .. } => true,
            Self::Drag { items, .. } => !items.is_empty(),
            Self::Paste { clip, .. } => !clip.is_empty(),
            Self::Segment { wires, blocker } => !wires.is_empty() || blocker.is_some(),
        }
    }
}

/// Shared geometry, theme, mask, and accounting parameters for [`paint_ghosts`].
pub(super) struct GhostPaintParams<'a> {
    pub doc: &'a Doc,
    pub camera: &'a Camera,
    pub bounds: Bounds<Pixels>,
    pub visible_padded: Rect,
    pub sf: f32,
    pub clip: (f64, f64, f64, f64),
    pub theme: &'static Theme,
    pub masks: &'a Masks,
    pub scope: Option<&'a dyn OverlayPaintScope>,
}

/// Result of transforming a world point by an [`Isometry`] or offset without overflowing `i32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransformedPt {
    pt: Pt,
    in_bounds: bool,
}

/// Resolved ghost geometry for a dragged part.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GhostPart {
    symbol: SymbolId,
    at: Pt,
    orient: Orient,
    body: Option<Rect>,
    in_bounds: bool,
}

/// Resolved ghost geometry for a dragged wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GhostWire {
    wire: Wire,
    bounds: Rect,
    in_bounds: bool,
}

/// Resolved ghost geometry for a dragged net label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GhostLabel {
    at: Pt,
    in_bounds: bool,
}

/// Extracts the orientation component of an [`Isometry`].
fn isometry_orient(transform: Isometry) -> Orient {
    match transform {
        Isometry::Translate(_) => Orient::IDENTITY,
        Isometry::Reorient { by, .. } | Isometry::Transform { by, .. } => by,
    }
}

/// Applies `orient` to `(dx, dy)` in `i64`, matching `omnia_doc::Orient::apply` without overflow.
#[inline]
fn apply_orient_i64(orient: Orient, dx: i64, dy: i64) -> (i64, i64) {
    match orient.to_raw() {
        0 => (dx, dy),
        1 => (-dy, dx),
        2 => (-dx, -dy),
        3 => (dy, -dx),
        4 => (-dx, dy),
        5 => (-dy, -dx),
        6 => (dx, -dy),
        _ => (dy, dx),
    }
}

/// Clamps an `i64` nanometre coordinate to `i32` for screen projection and reports K1 validity.
#[inline]
fn to_transformed_pt(x64: i64, y64: i64) -> TransformedPt {
    let limit = i64::from(SHEET_LIMIT_NM);
    let in_bounds = (-limit..=limit).contains(&x64) && (-limit..=limit).contains(&y64);
    TransformedPt {
        pt: Pt {
            x: x64.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: y64.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        },
        in_bounds,
    }
}

/// Transforms a world point by `transform`, matching `omnia_doc::Isometry::apply` exactly when
/// in K1 bounds and clamping cleanly to `i32` with `in_bounds = false` otherwise.
fn transform_pt(transform: Isometry, p: Pt) -> TransformedPt {
    if let Ok(pt) = transform.apply(p) {
        return TransformedPt {
            pt,
            in_bounds: true,
        };
    }
    let (x64, y64) = match transform {
        Isometry::Translate(delta) => (
            i64::from(p.x) + i64::from(delta.x),
            i64::from(p.y) + i64::from(delta.y),
        ),
        Isometry::Reorient { pivot, by } => {
            let (rx, ry) = apply_orient_i64(
                by,
                i64::from(p.x) - i64::from(pivot.x),
                i64::from(p.y) - i64::from(pivot.y),
            );
            (i64::from(pivot.x) + rx, i64::from(pivot.y) + ry)
        }
        Isometry::Transform { delta, pivot, by } => {
            let (rx, ry) = apply_orient_i64(
                by,
                i64::from(p.x) - i64::from(pivot.x),
                i64::from(p.y) - i64::from(pivot.y),
            );
            (
                i64::from(pivot.x) + rx + i64::from(delta.x),
                i64::from(pivot.y) + ry + i64::from(delta.y),
            )
        }
    };
    to_transformed_pt(x64, y64)
}

/// Computes the world position of a symbol's local pin at `(at, orient)` without overflowing `i32`.
fn part_world_pin(at: Pt, orient: Orient, local_pin: Pt) -> TransformedPt {
    let offset = orient.apply(local_pin);
    to_transformed_pt(
        i64::from(at.x) + i64::from(offset.x),
        i64::from(at.y) + i64::from(offset.y),
    )
}

/// Computes the transformed axis-aligned bounds rectangle for `bounds` at `(at, orient)`.
fn oriented_bounds_rect(bounds: Rect, at: Pt, orient: Orient) -> Rect {
    let a = orient.apply(bounds.min);
    let b = orient.apply(bounds.max);
    let min_x = (i64::from(at.x) + i64::from(a.x.min(b.x)))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let max_x = (i64::from(at.x) + i64::from(a.x.max(b.x)))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let min_y = (i64::from(at.y) + i64::from(a.y.min(b.y)))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    let max_y = (i64::from(at.y) + i64::from(a.y.max(b.y)))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    Rect {
        min: Pt { x: min_x, y: min_y },
        max: Pt { x: max_x, y: max_y },
    }
}

/// Computes the transformed axis-aligned body rectangle for `def` at `(at, orient)`.
fn part_body_rect(def: &SymbolDef, at: Pt, orient: Orient) -> Rect {
    oriented_bounds_rect(def.bounds, at, orient)
}

/// Checks whether a `Tool::Place` preview at `(at, orient)` keeps its origin and every pin inside K1.
fn place_in_bounds(def: &SymbolDef, at: Pt, orient: Orient) -> bool {
    at.is_in_bounds()
        && def
            .pins
            .iter()
            .all(|pin| part_world_pin(at, orient, pin.at).in_bounds)
}

/// Resolves a dragged part's transformed placement, body bounds, and K1 validity.
fn resolve_drag_part(
    doc: &Doc,
    id: PartId,
    transform: Isometry,
    masks: &Masks,
) -> Option<GhostPart> {
    let part = doc.part(id)?;
    let moved_at = transform_pt(transform, part.at);
    let orient = part.orient.then(isometry_orient(transform));
    let pins_ok = doc
        .pins(id)
        .is_none_or(|pins| pins.iter().all(|&pin| transform.apply(pin).is_ok()));
    let body = if let Some(def) = masks.symbol_def(part.symbol) {
        Some(part_body_rect(def, moved_at.pt, orient))
    } else {
        doc.part_body(id).map(|rect| {
            let a = transform_pt(transform, rect.min).pt;
            let b = transform_pt(transform, rect.max).pt;
            Rect {
                min: Pt {
                    x: a.x.min(b.x),
                    y: a.y.min(b.y),
                },
                max: Pt {
                    x: a.x.max(b.x),
                    y: a.y.max(b.y),
                },
            }
        })
    };
    Some(GhostPart {
        symbol: part.symbol,
        at: moved_at.pt,
        orient,
        body,
        in_bounds: moved_at.in_bounds && pins_ok,
    })
}

/// Resolves a dragged wire's transformed endpoints, bounding box, and K1 validity.
fn resolve_drag_wire(doc: &Doc, id: WireId, transform: Isometry) -> Option<GhostWire> {
    let wire = *doc.wire(id)?;
    let a = transform_pt(transform, wire.a);
    let b = transform_pt(transform, wire.b);
    let moved = Wire { a: a.pt, b: b.pt };
    let bounds = Rect {
        min: Pt {
            x: moved.a.x.min(moved.b.x),
            y: moved.a.y.min(moved.b.y),
        },
        max: Pt {
            x: moved.a.x.max(moved.b.x),
            y: moved.a.y.max(moved.b.y),
        },
    };
    Some(GhostWire {
        wire: moved,
        bounds,
        in_bounds: a.in_bounds && b.in_bounds,
    })
}

/// Resolves a dragged net label's transformed anchor and K1 validity.
fn resolve_drag_label(doc: &Doc, id: LabelId, transform: Isometry) -> Option<GhostLabel> {
    let label = doc.label(id)?;
    let moved = transform_pt(transform, label.at);
    Some(GhostLabel {
        at: moved.pt,
        in_bounds: moved.in_bounds,
    })
}

/// Resolves a pasted part's transformed placement, body bounds, and K1 validity without allocating.
fn resolve_paste_part(cp: &ClipPart, paste_at: Pt, orient: Orient, masks: &Masks) -> GhostPart {
    let (rx, ry) = apply_orient_i64(orient, i64::from(cp.part.at.x), i64::from(cp.part.at.y));
    let moved = to_transformed_pt(i64::from(paste_at.x) + rx, i64::from(paste_at.y) + ry);
    let part_orient = cp.part.orient.then(orient);
    let pins_ok = cp.local_pins.iter().all(|&local_pin| {
        local_pin.is_in_bounds() && part_world_pin(moved.pt, part_orient, local_pin).in_bounds
    });
    let bounds_ok = cp.local_bounds.is_in_bounds()
        && part_world_pin(moved.pt, part_orient, cp.local_bounds.min).in_bounds
        && part_world_pin(moved.pt, part_orient, cp.local_bounds.max).in_bounds;
    let body = if let Some(def) = masks.symbol_def(cp.part.symbol) {
        Some(part_body_rect(def, moved.pt, part_orient))
    } else {
        Some(oriented_bounds_rect(cp.local_bounds, moved.pt, part_orient))
    };
    GhostPart {
        symbol: cp.part.symbol,
        at: moved.pt,
        orient: part_orient,
        body,
        in_bounds: moved.in_bounds && pins_ok && bounds_ok,
    }
}

/// Resolves a pasted wire's transformed endpoints, bounding box, and K1 validity without allocating.
fn resolve_paste_wire(wire: Wire, paste_at: Pt, orient: Orient) -> GhostWire {
    let (ax, ay) = apply_orient_i64(orient, i64::from(wire.a.x), i64::from(wire.a.y));
    let (bx, by) = apply_orient_i64(orient, i64::from(wire.b.x), i64::from(wire.b.y));
    let a = to_transformed_pt(i64::from(paste_at.x) + ax, i64::from(paste_at.y) + ay);
    let b = to_transformed_pt(i64::from(paste_at.x) + bx, i64::from(paste_at.y) + by);
    let moved = Wire { a: a.pt, b: b.pt };
    GhostWire {
        wire: moved,
        bounds: wire_bounds(moved),
        in_bounds: a.in_bounds && b.in_bounds,
    }
}

/// Resolves a pasted net label's transformed anchor and K1 validity without allocating.
fn resolve_paste_label(label: &Label, paste_at: Pt, orient: Orient) -> GhostLabel {
    let (lx, ly) = apply_orient_i64(orient, i64::from(label.at.x), i64::from(label.at.y));
    let moved = to_transformed_pt(i64::from(paste_at.x) + lx, i64::from(paste_at.y) + ly);
    GhostLabel {
        at: moved.pt,
        in_bounds: moved.in_bounds,
    }
}

/// Palette derived exclusively from `omnia-theme` tokens (INV6).
#[derive(Clone, Copy)]
struct GhostPalette {
    ink: Hsla,
    select: Hsla,
    halo: Hsla,
    wire_halo: Hsla,
    snap: Hsla,
    conflict_ink: Hsla,
    conflict_outline: Hsla,
    conflict_halo: Hsla,
}

impl GhostPalette {
    fn from_theme(theme: &'static Theme) -> Self {
        let ink = gpui_color(theme.stage_ink);
        let select = gpui_color(theme.stage_select);
        let halo = gpui_color(theme.stage_select_halo);
        let snap = gpui_color(theme.stage_snap);
        let conflict = gpui_color(theme.mark_conflict);
        Self {
            ink: ink.opacity(GHOST_INK_ALPHA),
            select: select.opacity(GHOST_SELECT_ALPHA),
            halo: halo.opacity(GHOST_HALO_ALPHA),
            wire_halo: halo.opacity(GHOST_WIRE_HALO_ALPHA),
            snap: snap.opacity(GHOST_SNAP_ALPHA),
            conflict_ink: conflict.opacity(CONFLICT_INK_ALPHA),
            conflict_outline: conflict.opacity(CONFLICT_OUTLINE_ALPHA),
            conflict_halo: conflict.opacity(CONFLICT_HALO_ALPHA),
        }
    }

    #[inline]
    fn ink_for(self, in_bounds: bool) -> Hsla {
        if in_bounds {
            self.ink
        } else {
            self.conflict_ink
        }
    }

    #[inline]
    fn outline_for(self, in_bounds: bool) -> Hsla {
        if in_bounds {
            self.select
        } else {
            self.conflict_outline
        }
    }

    #[inline]
    fn halo_for(self, in_bounds: bool) -> Hsla {
        if in_bounds {
            self.halo
        } else {
            self.conflict_halo
        }
    }

    #[inline]
    fn wire_halo_for(self, in_bounds: bool) -> Hsla {
        if in_bounds {
            self.wire_halo
        } else {
            self.conflict_halo
        }
    }
}

/// Paints a single symbol ghost as an atlas sprite or LOD square box (matching `SheetView`).
fn paint_symbol_shape(
    shape: Shape<'_>,
    color: Hsla,
    sf: f32,
    scope: Option<&dyn OverlayPaintScope>,
    window: &mut Window,
    cx: &App,
) {
    if lod::symbol_box(shape.logical_side) {
        let side = stroke_device_px(shape.logical_side, sf);
        let scale = f64::from(sf);
        paint_quad_scoped(
            window,
            fill(
                centered_square(point(shape.x / scale, shape.y / scale), side, sf),
                color,
            ),
            scope,
        );
    } else {
        let (sprite_bounds, matrix) = shape.sprite(sf);
        let key = shape.mask.key.clone();
        run_gpui(scope, || {
            if let Err(error) =
                window.paint_svg(sprite_bounds, key, Some(&shape.mask.svg), matrix, color, cx)
            {
                eprintln!("ghost mask: {error}");
            }
        });
    }
}

/// Computes the device-pixel frame around a WD4 refusal blocker (plus the exact pin point when
/// blocked by `Blocker::Pin`), allocating nothing (WD7, WD11, B10).
fn blocker_dev_rect(
    blocker: Blocker,
    params: &GhostPaintParams<'_>,
) -> Option<(super::DevRect, Option<Pt>)> {
    match blocker {
        Blocker::Pin { part, pin } => {
            let at = *params.doc.pins(part)?.get(pin)?;
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
        Blocker::WireEnd(id) | Blocker::WireBody(id) | Blocker::Overlap(id) => {
            let wire = *params.doc.wire(id)?;
            let bounds = wire_bounds(wire);
            if !bounds.intersects(params.visible_padded) {
                return None;
            }
            padded_rect_dev(
                bounds,
                BLOCKER_WIRE_PAD_PX,
                params.camera,
                params.sf,
                params.clip,
            )
            .map(|rect| (rect, None))
        }
        Blocker::Sheet => None,
    }
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

fn paint_ghost_part_halo(
    ghost: GhostPart,
    params: &GhostPaintParams<'_>,
    palette: GhostPalette,
    window: &mut Window,
) {
    let Some(body) = ghost.body else {
        return;
    };
    if !body.intersects(params.visible_padded) {
        return;
    }
    if let Some(rect) = padded_rect_dev(
        body,
        PART_FRAME_PAD_PX,
        params.camera,
        params.sf,
        params.clip,
    ) {
        paint_quad_scoped(
            window,
            fill(rect.bounds(params.sf), palette.halo_for(ghost.in_bounds)),
            params.scope,
        );
    }
}

fn paint_ghost_wire_halo(
    ghost: GhostWire,
    wire_halo_dev: u32,
    params: &GhostPaintParams<'_>,
    palette: GhostPalette,
    window: &mut Window,
) {
    if !ghost.bounds.intersects(params.visible_padded) {
        return;
    }
    if let Some(rect) = wire_rect_dev(
        ghost.wire,
        params.camera,
        params.sf,
        wire_halo_dev,
        params.clip,
    ) {
        paint_quad_scoped(
            window,
            fill(
                rect.bounds(params.sf),
                palette.wire_halo_for(ghost.in_bounds),
            ),
            params.scope,
        );
    }
}

fn paint_ghost_label_halo(
    ghost: GhostLabel,
    params: &GhostPaintParams<'_>,
    palette: GhostPalette,
    window: &mut Window,
) {
    if !params.visible_padded.contains(ghost.at) {
        return;
    }
    if let Some(rect) = label_rect_dev(
        ghost.at,
        LABEL_HALF_PX,
        params.camera,
        params.sf,
        params.clip,
    ) {
        paint_quad_scoped(
            window,
            fill(rect.bounds(params.sf), palette.halo_for(ghost.in_bounds)),
            params.scope,
        );
    }
}

fn paint_ghost_part_fg(
    ghost: GhostPart,
    outline_dev: u32,
    params: &GhostPaintParams<'_>,
    palette: GhostPalette,
    window: &mut Window,
    cx: &App,
) {
    if ghost
        .body
        .is_some_and(|body| !body.intersects(params.visible_padded))
    {
        return;
    }
    if let Some(body) = ghost.body
        && let Some(rect) = padded_rect_dev(
            body,
            PART_FRAME_PAD_PX,
            params.camera,
            params.sf,
            params.clip,
        )
    {
        paint_solid_outline(
            rect,
            outline_dev,
            palette.outline_for(ghost.in_bounds),
            params.sf,
            params.scope,
            window,
        );
    }
    if let Some(shape) = params.masks.symbol_at(
        ghost.symbol,
        ghost.at,
        ghost.orient,
        params.camera,
        params.sf,
    ) {
        paint_symbol_shape(
            shape,
            palette.ink_for(ghost.in_bounds),
            params.sf,
            params.scope,
            window,
            cx,
        );
    }
}

fn paint_ghost_wire_fg(
    ghost: GhostWire,
    wire_width_dev: u32,
    params: &GhostPaintParams<'_>,
    palette: GhostPalette,
    window: &mut Window,
) {
    if !ghost.bounds.intersects(params.visible_padded) {
        return;
    }
    if let Some(rect) = wire_rect_dev(
        ghost.wire,
        params.camera,
        params.sf,
        wire_width_dev,
        params.clip,
    ) {
        paint_quad_scoped(
            window,
            fill(rect.bounds(params.sf), palette.ink_for(ghost.in_bounds)),
            params.scope,
        );
    }
}

fn paint_ghost_label_fg(
    ghost: GhostLabel,
    outline_dev: u32,
    label_dot_dev: u32,
    params: &GhostPaintParams<'_>,
    palette: GhostPalette,
    window: &mut Window,
) {
    if !params.visible_padded.contains(ghost.at) {
        return;
    }
    let color = palette.outline_for(ghost.in_bounds);
    if let Some(rect) = label_rect_dev(
        ghost.at,
        LABEL_HALF_PX,
        params.camera,
        params.sf,
        params.clip,
    ) {
        paint_solid_outline(rect, outline_dev, color, params.sf, params.scope, window);
    }
    let screen = params.camera.world_to_screen(ghost.at);
    paint_quad_scoped(
        window,
        fill(centered_square(screen, label_dot_dev, params.sf), color),
        params.scope,
    );
}

/// Paints the active placement, drag-move, or segment-drag ghost preview inside its own GPUI paint
/// layers.
///
/// Must be called from `paint_overlay` inside its content-masked `run_gpui` block.
pub(super) fn paint_ghosts(
    preview: GhostPreview<'_>,
    params: &GhostPaintParams<'_>,
    window: &mut Window,
    cx: &App,
) {
    if !preview.is_active()
        || params.bounds.size.width <= px(0.0)
        || params.bounds.size.height <= px(0.0)
    {
        return;
    }

    let palette = GhostPalette::from_theme(params.theme);
    let wire_width_dev = wires::width(params.camera, params.sf);
    let wire_base_px = wires::WIDTH_NM * params.camera.scale();
    let wire_halo_dev = match_parity(
        stroke_device_px(wire_base_px + GHOST_WIRE_HALO_EXTRA_PX, params.sf)
            .max(wire_width_dev.saturating_add(2)),
        wire_width_dev,
    );
    let outline_dev = stroke_device_px(SELECT_OUTLINE_PX, params.sf);
    let blocker_outline_dev = stroke_device_px(BLOCKER_OUTLINE_PX, params.sf);
    let blocker_pin_dot_dev = stroke_device_px(BLOCKER_PIN_DOT_PX, params.sf);
    let anchor_pin_dev = stroke_device_px(ANCHOR_PIN_MARKER_PX, params.sf);
    let pin_dev = stroke_device_px(PIN_MARKER_PX, params.sf);
    let label_dot_dev = stroke_device_px(LABEL_ANCHOR_DOT_PX, params.sf);

    match preview {
        GhostPreview::None => {}
        GhostPreview::Place { symbol, at, orient } => {
            let Some(def) = params.masks.symbol_def(symbol) else {
                return;
            };
            let body = part_body_rect(def, at, orient);
            if !body.intersects(params.visible_padded) {
                return;
            }
            let in_bounds = place_in_bounds(def, at, orient);

            // Layer 1: translucent placement fill underneath the symbol sprite.
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    if let Some(rect) = padded_rect_dev(
                        body,
                        PART_FRAME_PAD_PX,
                        params.camera,
                        params.sf,
                        params.clip,
                    ) {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(params.sf), palette.halo_for(in_bounds)),
                            params.scope,
                        );
                    }
                });
            });

            // Layer 2: symbol sprite (or LOD box), body outline, and pin markers (anchor pin 0
            // in `stage/snap`, remaining pins in `stage/select`).
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    if let Some(rect) = padded_rect_dev(
                        body,
                        PART_FRAME_PAD_PX,
                        params.camera,
                        params.sf,
                        params.clip,
                    ) {
                        paint_solid_outline(
                            rect,
                            outline_dev,
                            palette.outline_for(in_bounds),
                            params.sf,
                            params.scope,
                            window,
                        );
                    }
                    let shape =
                        params
                            .masks
                            .symbol_at(symbol, at, orient, params.camera, params.sf);
                    let show_pins = shape
                        .as_ref()
                        .is_some_and(|s| !lod::symbol_box(s.logical_side));
                    if show_pins {
                        for (idx, pin) in def.pins.iter().enumerate() {
                            let world_pin = part_world_pin(at, orient, pin.at);
                            let screen = params.camera.world_to_screen(world_pin.pt);
                            let (side, color) = if idx == 0 {
                                (
                                    anchor_pin_dev,
                                    if in_bounds && world_pin.in_bounds {
                                        palette.snap
                                    } else {
                                        palette.conflict_outline
                                    },
                                )
                            } else {
                                (
                                    pin_dev,
                                    palette.outline_for(in_bounds && world_pin.in_bounds),
                                )
                            };
                            paint_quad_scoped(
                                window,
                                fill(centered_square(screen, side, params.sf), color),
                                params.scope,
                            );
                        }
                    }
                    if let Some(shape) = shape {
                        paint_symbol_shape(
                            shape,
                            palette.ink_for(in_bounds),
                            params.sf,
                            params.scope,
                            window,
                            cx,
                        );
                    }
                });
            });
        }
        GhostPreview::Drag { items, transform } => {
            // Layer 1: translucent fills and wire halos underneath crisp ghost geometry.
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    for &item in items {
                        match item {
                            ItemId::Part(id) => {
                                if let Some(ghost) =
                                    resolve_drag_part(params.doc, id, transform, params.masks)
                                {
                                    paint_ghost_part_halo(ghost, params, palette, window);
                                }
                            }
                            ItemId::Wire(id) => {
                                if let Some(ghost) = resolve_drag_wire(params.doc, id, transform) {
                                    paint_ghost_wire_halo(
                                        ghost,
                                        wire_halo_dev,
                                        params,
                                        palette,
                                        window,
                                    );
                                }
                            }
                            ItemId::Label(id) => {
                                if let Some(ghost) = resolve_drag_label(params.doc, id, transform) {
                                    paint_ghost_label_halo(ghost, params, palette, window);
                                }
                            }
                        }
                    }
                });
            });

            // Layer 2: ghost wires, outlines, label anchors, and symbol sprites/LOD boxes.
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    for &item in items {
                        match item {
                            ItemId::Part(id) => {
                                if let Some(ghost) =
                                    resolve_drag_part(params.doc, id, transform, params.masks)
                                {
                                    paint_ghost_part_fg(
                                        ghost,
                                        outline_dev,
                                        params,
                                        palette,
                                        window,
                                        cx,
                                    );
                                }
                            }
                            ItemId::Wire(id) => {
                                if let Some(ghost) = resolve_drag_wire(params.doc, id, transform) {
                                    paint_ghost_wire_fg(
                                        ghost,
                                        wire_width_dev,
                                        params,
                                        palette,
                                        window,
                                    );
                                }
                            }
                            ItemId::Label(id) => {
                                if let Some(ghost) = resolve_drag_label(params.doc, id, transform) {
                                    paint_ghost_label_fg(
                                        ghost,
                                        outline_dev,
                                        label_dot_dev,
                                        params,
                                        palette,
                                        window,
                                    );
                                }
                            }
                        }
                    }
                });
            });
        }
        GhostPreview::Paste { clip, at, orient } => {
            // Layer 1: translucent fills and wire halos underneath crisp ghost geometry (D2).
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    for cp in clip.parts() {
                        let ghost = resolve_paste_part(cp, at, orient, params.masks);
                        paint_ghost_part_halo(ghost, params, palette, window);
                    }
                    for &wire in clip.wires() {
                        let ghost = resolve_paste_wire(wire, at, orient);
                        paint_ghost_wire_halo(ghost, wire_halo_dev, params, palette, window);
                    }
                    for label in clip.labels() {
                        let ghost = resolve_paste_label(label, at, orient);
                        paint_ghost_label_halo(ghost, params, palette, window);
                    }
                });
            });

            // Layer 2: ghost wires, outlines, label anchors, and symbol sprites/LOD boxes.
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    for cp in clip.parts() {
                        let ghost = resolve_paste_part(cp, at, orient, params.masks);
                        paint_ghost_part_fg(ghost, outline_dev, params, palette, window, cx);
                    }
                    for &wire in clip.wires() {
                        let ghost = resolve_paste_wire(wire, at, orient);
                        paint_ghost_wire_fg(ghost, wire_width_dev, params, palette, window);
                    }
                    for label in clip.labels() {
                        let ghost = resolve_paste_label(label, at, orient);
                        paint_ghost_label_fg(
                            ghost,
                            outline_dev,
                            label_dot_dev,
                            params,
                            palette,
                            window,
                        );
                    }
                });
            });
        }
        GhostPreview::Segment { wires, blocker } => {
            let valid = blocker.is_none();
            let halo_color = palette.wire_halo_for(valid);
            let wire_color = palette.ink_for(valid);

            // Layer 1: translucent wire halos and blocker highlight fill (`stage/select-halo` or
            // `mark/conflict`, INV6).
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    for &wire in wires {
                        if !wire_bounds(wire).intersects(params.visible_padded) {
                            continue;
                        }
                        if let Some(rect) = wire_rect_dev(
                            wire,
                            params.camera,
                            params.sf,
                            wire_halo_dev,
                            params.clip,
                        ) {
                            paint_quad_scoped(
                                window,
                                fill(rect.bounds(params.sf), halo_color),
                                params.scope,
                            );
                        }
                    }
                    if let Some(blocker) = blocker
                        && let Some((rect, _)) = blocker_dev_rect(blocker, params)
                    {
                        paint_quad_scoped(
                            window,
                            fill(rect.bounds(params.sf), palette.conflict_halo),
                            params.scope,
                        );
                    }
                });
            });

            // Layer 2: planned/attempted wire strokes and crisp blocker outline in `mark/conflict`
            // (WD7, WD11, INV6).
            window.paint_layer(|window| {
                run_omnia(params.scope, || {
                    for &wire in wires {
                        if !wire_bounds(wire).intersects(params.visible_padded) {
                            continue;
                        }
                        if let Some(rect) = wire_rect_dev(
                            wire,
                            params.camera,
                            params.sf,
                            wire_width_dev,
                            params.clip,
                        ) {
                            paint_quad_scoped(
                                window,
                                fill(rect.bounds(params.sf), wire_color),
                                params.scope,
                            );
                        }
                    }
                    if let Some(blocker) = blocker
                        && let Some((rect, pin_at)) = blocker_dev_rect(blocker, params)
                    {
                        paint_solid_outline(
                            rect,
                            blocker_outline_dev,
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
                                    centered_square(screen, blocker_pin_dot_dev, params.sf),
                                    palette.conflict_outline,
                                ),
                                params.scope,
                            );
                        }
                    }
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay::viewport_clip_dev;
    use gpui::{
        App, Bounds, ContentMask, Context, Element, ElementId, GlobalElementId, InspectorElementId,
        IntoElement, LayoutId, ParentElement, Pixels, Render, Style, Styled, TestAppContext,
        Window, div, point, px, relative, size,
    };
    use omnia_doc::{
        Command, GRID_NM, Label, LabelId, Part, PartId, Wire, WireId, prefs::FollowPolicy, snap_to,
    };
    use omnia_symbols::SymbolLibrary;
    use omnia_theme::{GRAPHITE, GRAY};
    use std::{cell::Cell, rc::Rc};

    fn sample_doc() -> Doc {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let res_def = lib.get(res);
        let opamp_def = lib.get(opamp);
        let g = GRID_NM;

        let parts = [
            (
                Part {
                    symbol: res,
                    at: Pt { x: -4 * g, y: 0 },
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                res_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                res_def.bounds,
            ),
            (
                Part {
                    symbol: opamp,
                    at: Pt { x: g / 2, y: g / 2 },
                    orient: Orient::ROT_90,
                    designator: "U1".into(),
                    value: "".into(),
                },
                opamp_def.pins.iter().map(|p| p.at).collect::<Vec<_>>(),
                opamp_def.bounds,
            ),
        ];
        let wires = [Wire {
            a: Pt { x: 0, y: 4 * g },
            b: Pt { x: 6 * g, y: 4 * g },
        }];
        let labels = [Label {
            at: Pt { x: 2 * g, y: 6 * g },
            text: "OUT".into(),
        }];
        Doc::from_items(parts, wires, labels).unwrap()
    }

    #[test]
    fn transform_pt_matches_omnia_doc_isometry_for_all_orients() {
        let g = GRID_NM;
        let pivot = Pt {
            x: 3 * g,
            y: -2 * g,
        };
        let delta = Pt {
            x: -5 * g,
            y: 4 * g,
        };
        let points = [
            Pt::ZERO,
            pivot,
            Pt {
                x: -7 * g,
                y: 9 * g,
            },
            Pt {
                x: 11 * g / 2,
                y: -3 * g / 2,
            },
        ];
        for by in Orient::ALL {
            let isometries = [
                Isometry::Translate(delta),
                Isometry::Reorient { pivot, by },
                Isometry::Transform { delta, pivot, by },
            ];
            for transform in isometries {
                for &p in &points {
                    let expected = transform.apply(p).unwrap();
                    let actual = transform_pt(transform, p);
                    assert_eq!(actual.pt, expected);
                    assert!(actual.in_bounds);
                }
            }
        }
    }

    #[test]
    fn transform_pt_marks_out_of_bounds_without_overflow() {
        let edge = Pt {
            x: SHEET_LIMIT_NM,
            y: -SHEET_LIMIT_NM,
        };
        let pushed = transform_pt(
            Isometry::Translate(Pt {
                x: GRID_NM,
                y: -GRID_NM,
            }),
            edge,
        );
        assert!(!pushed.in_bounds);
        assert_eq!(
            pushed.pt,
            Pt {
                x: SHEET_LIMIT_NM + GRID_NM,
                y: -SHEET_LIMIT_NM - GRID_NM,
            }
        );

        let extreme = transform_pt(
            Isometry::Transform {
                delta: Pt {
                    x: SHEET_LIMIT_NM,
                    y: SHEET_LIMIT_NM,
                },
                pivot: Pt {
                    x: -SHEET_LIMIT_NM,
                    y: -SHEET_LIMIT_NM,
                },
                by: Orient::ROT_180,
            },
            edge,
        );
        assert!(!extreme.in_bounds);
    }

    #[test]
    fn drag_ghost_geometry_matches_committed_command_transform_exactly() {
        let g = GRID_NM;
        let masks = Masks::new();
        let items = [
            ItemId::Part(PartId(0)),
            ItemId::Part(PartId(1)),
            ItemId::Wire(WireId(0)),
            ItemId::Label(LabelId(0)),
        ];
        let pivot = Pt { x: g, y: -g };
        let delta = Pt {
            x: 4 * g,
            y: -3 * g,
        };

        for by in Orient::ALL {
            let mut doc = sample_doc();
            let transform = Isometry::Transform { delta, pivot, by };
            let ghost_p0 = resolve_drag_part(&doc, PartId(0), transform, &masks).unwrap();
            let ghost_p1 = resolve_drag_part(&doc, PartId(1), transform, &masks).unwrap();
            let ghost_w0 = resolve_drag_wire(&doc, WireId(0), transform).unwrap();
            let ghost_l0 = resolve_drag_label(&doc, LabelId(0), transform).unwrap();

            doc.apply(Command::Transform {
                items: items.to_vec(),
                delta,
                pivot,
                by,
                follow: FollowPolicy::new(false, 1, false).unwrap(),
            })
            .unwrap();

            let p0 = doc.part(PartId(0)).unwrap();
            assert_eq!((ghost_p0.at, ghost_p0.orient), (p0.at, p0.orient));
            assert_eq!(ghost_p0.body, doc.part_body(PartId(0)));
            assert!(ghost_p0.in_bounds);

            let p1 = doc.part(PartId(1)).unwrap();
            assert_eq!((ghost_p1.at, ghost_p1.orient), (p1.at, p1.orient));
            assert_eq!(ghost_p1.body, doc.part_body(PartId(1)));
            assert!(ghost_p1.in_bounds);

            let w0 = *doc.wire(WireId(0)).unwrap();
            assert_eq!(
                (
                    ghost_w0.wire.a.x.min(ghost_w0.wire.b.x),
                    ghost_w0.wire.a.y.min(ghost_w0.wire.b.y),
                    ghost_w0.wire.a.x.max(ghost_w0.wire.b.x),
                    ghost_w0.wire.a.y.max(ghost_w0.wire.b.y),
                ),
                (
                    w0.a.x.min(w0.b.x),
                    w0.a.y.min(w0.b.y),
                    w0.a.x.max(w0.b.x),
                    w0.a.y.max(w0.b.y),
                )
            );
            assert!(ghost_w0.in_bounds);

            let l0 = doc.label(LabelId(0)).unwrap();
            assert_eq!(ghost_l0.at, l0.at);
            assert!(ghost_l0.in_bounds);
        }
    }

    #[test]
    fn place_preview_anchor_pin_snapping_and_world_pins_match_library() {
        let lib = SymbolLibrary::builtin();
        let g = GRID_NM;
        let raw_cursor = Pt {
            x: 3 * g + 400_000,
            y: -2 * g - 500_000,
        };

        for def in lib.iter() {
            let local_anchor = def.pins.first().map_or(Pt::ZERO, |pin| pin.at);
            for orient in Orient::ALL {
                let offset = orient.apply(local_anchor);
                let pin_raw = Pt {
                    x: raw_cursor.x + offset.x,
                    y: raw_cursor.y + offset.y,
                };
                let snapped_pin = snap_to(pin_raw, g);
                let part_at = Pt {
                    x: snapped_pin.x - offset.x,
                    y: snapped_pin.y - offset.y,
                };

                assert!(place_in_bounds(def, part_at, orient));
                if let Some(pin0) = def.pins.first() {
                    let world_pin0 = part_world_pin(part_at, orient, pin0.at);
                    assert!(world_pin0.in_bounds);
                    assert_eq!(world_pin0.pt, snapped_pin);
                    assert_eq!(world_pin0.pt.x % g, 0);
                    assert_eq!(world_pin0.pt.y % g, 0);
                }

                // Compare every ghost preview pin and body rect with `SymbolLibrary::place_command`.
                let mut doc = Doc::new();
                let cmd = lib.place_command(def.id, part_at, orient, "X1".into(), "".into());
                let change = doc.apply(cmd).unwrap();
                let [ItemId::Part(id)] = change.added[..] else {
                    panic!("expected one placed part");
                };
                let expected_pins = doc.pins(id).unwrap();
                let ghost_pins: Vec<Pt> = def
                    .pins
                    .iter()
                    .map(|pin| part_world_pin(part_at, orient, pin.at).pt)
                    .collect();
                assert_eq!(ghost_pins, expected_pins);
                assert_eq!(
                    part_body_rect(def, part_at, orient),
                    doc.part_body(id).unwrap()
                );
            }
        }
    }

    #[test]
    fn drag_part_near_sheet_edge_flags_conflict_when_pin_leaves_k1() {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let def = lib.get(res);
        let masks = Masks::new();
        let edge_at = Pt {
            x: SHEET_LIMIT_NM - 3 * GRID_NM,
            y: 0,
        };
        let mut doc = Doc::new();
        doc.apply(lib.place_command(res, edge_at, Orient::IDENTITY, "R1".into(), "".into()))
            .unwrap();

        // Moving 2 grid steps right puts pin at +2 grid steps past SHEET_LIMIT_NM while `part.at`
        // is still inside K1: the ghost must still flag `in_bounds == false`.
        let delta = Pt {
            x: 2 * GRID_NM,
            y: 0,
        };
        let ghost = resolve_drag_part(&doc, PartId(0), Isometry::Translate(delta), &masks).unwrap();
        assert!(ghost.at.is_in_bounds());
        assert!(!ghost.in_bounds);
        assert!(!place_in_bounds(def, ghost.at, ghost.orient));
    }

    struct CountingScope {
        omnia_calls: Cell<u32>,
        gpui_calls: Cell<u32>,
    }

    impl CountingScope {
        fn new() -> Self {
            Self {
                omnia_calls: Cell::new(0),
                gpui_calls: Cell::new(0),
            }
        }
    }

    impl OverlayPaintScope for CountingScope {
        fn omnia(&self, work: &mut dyn FnMut()) {
            self.omnia_calls.set(self.omnia_calls.get() + 1);
            work();
        }

        fn gpui(&self, work: &mut dyn FnMut()) {
            self.gpui_calls.set(self.gpui_calls.get() + 1);
            work();
        }
    }

    #[derive(Clone)]
    enum OwnedPreview {
        None,
        Place {
            symbol: SymbolId,
            at: Pt,
            orient: Orient,
        },
        Drag {
            items: Vec<ItemId>,
            transform: Isometry,
        },
        Paste {
            clip: Clip,
            at: Pt,
            orient: Orient,
        },
        Segment {
            wires: Vec<Wire>,
            blocker: Option<Blocker>,
        },
    }

    struct GhostTestView {
        doc: Rc<Doc>,
        camera: Camera,
        theme: &'static Theme,
        masks: Rc<Masks>,
        preview: OwnedPreview,
        scope: Rc<CountingScope>,
    }

    impl Render for GhostTestView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(GhostTestElement {
                doc: self.doc.clone(),
                camera: self.camera.clone(),
                theme: self.theme,
                masks: self.masks.clone(),
                preview: self.preview.clone(),
                scope: self.scope.clone(),
            })
        }
    }

    struct GhostTestElement {
        doc: Rc<Doc>,
        camera: Camera,
        theme: &'static Theme,
        masks: Rc<Masks>,
        preview: OwnedPreview,
        scope: Rc<CountingScope>,
    }

    impl Element for GhostTestElement {
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
            cx: &mut App,
        ) {
            let mut camera = self.camera.clone();
            camera.set_viewport(bounds);
            let sf = window.scale_factor();
            let clip = viewport_clip_dev(bounds, sf, 32.0);
            let params = GhostPaintParams {
                doc: &self.doc,
                camera: &camera,
                bounds,
                visible_padded: camera.visible(),
                sf,
                clip,
                theme: self.theme,
                masks: &self.masks,
                scope: Some(self.scope.as_ref()),
            };
            let preview = match &self.preview {
                OwnedPreview::None => GhostPreview::None,
                OwnedPreview::Place { symbol, at, orient } => GhostPreview::Place {
                    symbol: *symbol,
                    at: *at,
                    orient: *orient,
                },
                OwnedPreview::Drag { items, transform } => GhostPreview::Drag {
                    items,
                    transform: *transform,
                },
                OwnedPreview::Paste { clip, at, orient } => GhostPreview::Paste {
                    clip,
                    at: *at,
                    orient: *orient,
                },
                OwnedPreview::Segment { wires, blocker } => GhostPreview::Segment {
                    wires,
                    blocker: *blocker,
                },
            };
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                paint_ghosts(preview, &params, window, cx);
            });
        }
    }

    #[gpui::test]
    fn paint_ghosts_renders_place_and_drag_in_both_themes_and_lod_modes(cx: &mut TestAppContext) {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let masks = Rc::new(Masks::new());
        let doc = Rc::new(sample_doc());
        let scope = Rc::new(CountingScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));

        let (view, cx) = cx.add_window_view(|_, _| GhostTestView {
            doc,
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            theme: &GRAPHITE,
            masks,
            preview: OwnedPreview::None,
            scope: scope.clone(),
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 0);
        assert_eq!(scope.gpui_calls.get(), 0);

        // Place preview at normal zoom (sprite + outline + pin markers).
        view.update(cx, |v, cx| {
            v.preview = OwnedPreview::Place {
                symbol: res,
                at: Pt::ZERO,
                orient: Orient::ROT_90,
            };
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 2);
        assert!(scope.gpui_calls.get() > 0);

        // Drag preview of parts, wire, and label with combined Transform in GRAY theme.
        scope.omnia_calls.set(0);
        scope.gpui_calls.set(0);
        view.update(cx, |v, cx| {
            v.theme = &GRAY;
            v.preview = OwnedPreview::Drag {
                items: vec![
                    ItemId::Part(PartId(0)),
                    ItemId::Part(PartId(1)),
                    ItemId::Wire(WireId(0)),
                    ItemId::Label(LabelId(0)),
                ],
                transform: Isometry::Transform {
                    delta: Pt {
                        x: 2 * GRID_NM,
                        y: -GRID_NM,
                    },
                    pivot: Pt::ZERO,
                    by: Orient::FLIP_X_ROT_90,
                },
            };
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 2);
        assert!(scope.gpui_calls.get() > 0);

        // Far-zoom LOD box path for both Place and Drag.
        scope.omnia_calls.set(0);
        scope.gpui_calls.set(0);
        view.update(cx, |v, cx| {
            v.camera = Camera::new(Pt::ZERO, 0.05, viewport);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 2);
        assert!(scope.gpui_calls.get() > 0);

        // Empty drag and unknown symbol are safe no-ops.
        scope.omnia_calls.set(0);
        scope.gpui_calls.set(0);
        view.update(cx, |v, cx| {
            v.preview = OwnedPreview::Drag {
                items: Vec::new(),
                transform: Isometry::Translate(Pt::ZERO),
            };
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 0);
        assert_eq!(scope.gpui_calls.get(), 0);

        view.update(cx, |v, cx| {
            v.preview = OwnedPreview::Place {
                symbol: SymbolId(u16::MAX),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
            };
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 0);
        assert_eq!(scope.gpui_calls.get(), 0);
    }

    /// Counting scope that tracks heap allocations strictly inside `omnia` work while pausing
    /// during `gpui` calls (mirroring B10 / Q5 accounting).
    struct AllocCountingScope {
        counting: Cell<bool>,
        omnia_allocs: Cell<usize>,
        omnia_calls: Cell<u32>,
    }

    impl AllocCountingScope {
        fn new() -> Self {
            Self {
                counting: Cell::new(false),
                omnia_allocs: Cell::new(0),
                omnia_calls: Cell::new(0),
            }
        }
    }

    impl OverlayPaintScope for AllocCountingScope {
        fn omnia(&self, work: &mut dyn FnMut()) {
            self.omnia_calls.set(self.omnia_calls.get() + 1);
            if !self.counting.get() {
                work();
                return;
            }
            let prev = crate::sheet::replace_alloc_counter(Some(0));
            work();
            let used = crate::sheet::replace_alloc_counter(prev).unwrap_or(0);
            self.omnia_allocs.set(self.omnia_allocs.get() + used);
        }

        fn gpui(&self, work: &mut dyn FnMut()) {
            if !self.counting.get() {
                work();
                return;
            }
            let paused = crate::sheet::replace_alloc_counter(None);
            work();
            crate::sheet::replace_alloc_counter(paused);
        }
    }

    /// B10: proves the warm ghost paint layers perform zero heap allocations in `omnia` work for
    /// both `Place` and `Drag` previews at normal zoom and far-zoom LOD.
    #[gpui::test]
    fn t5_3_paint_ghosts_warm_path_makes_zero_omnia_allocations(cx: &mut TestAppContext) {
        let lib = SymbolLibrary::builtin();
        let res = lib.by_name("Resistor-IEEE-Standard").unwrap();
        let masks = Rc::new(Masks::new());
        let doc = Rc::new(sample_doc());
        let alloc_scope = Rc::new(AllocCountingScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));

        struct AllocGhostView {
            doc: Rc<Doc>,
            camera: Camera,
            masks: Rc<Masks>,
            preview: OwnedPreview,
            scope: Rc<AllocCountingScope>,
        }

        impl Render for AllocGhostView {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                struct El {
                    doc: Rc<Doc>,
                    camera: Camera,
                    masks: Rc<Masks>,
                    preview: OwnedPreview,
                    scope: Rc<AllocCountingScope>,
                }
                impl Element for El {
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
                    ) -> (LayoutId, ()) {
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
                        _layout: &mut (),
                        _window: &mut Window,
                        _cx: &mut App,
                    ) {
                    }
                    fn paint(
                        &mut self,
                        _id: Option<&GlobalElementId>,
                        _inspector_id: Option<&InspectorElementId>,
                        bounds: Bounds<Pixels>,
                        _layout: &mut (),
                        _prepaint: &mut (),
                        window: &mut Window,
                        cx: &mut App,
                    ) {
                        let mut camera = self.camera.clone();
                        camera.set_viewport(bounds);
                        let sf = window.scale_factor();
                        let clip = viewport_clip_dev(bounds, sf, 32.0);
                        let params = GhostPaintParams {
                            doc: &self.doc,
                            camera: &camera,
                            bounds,
                            visible_padded: camera.visible(),
                            sf,
                            clip,
                            theme: &GRAPHITE,
                            masks: &self.masks,
                            scope: Some(self.scope.as_ref()),
                        };
                        let preview = match &self.preview {
                            OwnedPreview::None => GhostPreview::None,
                            OwnedPreview::Place { symbol, at, orient } => GhostPreview::Place {
                                symbol: *symbol,
                                at: *at,
                                orient: *orient,
                            },
                            OwnedPreview::Drag { items, transform } => GhostPreview::Drag {
                                items,
                                transform: *transform,
                            },
                            OwnedPreview::Paste { clip, at, orient } => GhostPreview::Paste {
                                clip,
                                at: *at,
                                orient: *orient,
                            },
                            OwnedPreview::Segment { wires, blocker } => GhostPreview::Segment {
                                wires,
                                blocker: *blocker,
                            },
                        };
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            paint_ghosts(preview, &params, window, cx);
                        });
                    }
                }
                div().size_full().child(El {
                    doc: self.doc.clone(),
                    camera: self.camera.clone(),
                    masks: self.masks.clone(),
                    preview: self.preview.clone(),
                    scope: self.scope.clone(),
                })
            }
        }

        let (view, cx) = cx.add_window_view(|_, _| AllocGhostView {
            doc,
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            masks,
            preview: OwnedPreview::Place {
                symbol: res,
                at: Pt::ZERO,
                orient: Orient::ROT_90,
            },
            scope: alloc_scope.clone(),
        });
        cx.run_until_parked();

        // Warm Place path, then count across 5 repaints.
        alloc_scope.omnia_allocs.set(0);
        alloc_scope.omnia_calls.set(0);
        alloc_scope.counting.set(true);
        for step in 0..5 {
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Place {
                    symbol: res,
                    at: Pt {
                        x: step * GRID_NM,
                        y: 0,
                    },
                    orient: Orient::ROT_90,
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        alloc_scope.counting.set(false);
        assert!(alloc_scope.omnia_calls.get() >= 10);
        assert_eq!(
            alloc_scope.omnia_allocs.get(),
            0,
            "B10: warm Place ghost paint must not allocate in omnia code"
        );

        // Warm Drag path (parts, wire, label with Transform), then count across 5 repaints.
        let drag_items = vec![
            ItemId::Part(PartId(0)),
            ItemId::Part(PartId(1)),
            ItemId::Wire(WireId(0)),
            ItemId::Label(LabelId(0)),
        ];
        view.update(cx, |v, cx| {
            v.preview = OwnedPreview::Drag {
                items: drag_items.clone(),
                transform: Isometry::Transform {
                    delta: Pt { x: GRID_NM, y: 0 },
                    pivot: Pt::ZERO,
                    by: Orient::FLIP_X_ROT_90,
                },
            };
            cx.notify();
        });
        cx.run_until_parked();

        alloc_scope.omnia_allocs.set(0);
        alloc_scope.omnia_calls.set(0);
        alloc_scope.counting.set(true);
        for step in 1..=5 {
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Drag {
                    items: drag_items.clone(),
                    transform: Isometry::Transform {
                        delta: Pt {
                            x: step * GRID_NM,
                            y: -GRID_NM,
                        },
                        pivot: Pt::ZERO,
                        by: Orient::FLIP_X_ROT_90,
                    },
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        alloc_scope.counting.set(false);
        assert!(alloc_scope.omnia_calls.get() >= 10);
        assert_eq!(
            alloc_scope.omnia_allocs.get(),
            0,
            "B10: warm Drag ghost paint must not allocate in omnia code"
        );

        // Warm Segment drag preview path (accepted + every Blocker kind), then assert 0 allocs.
        let seg_wires = vec![
            Wire {
                a: Pt {
                    x: 0,
                    y: 2 * GRID_NM,
                },
                b: Pt {
                    x: 6 * GRID_NM,
                    y: 2 * GRID_NM,
                },
            },
            Wire {
                a: Pt {
                    x: 0,
                    y: 2 * GRID_NM,
                },
                b: Pt {
                    x: 0,
                    y: 4 * GRID_NM,
                },
            },
        ];
        view.update(cx, |v, cx| {
            v.preview = OwnedPreview::Segment {
                wires: seg_wires.clone(),
                blocker: None,
            };
            cx.notify();
        });
        cx.run_until_parked();

        alloc_scope.omnia_allocs.set(0);
        alloc_scope.omnia_calls.set(0);
        alloc_scope.counting.set(true);
        for blocker in [
            None,
            Some(Blocker::Pin {
                part: PartId(0),
                pin: 0,
            }),
            Some(Blocker::WireEnd(WireId(0))),
            Some(Blocker::WireBody(WireId(0))),
            Some(Blocker::Overlap(WireId(0))),
            Some(Blocker::Sheet),
        ] {
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Segment {
                    wires: seg_wires.clone(),
                    blocker,
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        alloc_scope.counting.set(false);
        assert!(alloc_scope.omnia_calls.get() >= 12);
        assert_eq!(
            alloc_scope.omnia_allocs.get(),
            0,
            "B10: warm Segment drag preview paint (accepted and refused) must not allocate"
        );

        // Far-zoom LOD box path for both Drag and Place.
        view.update(cx, |v, cx| {
            v.camera = Camera::new(Pt::ZERO, 0.05, viewport);
            cx.notify();
        });
        cx.run_until_parked();

        alloc_scope.omnia_allocs.set(0);
        alloc_scope.omnia_calls.set(0);
        alloc_scope.counting.set(true);
        for step in 1..=3 {
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Drag {
                    items: drag_items.clone(),
                    transform: Isometry::Translate(Pt {
                        x: step * GRID_NM,
                        y: 0,
                    }),
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        for step in 1..=3 {
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Place {
                    symbol: res,
                    at: Pt {
                        x: step * GRID_NM,
                        y: 0,
                    },
                    orient: Orient::IDENTITY,
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        alloc_scope.counting.set(false);
        assert!(alloc_scope.omnia_calls.get() >= 12);
        assert_eq!(
            alloc_scope.omnia_allocs.get(),
            0,
            "B10: far-zoom LOD ghost paint must not allocate in omnia code"
        );
    }

    #[test]
    fn t7_1_paste_ghost_geometry_matches_committed_command_across_orients() {
        let g = GRID_NM;
        let masks = Masks::new();
        let doc = sample_doc();
        let clip = doc.copy(&[
            ItemId::Part(PartId(0)),
            ItemId::Part(PartId(1)),
            ItemId::Wire(WireId(0)),
            ItemId::Label(LabelId(0)),
        ]);
        let paste_at = Pt {
            x: 12 * g,
            y: -8 * g,
        };

        for orient in Orient::ALL {
            let mut probe = doc.fork();
            let change = probe
                .apply(Command::Paste {
                    clip: clip.reoriented(orient),
                    at: paste_at,
                })
                .unwrap();

            let pasted_parts: Vec<PartId> = change
                .added
                .iter()
                .filter_map(|&id| match id {
                    ItemId::Part(p) => Some(p),
                    _ => None,
                })
                .collect();
            let pasted_wires: Vec<WireId> = change
                .added
                .iter()
                .filter_map(|&id| match id {
                    ItemId::Wire(w) => Some(w),
                    _ => None,
                })
                .collect();
            let pasted_labels: Vec<LabelId> = change
                .added
                .iter()
                .filter_map(|&id| match id {
                    ItemId::Label(l) => Some(l),
                    _ => None,
                })
                .collect();
            assert_eq!(pasted_parts.len(), clip.parts().len());
            assert_eq!(pasted_wires.len(), clip.wires().len());
            assert_eq!(pasted_labels.len(), clip.labels().len());

            for (cp, pid) in clip.parts().iter().zip(pasted_parts) {
                let ghost = resolve_paste_part(cp, paste_at, orient, &masks);
                let actual = probe.part(pid).unwrap();
                assert!(ghost.in_bounds);
                assert_eq!(ghost.symbol, actual.symbol);
                assert_eq!(ghost.at, actual.at);
                assert_eq!(ghost.orient, actual.orient);
                assert_eq!(ghost.body, probe.part_body(pid));
            }
            for (&cw, wid) in clip.wires().iter().zip(pasted_wires) {
                let ghost = resolve_paste_wire(cw, paste_at, orient);
                assert!(ghost.in_bounds);
                assert_eq!(ghost.wire, *probe.wire(wid).unwrap());
            }
            for (cl, lid) in clip.labels().iter().zip(pasted_labels) {
                let ghost = resolve_paste_label(cl, paste_at, orient);
                assert!(ghost.in_bounds);
                assert_eq!(ghost.at, probe.label(lid).unwrap().at);
            }
        }

        // K1 out-of-bounds checks: shifting pasted part, pin, bounds, wire, or label past limit sets in_bounds = false.
        let oob_at = Pt {
            x: SHEET_LIMIT_NM,
            y: 0,
        };
        let rotated = clip.reoriented(Orient::ROT_90);
        assert!(clip.parts().iter().any(|cp| {
            let shifted_at = Pt {
                x: SHEET_LIMIT_NM - cp.part.at.x + g,
                y: 0,
            };
            !resolve_paste_part(cp, shifted_at, Orient::IDENTITY, &masks).in_bounds
        }));
        assert!(
            rotated
                .wires()
                .iter()
                .any(|&w| !resolve_paste_wire(w, oob_at, Orient::IDENTITY).in_bounds)
                || clip
                    .wires()
                    .iter()
                    .any(|&w| !resolve_paste_wire(w, oob_at, Orient::IDENTITY).in_bounds)
        );
        let far_label = Label {
            at: Pt { x: 4 * g, y: 0 },
            text: "X".into(),
        };
        assert!(!resolve_paste_label(&far_label, oob_at, Orient::IDENTITY).in_bounds);
    }

    #[gpui::test]
    fn t7_1_paint_paste_ghosts_both_themes_and_lod(cx: &mut TestAppContext) {
        let masks = Rc::new(Masks::new());
        let doc = Rc::new(sample_doc());
        let clip = doc.copy(&[
            ItemId::Part(PartId(0)),
            ItemId::Part(PartId(1)),
            ItemId::Wire(WireId(0)),
            ItemId::Label(LabelId(0)),
        ]);
        let scope = Rc::new(CountingScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));

        let (view, cx) = cx.add_window_view(|_, _| GhostTestView {
            doc,
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            theme: &GRAPHITE,
            masks,
            preview: OwnedPreview::Paste {
                clip: clip.clone(),
                at: Pt {
                    x: 2 * GRID_NM,
                    y: -GRID_NM,
                },
                orient: Orient::IDENTITY,
            },
            scope: scope.clone(),
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 2);
        assert!(scope.gpui_calls.get() > 0);

        // Switch theme to GRAY and reorient.
        scope.omnia_calls.set(0);
        scope.gpui_calls.set(0);
        view.update(cx, |v, cx| {
            v.theme = &GRAY;
            v.preview = OwnedPreview::Paste {
                clip: clip.clone(),
                at: Pt {
                    x: 4 * GRID_NM,
                    y: GRID_NM,
                },
                orient: Orient::FLIP_X_ROT_90,
            };
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 2);
        assert!(scope.gpui_calls.get() > 0);

        // Far-zoom LOD box path.
        scope.omnia_calls.set(0);
        scope.gpui_calls.set(0);
        view.update(cx, |v, cx| {
            v.camera = Camera::new(Pt::ZERO, 0.05, viewport);
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 2);
        assert!(scope.gpui_calls.get() > 0);

        // Empty clip paints nothing.
        scope.omnia_calls.set(0);
        scope.gpui_calls.set(0);
        view.update(cx, |v, cx| {
            v.preview = OwnedPreview::Paste {
                clip: Clip::default(),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
            };
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(scope.omnia_calls.get(), 0);
        assert_eq!(scope.gpui_calls.get(), 0);
    }

    #[gpui::test]
    fn t7_1_paste_ghost_warm_path_makes_zero_omnia_allocations(cx: &mut TestAppContext) {
        let masks = Rc::new(Masks::new());
        let doc = Rc::new(sample_doc());
        let clip = doc.copy(&[
            ItemId::Part(PartId(0)),
            ItemId::Part(PartId(1)),
            ItemId::Wire(WireId(0)),
            ItemId::Label(LabelId(0)),
        ]);
        let alloc_scope = Rc::new(AllocCountingScope::new());
        let viewport = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));

        struct AllocPasteView {
            doc: Rc<Doc>,
            camera: Camera,
            masks: Rc<Masks>,
            preview: OwnedPreview,
            scope: Rc<AllocCountingScope>,
        }

        impl Render for AllocPasteView {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                struct El {
                    doc: Rc<Doc>,
                    camera: Camera,
                    masks: Rc<Masks>,
                    preview: OwnedPreview,
                    scope: Rc<AllocCountingScope>,
                }
                impl Element for El {
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
                    ) -> (LayoutId, ()) {
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
                        _layout: &mut (),
                        _window: &mut Window,
                        _cx: &mut App,
                    ) {
                    }
                    fn paint(
                        &mut self,
                        _id: Option<&GlobalElementId>,
                        _inspector_id: Option<&InspectorElementId>,
                        bounds: Bounds<Pixels>,
                        _layout: &mut (),
                        _prepaint: &mut (),
                        window: &mut Window,
                        cx: &mut App,
                    ) {
                        let mut camera = self.camera.clone();
                        camera.set_viewport(bounds);
                        let sf = window.scale_factor();
                        let clip_dev = viewport_clip_dev(bounds, sf, 32.0);
                        let params = GhostPaintParams {
                            doc: &self.doc,
                            camera: &camera,
                            bounds,
                            visible_padded: camera.visible(),
                            sf,
                            clip: clip_dev,
                            theme: &GRAPHITE,
                            masks: &self.masks,
                            scope: Some(self.scope.as_ref()),
                        };
                        let preview = match &self.preview {
                            OwnedPreview::Paste { clip, at, orient } => GhostPreview::Paste {
                                clip,
                                at: *at,
                                orient: *orient,
                            },
                            _ => GhostPreview::None,
                        };
                        window.with_content_mask(Some(ContentMask { bounds }), |window| {
                            paint_ghosts(preview, &params, window, cx);
                        });
                    }
                }
                div().size_full().child(El {
                    doc: self.doc.clone(),
                    camera: self.camera.clone(),
                    masks: self.masks.clone(),
                    preview: self.preview.clone(),
                    scope: self.scope.clone(),
                })
            }
        }

        let (view, cx) = cx.add_window_view(|_, _| AllocPasteView {
            doc,
            camera: Camera::new(Pt::ZERO, 1.0, viewport),
            masks,
            preview: OwnedPreview::Paste {
                clip: clip.clone(),
                at: Pt::ZERO,
                orient: Orient::IDENTITY,
            },
            scope: alloc_scope.clone(),
        });
        cx.run_until_parked();

        alloc_scope.omnia_allocs.set(0);
        alloc_scope.omnia_calls.set(0);
        alloc_scope.counting.set(true);
        for (step, &orient) in Orient::ALL.iter().enumerate() {
            let offset = i32::try_from(step).unwrap() * GRID_NM;
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Paste {
                    clip: clip.clone(),
                    at: Pt {
                        x: offset,
                        y: -GRID_NM,
                    },
                    orient,
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        alloc_scope.counting.set(false);
        assert!(alloc_scope.omnia_calls.get() >= 16);
        assert_eq!(
            alloc_scope.omnia_allocs.get(),
            0,
            "B10: warm Paste ghost paint must not allocate in omnia code"
        );

        // Far-zoom LOD warm path.
        view.update(cx, |v, cx| {
            v.camera = Camera::new(Pt::ZERO, 0.05, viewport);
            cx.notify();
        });
        cx.run_until_parked();

        alloc_scope.omnia_allocs.set(0);
        alloc_scope.omnia_calls.set(0);
        alloc_scope.counting.set(true);
        for step in 1..=4 {
            view.update(cx, |v, cx| {
                v.preview = OwnedPreview::Paste {
                    clip: clip.clone(),
                    at: Pt {
                        x: step * GRID_NM,
                        y: 0,
                    },
                    orient: Orient::ROT_90,
                };
                cx.notify();
            });
            cx.run_until_parked();
        }
        alloc_scope.counting.set(false);
        assert!(alloc_scope.omnia_calls.get() >= 8);
        assert_eq!(
            alloc_scope.omnia_allocs.get(),
            0,
            "B10: far-zoom LOD Paste ghost paint must not allocate in omnia code"
        );
    }
}
