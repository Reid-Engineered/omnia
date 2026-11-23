//! T4.4: net highlight resolution and overlay wire painting (D7, INV6, INV11).
//!
//! Hovering or selecting any wire resolves its logical [`NetId`] via [`Doc::net_of`] and gathers
//! every wire in that net via [`Doc::net_wires`], including across net-label bridges. Because
//! [`NetId`] values are valid only until the next document edit (see `omnia_doc::connectivity`),
//! [`NetHighlight`] always re-resolves from the current hovered and selected wires on any
//! interaction or document change.
//!
//! All colours come from `omnia-theme`'s `stage/net-highlight` token (INV6). Repainting a
//! highlighted net happens solely inside [`crate::OverlayView`] and never notifies `SheetView`
//! (D7, INV11).

use super::{DevRect, gpui_color, item_world_bounds, wire_rect_dev};
use crate::camera::Camera;
use gpui::{Window, fill};
use omnia_doc::{Doc, ItemId, NetId, Rect, WireId};
use omnia_theme::Theme;

/// Resolved net highlight state derived from the current hovered and selected wires (D7).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetHighlight {
    nets: Vec<NetId>,
    wires: Vec<WireId>,
    prev_wires: Vec<WireId>,
    scratch: Vec<WireId>,
}

impl NetHighlight {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.wires.is_empty()
    }

    /// Canonical sorted, deduplicated [`NetId`]s currently highlighted.
    pub fn nets(&self) -> &[NetId] {
        &self.nets
    }

    /// Canonical sorted, deduplicated [`WireId`]s belonging to the highlighted nets.
    pub fn wires(&self) -> &[WireId] {
        &self.wires
    }

    /// Clears any active net highlight. Returns whether anything changed.
    pub fn clear(&mut self) -> bool {
        if self.nets.is_empty() && self.wires.is_empty() {
            return false;
        }
        self.nets.clear();
        self.wires.clear();
        true
    }

    /// Re-resolves highlighted nets and wires from `hovered` and `selection` against `doc`.
    /// Returns whether the highlighted wire set changed. Allocates nothing once capacities
    /// are warm.
    pub fn sync(&mut self, doc: &Doc, hovered: Option<ItemId>, selection: &[ItemId]) -> bool {
        self.nets.clear();
        if let Some(ItemId::Wire(id)) = hovered
            && let Some(net) = doc.net_of(id)
        {
            self.nets.push(net);
        }
        for &item in selection {
            if let ItemId::Wire(id) = item
                && let Some(net) = doc.net_of(id)
            {
                self.nets.push(net);
            }
        }
        self.nets.sort_unstable();
        self.nets.dedup();

        std::mem::swap(&mut self.wires, &mut self.prev_wires);
        self.wires.clear();
        for &net in &self.nets {
            doc.net_wires(net, &mut self.scratch);
            self.wires.extend_from_slice(&self.scratch);
        }
        self.wires.sort_unstable_by_key(|w| w.0);
        self.wires.dedup();

        self.wires != self.prev_wires
    }
}

/// Paints every visible wire in `wires` in `theme.stage_net_highlight` (INV6).
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_wires(
    doc: &Doc,
    wires: &[WireId],
    camera: &Camera,
    visible_padded: Rect,
    sf: f32,
    width_dev: u32,
    clip: (f64, f64, f64, f64),
    theme: &'static Theme,
    window: &mut Window,
) {
    if wires.is_empty() {
        return;
    }
    let color = gpui_color(theme.stage_net_highlight);
    for &id in wires {
        if !item_world_bounds(doc, ItemId::Wire(id)).is_some_and(|r| r.intersects(visible_padded)) {
            continue;
        }
        if let Some(&wire) = doc.wire(id)
            && let Some(rect) = wire_rect_dev(wire, camera, sf, width_dev, clip)
        {
            let _typed: DevRect = rect;
            window.paint_quad(fill(rect.bounds(sf), color));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnia_doc::{Command, GRID_NM, Label, LabelId, PartId, Pt, Wire};

    fn net_doc() -> Doc {
        let g = GRID_NM;
        // Net A: Wire(0) + Wire(1) physically connected, plus Wire(2) bridged via label "SIG".
        // Net B: Wire(3) + Wire(4) physically connected, no label.
        let wires = [
            Wire {
                a: Pt { x: 0, y: 0 },
                b: Pt { x: 4 * g, y: 0 },
            },
            Wire {
                a: Pt { x: 4 * g, y: 0 },
                b: Pt { x: 4 * g, y: 4 * g },
            },
            Wire {
                a: Pt { x: 10 * g, y: 0 },
                b: Pt { x: 14 * g, y: 0 },
            },
            Wire {
                a: Pt { x: 0, y: 8 * g },
                b: Pt { x: 4 * g, y: 8 * g },
            },
            Wire {
                a: Pt { x: 4 * g, y: 8 * g },
                b: Pt {
                    x: 4 * g,
                    y: 12 * g,
                },
            },
        ];
        let labels = [
            Label {
                at: Pt { x: 0, y: 0 },
                text: "SIG".into(),
            },
            Label {
                at: Pt { x: 10 * g, y: 0 },
                text: "SIG".into(),
            },
        ];
        Doc::from_items([], wires, labels).unwrap()
    }

    #[test]
    fn sync_resolves_hovered_and_selected_wire_nets_deterministically() {
        let doc = net_doc();
        let mut nh = NetHighlight::new();
        assert!(nh.is_empty());

        // Non-wire hover/selection produces no net highlight.
        assert!(!nh.sync(
            &doc,
            Some(ItemId::Label(LabelId(0))),
            &[ItemId::Part(PartId(0))]
        ));
        assert!(nh.is_empty());

        // Hovering Wire(0) highlights Net A across all 3 wires (including label-bridged Wire(2)).
        assert!(nh.sync(&doc, Some(ItemId::Wire(WireId(0))), &[]));
        assert_eq!(nh.nets(), &[doc.net_of(WireId(0)).unwrap()]);
        assert_eq!(nh.wires(), &[WireId(0), WireId(1), WireId(2)]);

        // Idempotent when nothing changes.
        assert!(!nh.sync(&doc, Some(ItemId::Wire(WireId(0))), &[]));

        // Selecting another wire on the same net deduplicates nets and wires.
        assert!(!nh.sync(
            &doc,
            Some(ItemId::Wire(WireId(0))),
            &[ItemId::Wire(WireId(2))]
        ));
        assert_eq!(nh.nets().len(), 1);
        assert_eq!(nh.wires(), &[WireId(0), WireId(1), WireId(2)]);

        // Hovering Net B while Net A is selected highlights both nets sorted by WireId.
        assert!(nh.sync(
            &doc,
            Some(ItemId::Wire(WireId(3))),
            &[ItemId::Wire(WireId(2))]
        ));
        assert_eq!(
            nh.wires(),
            &[WireId(0), WireId(1), WireId(2), WireId(3), WireId(4)]
        );

        // Clearing returns true once, then false.
        assert!(nh.clear());
        assert!(!nh.clear());
        assert!(nh.is_empty());
    }

    #[test]
    fn sync_re_resolves_after_net_merge_and_split() {
        let mut doc = net_doc();
        let mut nh = NetHighlight::new();

        // Select Wire(0) -> [0, 1, 2].
        let sel = [ItemId::Wire(WireId(0))];
        assert!(nh.sync(&doc, None, &sel));
        assert_eq!(nh.wires(), &[WireId(0), WireId(1), WireId(2)]);

        // Merge Net A and Net B with a perpendicular bridge wire from (0, 0) to (0, 8g).
        let change = doc
            .apply(Command::AddWires(vec![Wire {
                a: Pt { x: 0, y: 0 },
                b: Pt {
                    x: 0,
                    y: 8 * GRID_NM,
                },
            }]))
            .unwrap();
        let bridge = change
            .added
            .iter()
            .find_map(|item| match *item {
                ItemId::Wire(id) => Some(id),
                _ => None,
            })
            .unwrap();
        assert!(nh.sync(&doc, None, &sel));
        assert_eq!(
            nh.wires(),
            &[
                WireId(0),
                WireId(1),
                WireId(2),
                WireId(3),
                WireId(4),
                bridge
            ]
        );

        // Undo splits the nets again; re-sync restores [0, 1, 2].
        doc.undo().unwrap();
        assert!(nh.sync(&doc, None, &sel));
        assert_eq!(nh.wires(), &[WireId(0), WireId(1), WireId(2)]);
    }
}
