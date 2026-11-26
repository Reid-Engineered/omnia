//! T7.1: copy/paste clipboard global, anchor-pin grid snapping, and paste ghost state
//! (D2, K2, INV5, INV11, B10, ui-plan.md UD3).
//!
//! [`CanvasClipboard`] stores the latest copied [`Clip`] in a GPUI [`Global`] shared across
//! canvases/tabs in the application. [`PasteGesture`] holds the live paste preview state inside
//! [`crate::tool::ToolState::Pasting`]: pointer moves and R/X/Y reorientations update the ghost
//! preview without touching the document (`INV5`) or notifying `SheetView` (`INV11`), while a
//! left click commits one [`omnia_doc::Command::Paste`] (`INV5`) and Esc cancels cleanly.

use std::sync::Arc;

use gpui::{App, Global};
use omnia_doc::{Clip, GRID_NM, Orient, Pt, SHEET_LIMIT_NM};

/// Largest multiple of [`GRID_NM`] that lies within K1 (`±SHEET_LIMIT_NM`).
const MAX_GRID_NM: i64 = (SHEET_LIMIT_NM / GRID_NM) as i64 * GRID_NM as i64;

/// Process-wide in-app schematic clipboard stored as a GPUI [`Global`] (T7.1).
///
/// Shared across canvases and tabs in one `App`, and isolated per `TestAppContext`.
#[derive(Clone, Debug, Default)]
pub struct CanvasClipboard {
    clip: Option<Arc<Clip>>,
}

impl Global for CanvasClipboard {}

/// Returns `true` when the in-app clipboard holds a non-empty [`Clip`].
pub fn has_clip(cx: &App) -> bool {
    cx.try_global::<CanvasClipboard>()
        .is_some_and(|cb| cb.clip.as_ref().is_some_and(|c| !c.is_empty()))
}

/// Reads the current non-empty [`Clip`] from the in-app clipboard, if any.
pub fn read_clip(cx: &App) -> Option<Arc<Clip>> {
    cx.try_global::<CanvasClipboard>()
        .and_then(|cb| cb.clip.clone())
        .filter(|c| !c.is_empty())
}

/// Stores `clip` in the in-app clipboard.
///
/// An empty clip leaves any existing clipboard content intact and returns `false`.
pub fn write_clip(clip: Clip, cx: &mut App) -> bool {
    if clip.is_empty() {
        return false;
    }
    cx.default_global::<CanvasClipboard>().clip = Some(Arc::new(clip));
    true
}

/// Clears the in-app clipboard.
pub fn clear_clip(cx: &mut App) {
    if cx.has_global::<CanvasClipboard>() {
        cx.global_mut::<CanvasClipboard>().clip = None;
    }
}

/// Applies `orient` to `(dx, dy)` in `i64` without overflow.
#[inline]
pub(crate) fn apply_orient_i64(orient: Orient, dx: i64, dy: i64) -> (i64, i64) {
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

/// Rounds `v` to the nearest multiple of positive `step`, ties away from zero (matching `snap_to`).
#[inline]
fn snap_i64(v: i64, step: i64) -> i64 {
    let half = step / 2;
    if v >= 0 {
        ((v + half) / step) * step
    } else {
        ((v - half) / step) * step
    }
}

/// Snaps `raw_cursor` so the clip's `anchor_pin` (after `orient` about `(0, 0)`) lands on
/// [`GRID_NM`] (K2), or snaps `raw_cursor` directly to [`GRID_NM`] when the clip has no pins.
pub fn snap_paste_origin(anchor_pin: Option<Pt>, raw_cursor: Pt, orient: Orient) -> Pt {
    let step = i64::from(GRID_NM);
    if let Some(p0) = anchor_pin {
        let (ox, oy) = apply_orient_i64(orient, i64::from(p0.x), i64::from(p0.y));
        let pin_raw_x = (i64::from(raw_cursor.x) + ox).clamp(-MAX_GRID_NM, MAX_GRID_NM);
        let pin_raw_y = (i64::from(raw_cursor.y) + oy).clamp(-MAX_GRID_NM, MAX_GRID_NM);
        let pin_x = snap_i64(pin_raw_x, step);
        let pin_y = snap_i64(pin_raw_y, step);
        Pt {
            x: (pin_x - ox).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
            y: (pin_y - oy).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        }
    } else {
        Pt {
            x: snap_i64(i64::from(raw_cursor.x), step).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
            y: snap_i64(i64::from(raw_cursor.y), step).clamp(-MAX_GRID_NM, MAX_GRID_NM) as i32,
        }
    }
}

/// Returns `true` when pasting `clip` reoriented by `orient` at `at` keeps every part origin,
/// world pin, local bounds, wire endpoint, and label anchor inside K1 (`±SHEET_LIMIT_NM`).
///
/// Allocates nothing.
pub fn clip_in_bounds_at(clip: &Clip, at: Pt, orient: Orient) -> bool {
    let limit = i64::from(SHEET_LIMIT_NM);
    let in_limit = |x: i64, y: i64| (-limit..=limit).contains(&x) && (-limit..=limit).contains(&y);

    for cp in clip.parts() {
        if !cp.local_bounds.is_in_bounds() {
            return false;
        }
        let (rx, ry) = apply_orient_i64(orient, i64::from(cp.part.at.x), i64::from(cp.part.at.y));
        let part_x = i64::from(at.x) + rx;
        let part_y = i64::from(at.y) + ry;
        if !in_limit(part_x, part_y) {
            return false;
        }
        let world_orient = cp.part.orient.then(orient);
        let min_off = world_orient.apply(cp.local_bounds.min);
        let max_off = world_orient.apply(cp.local_bounds.max);
        if !in_limit(part_x + i64::from(min_off.x), part_y + i64::from(min_off.y))
            || !in_limit(part_x + i64::from(max_off.x), part_y + i64::from(max_off.y))
        {
            return false;
        }
        for &local_pin in &cp.local_pins {
            if !local_pin.is_in_bounds() {
                return false;
            }
            let off = world_orient.apply(local_pin);
            if !in_limit(part_x + i64::from(off.x), part_y + i64::from(off.y)) {
                return false;
            }
        }
    }

    for w in clip.wires() {
        let (ax, ay) = apply_orient_i64(orient, i64::from(w.a.x), i64::from(w.a.y));
        let (bx, by) = apply_orient_i64(orient, i64::from(w.b.x), i64::from(w.b.y));
        if !in_limit(i64::from(at.x) + ax, i64::from(at.y) + ay)
            || !in_limit(i64::from(at.x) + bx, i64::from(at.y) + by)
        {
            return false;
        }
    }

    for l in clip.labels() {
        let (lx, ly) = apply_orient_i64(orient, i64::from(l.at.x), i64::from(l.at.y));
        if !in_limit(i64::from(at.x) + lx, i64::from(at.y) + ly) {
            return false;
        }
    }

    true
}

/// Interactive paste-placement ghost state inside [`crate::tool::ToolState::Pasting`] (T7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasteGesture {
    pub clip: Arc<Clip>,
    /// Snapped paste origin under the pointer, once the pointer position over the sheet is known.
    pub at: Option<Pt>,
    /// R, X and Y presses composed in order while placing the paste preview.
    pub orient: Orient,
    /// A left press was made while pasting and not yet released: only its release commits.
    pub pressed: bool,
    /// Cached clip-local anchor pin (pin 0 of the first pinned part), avoiding scans on move.
    anchor_pin: Option<Pt>,
    /// Last raw world cursor position, kept so R/X/Y re-snaps `at` immediately.
    raw_cursor: Option<Pt>,
}

impl PasteGesture {
    /// Creates a paste gesture for `clip`, snapping at `raw_cursor` when available.
    pub fn new(clip: Arc<Clip>, raw_cursor: Option<Pt>) -> Self {
        let anchor_pin = clip.anchor_pin();
        let at = raw_cursor.map(|raw| snap_paste_origin(anchor_pin, raw, Orient::IDENTITY));
        Self {
            clip,
            at,
            orient: Orient::IDENTITY,
            pressed: false,
            anchor_pin,
            raw_cursor,
        }
    }

    /// Clip-local position of the anchor pin used for [`GRID_NM`] snapping, if any.
    pub fn anchor_pin(&self) -> Option<Pt> {
        self.anchor_pin
    }

    /// Returns `true` when pasting at `at` with the current orientation fits inside K1.
    pub fn in_bounds_at(&self, at: Pt) -> bool {
        clip_in_bounds_at(&self.clip, at, self.orient)
    }

    /// Returns `true` when `at` is resolved and pasting there fits inside K1.
    pub fn is_in_bounds(&self) -> bool {
        self.at.is_some_and(|at| self.in_bounds_at(at))
    }

    pub(crate) fn update_cursor(&mut self, raw: Pt) -> bool {
        self.raw_cursor = Some(raw);
        let next = Some(snap_paste_origin(self.anchor_pin, raw, self.orient));
        let changed = self.at != next;
        self.at = next;
        changed
    }

    pub(crate) fn reorient(&mut self, by: Orient) {
        self.orient = self.orient.then(by);
        if let Some(raw) = self.raw_cursor {
            self.at = Some(snap_paste_origin(self.anchor_pin, raw, self.orient));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{Tool, ToolMachine, ToolPhase, ToolRequest};
    use omnia_doc::{Command, Doc, Label, PIN_LATTICE_NM, Part, Wire};
    use omnia_symbols::SymbolLibrary;

    const G: i32 = GRID_NM;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * G, y: y * G }
    }

    fn opamp_clip() -> Clip {
        let lib = SymbolLibrary::builtin();
        let opamp = lib.by_name("IC-COM-OpAmp").unwrap();
        let def = lib.get(opamp);
        let mut doc = Doc::new();
        let id = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: opamp,
                    at: Pt {
                        x: PIN_LATTICE_NM,
                        y: PIN_LATTICE_NM,
                    },
                    orient: Orient::IDENTITY,
                    designator: "U1".into(),
                    value: "".into(),
                },
                local_pins: def.pins.iter().map(|p| p.at).collect(),
                local_bounds: def.bounds,
            })
            .unwrap()
            .added[0];
        doc.copy(&[id])
    }

    fn wire_clip() -> Clip {
        let mut doc = Doc::new();
        let w = doc
            .apply(Command::AddWires(vec![Wire {
                a: pt(0, 0),
                b: pt(4, 0),
            }]))
            .unwrap()
            .added[0];
        let l = doc
            .apply(Command::AddLabel(Label {
                at: pt(2, 0),
                text: "SIG".into(),
            }))
            .unwrap()
            .added[0];
        doc.copy(&[w, l])
    }

    #[test]
    fn t7_1_snap_paste_origin_keeps_anchor_pin_on_grid_across_all_orients() {
        let clip = opamp_clip();
        let anchor_pin = clip.anchor_pin();
        assert!(anchor_pin.is_some());
        let raw = Pt {
            x: 5 * G + 400_000,
            y: -3 * G - 300_000,
        };

        for orient in Orient::ALL {
            let at = snap_paste_origin(anchor_pin, raw, orient);
            let rotated = clip.reoriented(orient);
            let cp = &rotated.parts()[0];
            let world_origin = at + cp.part.at;
            let world_pin0 = world_origin + cp.part.orient.apply(cp.local_pins[0]);
            assert_eq!(world_pin0.x % G, 0, "orient {orient:?} pin0 x off grid");
            assert_eq!(world_pin0.y % G, 0, "orient {orient:?} pin0 y off grid");
            assert!(clip_in_bounds_at(&clip, at, orient));
        }

        // Edge clamp never panics and keeps single-grid wires in K1.
        let wc = wire_clip();
        let edge_at = snap_paste_origin(
            wc.anchor_pin(),
            Pt {
                x: SHEET_LIMIT_NM,
                y: -SHEET_LIMIT_NM,
            },
            Orient::IDENTITY,
        );
        assert_eq!(edge_at.x % G, 0);
        assert_eq!(edge_at.y % G, 0);
    }

    #[test]
    fn t7_1_pure_tool_machine_paste_lifecycle() {
        let clip = Arc::new(opamp_clip());
        let mut m = ToolMachine::new();
        assert_eq!(m.phase(), ToolPhase::Idle);

        // Empty clip is ignored.
        let empty = Arc::new(Clip::default());
        assert!(!m.start_paste(empty, Some(pt(0, 0))).handled);
        assert_eq!(m.phase(), ToolPhase::Idle);

        // Start paste with cursor over sheet -> Pasting with snapped origin.
        let out = m.start_paste(Arc::clone(&clip), Some(pt(2, 3)));
        assert!(out.handled && out.preview_changed);
        assert_eq!(m.phase(), ToolPhase::Pasting);
        assert!(m.routes_pointer());
        assert!(!m.holds_capture());

        // Move and reorient updates preview without committing.
        assert!(m.pointer_move(pt(6, -2), false).preview_changed);
        assert!(m.reorient(Orient::ROT_90).preview_changed);
        assert_eq!(m.delete(), crate::tool::ToolOutcome::default());

        // Press and release commits one ToolRequest::Paste and returns to Select/Idle.
        let doc = Doc::new();
        assert!(
            m.pointer_down(&doc, 1, pt(6, -2), G / 4, &[], false)
                .handled
        );
        let up = m.pointer_up(pt(6, -2), false);
        assert_eq!(
            up.request,
            Some(ToolRequest::Paste {
                clip: Arc::clone(&clip),
                at: snap_paste_origin(clip.anchor_pin(), pt(6, -2), Orient::ROT_90),
                orient: Orient::ROT_90,
            })
        );
        assert_eq!((m.tool(), m.phase()), (Tool::Select, ToolPhase::Idle));

        // Out-of-bounds click does not commit and keeps Pasting active.
        let far_wire = Arc::new({
            let mut d = Doc::new();
            let id = d
                .apply(Command::AddWires(vec![Wire {
                    a: pt(0, 0),
                    b: pt(10, 0),
                }]))
                .unwrap()
                .added[0];
            d.copy(&[id])
        });
        m.start_paste(far_wire, None);
        let oob = Pt {
            x: SHEET_LIMIT_NM - G,
            y: 0,
        };
        m.pointer_down(&doc, 1, oob, G / 4, &[], false);
        let oob_up = m.pointer_up(oob, false);
        assert!(oob_up.request.is_none());
        assert_eq!(m.phase(), ToolPhase::Pasting);

        // Esc cancels back to Select/Idle.
        assert!(m.cancel().handled);
        assert_eq!((m.tool(), m.phase()), (Tool::Select, ToolPhase::Idle));

        // pointer_lost clears pending press while staying in Pasting.
        m.start_paste(Arc::clone(&clip), Some(pt(0, 0)));
        m.pointer_down(&doc, 1, pt(0, 0), G / 4, &[], false);
        assert!(m.pointer_lost().handled);
        assert_eq!(m.phase(), ToolPhase::Pasting);
        assert!(m.pointer_up(pt(0, 0), false).request.is_none());

        // set_tool or document_replaced cancels Pasting cleanly.
        assert!(m.document_replaced().handled);
        assert_eq!(m.phase(), ToolPhase::Idle);
    }

    #[gpui::test]
    fn t7_1_canvas_clipboard_global_operations(cx: &mut gpui::TestAppContext) {
        let clip = opamp_clip();
        cx.update(|cx| {
            assert!(!has_clip(cx));
            assert!(read_clip(cx).is_none());
            assert!(!write_clip(Clip::default(), cx));
            assert!(!has_clip(cx));

            assert!(write_clip(clip.clone(), cx));
            assert!(has_clip(cx));
            assert_eq!(read_clip(cx).as_deref(), Some(&clip));

            // Empty write preserves existing clipboard.
            assert!(!write_clip(Clip::default(), cx));
            assert_eq!(read_clip(cx).as_deref(), Some(&clip));

            clear_clip(cx);
            assert!(!has_clip(cx));
            assert!(read_clip(cx).is_none());
        });
    }
}
