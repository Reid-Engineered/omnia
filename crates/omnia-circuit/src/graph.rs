//! Derived circuit graph: dense SoA rows, CSR net membership, keys, names, roles and ground
//! evidence (CD1, CD7, CK1–CK4, CK6–CK7, CD18, section 11.2).

use std::{cmp::Ordering, ops::Range, sync::Arc};

use omnia_doc::{Doc, LabelId, NetId, PartId, Pt, Rect, SymbolId, WireId};
use omnia_symbols::SymbolLibrary;
use smallvec::SmallVec;

use crate::{
    intern::{Interner, LookupMap, StrId},
    keys::{
        CircuitKey, LabelSortKey, PinSortKey, WireSortKey, format_pin_net_name,
        format_wire_net_name, is_ground_name_hint,
    },
};

/// CK7: a signal net with strictly more than 32 pins is a hub.
pub const HUB_PIN_THRESHOLD: usize = 32;

/// Dense row index into [`Circuit`]'s part columns (CD7, section 11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PartRow(pub(crate) u32);

impl PartRow {
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Dense row index into [`Circuit`]'s pin columns (CD7, section 11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PinRow(pub(crate) u32);

impl PinRow {
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Dense row index into [`Circuit`]'s net columns, assigned in CD18 net order (CD7, CD18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NetRow(pub(crate) u32);

impl NetRow {
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Reference to a net row paired with the revision at which it was resolved (section 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NetRef {
    pub(crate) row: NetRow,
    pub(crate) revision: u64,
}

impl NetRef {
    #[inline]
    pub const fn row(self) -> NetRow {
        self.row
    }

    #[inline]
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

/// Per-pin electrical role (CK3, PD5). Every pin is [`PinRole::Unspecified`] until C3.1 reads
/// embedded part definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinRole {
    Passive,
    Input,
    Output,
    Bidir,
    Power,
    Ground,
    NoConnect,
    Unspecified,
}

/// Per-symbol class, kept separate from [`PinRole`] (CK3, XD5).
///
/// Until sim S1.1 attaches symbol classes directly to `omnia-symbols`, `Ground-COM-General` maps
/// to [`SymbolClass::Ground`] and every other symbol maps to [`SymbolClass::Component`] (CK4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolClass {
    Component,
    Ground,
    PowerRail,
    Instrument,
    Probe,
}

/// Logical net kind (CK4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetKind {
    Signal,
    Power,
    Ground,
}

/// Evidence tier for derived facts (CK6). No numeric confidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EvidenceTier {
    Declared,
    Exact,
    Derived,
    Heuristic,
    Suggested,
}

/// Declared ground-reference evidence item on a net (CK4, XD4, section 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroundEvidence {
    GroundSymbol(PartId),
    ZeroLabel(LabelId),
}

/// Compact bitflags stored per net row (CK4, CK7, section 11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NetFlags(u8);

impl NetFlags {
    pub const EMPTY: Self = Self(0);
    const GROUND_REFERENCE: u8 = 1 << 0;
    const HUB: u8 = 1 << 1;

    #[inline]
    pub const fn new(is_ground_reference: bool, is_hub: bool) -> Self {
        let mut bits = 0u8;
        if is_ground_reference {
            bits |= Self::GROUND_REFERENCE;
        }
        if is_hub {
            bits |= Self::HUB;
        }
        Self(bits)
    }

    /// `true` iff the net carries declared ground evidence (CK4).
    #[inline]
    pub const fn is_ground_reference(self) -> bool {
        (self.0 & Self::GROUND_REFERENCE) != 0
    }

    /// `true` iff the net is a hub under CK7 (`power`, `ground`, or strictly more than 32 pins).
    #[inline]
    pub const fn is_hub(self) -> bool {
        (self.0 & Self::HUB) != 0
    }
}

/// Contiguous slice range inside a flat CSR array (CD7). `cap` reserves room for C1.3's
/// in-place growth and free-list management.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CsrRange {
    pub(crate) start: u32,
    pub(crate) len: u32,
    pub(crate) cap: u32,
}

impl CsrRange {
    #[inline]
    pub(crate) const fn packed(start: u32, len: u32) -> Self {
        Self {
            start,
            len,
            cap: len,
        }
    }

    #[inline]
    pub(crate) const fn as_usize_range(self) -> Range<usize> {
        let start = self.start as usize;
        start..(start + self.len as usize)
    }
}

const ROW_NONE: u32 = u32::MAX;

/// Derived, read-only circuit model for one [`Doc`] revision (CD1, CD7, section 11.2).
#[derive(Debug, Clone)]
pub struct Circuit {
    revision: u64,
    interner: Interner,

    // ---- Part columns (SoA) ----
    part_id_to_row: Vec<u32>,
    part_ids: Vec<PartId>,
    part_designators: Vec<StrId>,
    part_values: Vec<StrId>,
    part_symbols: Vec<SymbolId>,
    part_symbol_classes: Vec<SymbolClass>,
    part_pin_ranges: Vec<CsrRange>,

    // ---- Pin columns (SoA, contiguous per part in stored pin index order) ----
    pin_part_rows: Vec<PartRow>,
    pin_indices: Vec<u32>,
    pin_numbers: Vec<Option<StrId>>,
    pin_roles: Vec<PinRole>,
    pin_nets: Vec<NetRow>,
    pin_positions: Vec<Pt>,

    // ---- Net columns (SoA, ordered in CD18 net order) ----
    net_id_to_row: LookupMap<NetId, NetRow>,
    net_ids: Vec<NetId>,
    net_keys: Vec<CircuitKey>,
    net_display_names: Vec<StrId>,
    net_kinds: Vec<NetKind>,
    net_kind_evidence: Vec<EvidenceTier>,
    net_flags: Vec<NetFlags>,
    net_changed_at: Vec<u64>,

    // ---- CSR membership & metadata arrays ----
    net_pin_ranges: Vec<CsrRange>,
    csr_pin_rows: Vec<PinRow>,
    csr_pin_refs: Vec<(PartId, u32)>,

    net_label_ranges: Vec<CsrRange>,
    csr_labels: Vec<LabelId>,

    net_candidate_ranges: Vec<CsrRange>,
    csr_candidates: Vec<StrId>,

    net_ground_ranges: Vec<CsrRange>,
    csr_ground: Vec<GroundEvidence>,

    // ---- Point-query indexes (non-iterable LookupMap: CINV10) ----
    key_to_net: LookupMap<CircuitKey, NetRow>,
    name_to_nets: LookupMap<StrId, SmallVec<[NetRow; 1]>>,
}

struct TempNet {
    doc_net_id: NetId,
    pins: SmallVec<[PinRow; 4]>,
    labels: SmallVec<[LabelId; 2]>,
    anchor_wire: Option<(WireId, WireSortKey)>,
}

impl TempNet {
    fn new(doc_net_id: NetId) -> Self {
        Self {
            doc_net_id,
            pins: SmallVec::new(),
            labels: SmallVec::new(),
            anchor_wire: None,
        }
    }
}

impl Circuit {
    /// Derives a complete [`Circuit`] graph from `doc` and `symbols` through `Doc`'s public
    /// read-only API (CD1, CD7, CK1–CK4, CD18).
    pub fn build(doc: &Doc, symbols: &SymbolLibrary) -> Self {
        let revision = doc.revision();
        let estimated_nets = doc.net_count();

        let mut interner = Interner::with_capacity(estimated_nets * 2 + 16, estimated_nets * 16);

        let mut part_id_to_row: Vec<u32> = Vec::new();
        let mut part_ids: Vec<PartId> = Vec::new();
        let mut part_designators: Vec<StrId> = Vec::new();
        let mut part_values: Vec<StrId> = Vec::new();
        let mut part_symbols: Vec<SymbolId> = Vec::new();
        let mut part_symbol_classes: Vec<SymbolClass> = Vec::new();
        let mut part_pin_ranges: Vec<CsrRange> = Vec::new();
        let mut part_bodies: Vec<Rect> = Vec::new();

        let mut pin_part_rows: Vec<PartRow> = Vec::new();
        let mut pin_indices: Vec<u32> = Vec::new();
        let mut pin_numbers: Vec<Option<StrId>> = Vec::new();
        let mut pin_roles: Vec<PinRole> = Vec::new();
        let mut pin_positions: Vec<Pt> = Vec::new();

        // Discover nets into a Vec indexed through a non-iterable LookupMap (CINV10).
        let mut temp_net_map: LookupMap<NetId, usize> = LookupMap::with_capacity(estimated_nets);
        let mut temp_nets: Vec<TempNet> = Vec::with_capacity(estimated_nets);

        let get_or_add_temp_net =
            |net_id: NetId, map: &mut LookupMap<NetId, usize>, nets: &mut Vec<TempNet>| -> usize {
                *map.get_or_insert_with(net_id, || {
                    let idx = nets.len();
                    nets.push(TempNet::new(net_id));
                    idx
                })
            };

        // 1. Collect parts and their pins (Doc::parts yields ascending PartId order).
        for (part_id, part) in doc.parts() {
            let part_row = PartRow(u32::try_from(part_ids.len()).expect("part row overflow"));
            let id_idx = part_id.0 as usize;
            if id_idx >= part_id_to_row.len() {
                part_id_to_row.resize(id_idx + 1, ROW_NONE);
            }
            part_id_to_row[id_idx] = part_row.0;

            let des_id = interner.intern(&part.designator);
            let val_id = interner.intern(&part.value);
            let sym_class = classify_symbol(symbols, part.symbol);
            let body = doc.part_body(part_id).unwrap_or(Rect {
                min: part.at,
                max: part.at,
            });

            let sym_def = (part.symbol.index() < symbols.len()).then(|| symbols.get(part.symbol));
            let world_pins = doc.pins(part_id).unwrap_or(&[]);
            let pin_start = u32::try_from(pin_part_rows.len()).expect("pin row overflow");
            let pin_len = u32::try_from(world_pins.len()).expect("part pin count overflow");

            part_ids.push(part_id);
            part_designators.push(des_id);
            part_values.push(val_id);
            part_symbols.push(part.symbol);
            part_symbol_classes.push(sym_class);
            part_pin_ranges.push(CsrRange::packed(pin_start, pin_len));
            part_bodies.push(body);

            for (idx, &at) in world_pins.iter().enumerate() {
                let pin_idx = u32::try_from(idx).expect("pin index overflow");
                let pin_row = PinRow(u32::try_from(pin_part_rows.len()).expect("pin row overflow"));
                let pin_num_id = sym_def
                    .and_then(|def| def.pins.get(idx))
                    .map(|p| interner.intern(p.number));

                pin_part_rows.push(part_row);
                pin_indices.push(pin_idx);
                pin_numbers.push(pin_num_id);
                // CK3 / C1.2: roles are `Unspecified` until C3.1 reads embedded definitions.
                pin_roles.push(PinRole::Unspecified);
                pin_positions.push(at);

                let net_id = doc
                    .net_at(at)
                    .expect("every live pin belongs to a net in connectivity");
                let temp_idx = get_or_add_temp_net(net_id, &mut temp_net_map, &mut temp_nets);
                temp_nets[temp_idx].pins.push(pin_row);
            }
        }

        // 2. Collect wires: only the CK1 anchor wire is needed per net.
        for (wire_id, &wire) in doc.wires() {
            let net_id = doc
                .net_of(wire_id)
                .expect("every live wire belongs to a net in connectivity");
            let temp_idx = get_or_add_temp_net(net_id, &mut temp_net_map, &mut temp_nets);
            let candidate = WireSortKey::from_wire(wire_id, wire);
            let slot = &mut temp_nets[temp_idx].anchor_wire;
            if slot.is_none_or(|(_, best)| candidate < best) {
                *slot = Some((wire_id, candidate));
            }
        }

        // 3. Collect labels.
        for (label_id, label) in doc.labels() {
            let net_id = doc
                .net_at(label.at)
                .expect("every live label belongs to a net in connectivity");
            let temp_idx = get_or_add_temp_net(net_id, &mut temp_net_map, &mut temp_nets);
            temp_nets[temp_idx].labels.push(label_id);
        }

        let make_pin_sort_key = |pin_row: PinRow| -> PinSortKey<'_> {
            let p_idx = pin_row.index();
            let part_row = pin_part_rows[p_idx];
            let pr = part_row.index();
            let part_id = part_ids[pr];
            let part = doc.part(part_id).expect("live part exists");
            PinSortKey {
                designator: interner.resolve(part_designators[pr]),
                pin_number: pin_numbers[p_idx].map(|s| interner.resolve(s)),
                at: pin_positions[p_idx],
                symbol: part_symbols[pr],
                value: interner.resolve(part_values[pr]),
                orient: part.orient,
                bounds: part_bodies[pr],
                part: part_id,
                pin_index: pin_indices[p_idx],
            }
        };

        let make_label_sort_key = |label_id: LabelId| -> LabelSortKey<'_> {
            let label = doc.label(label_id).expect("live label exists");
            LabelSortKey {
                text: &label.text,
                at: label.at,
                id: label_id,
            }
        };

        // 4. Sort each net's pins and labels in CD18 order.
        for net in &mut temp_nets {
            net.pins
                .sort_unstable_by(|&a, &b| make_pin_sort_key(a).cmp_cd18(&make_pin_sort_key(b)));
            net.labels.sort_unstable_by(|&a, &b| {
                make_label_sort_key(a).cmp_cd18(&make_label_sort_key(b))
            });
        }

        // 5. Sort nets in CD18 net order: pin nets -> label-only nets -> wire-only islands.
        let mut net_order: Vec<usize> = (0..temp_nets.len()).collect();
        net_order.sort_unstable_by(|&ia, &ib| {
            let na = &temp_nets[ia];
            let nb = &temp_nets[ib];
            cmp_temp_nets(na, nb, &make_pin_sort_key, &make_label_sort_key)
        });

        // 6. Populate final net columns, CSR arrays, pin_nets, keys, display names, kinds & indexes.
        let net_count = net_order.len();
        let total_pins = pin_part_rows.len();

        let mut pin_nets: Vec<NetRow> = vec![NetRow(0); total_pins];
        let mut net_id_to_row: LookupMap<NetId, NetRow> = LookupMap::with_capacity(net_count);
        let mut net_ids: Vec<NetId> = Vec::with_capacity(net_count);
        let mut net_keys: Vec<CircuitKey> = Vec::with_capacity(net_count);
        let mut net_display_names: Vec<StrId> = Vec::with_capacity(net_count);
        let mut net_kinds: Vec<NetKind> = Vec::with_capacity(net_count);
        let mut net_kind_evidence: Vec<EvidenceTier> = Vec::with_capacity(net_count);
        let mut net_flags: Vec<NetFlags> = Vec::with_capacity(net_count);
        let mut net_changed_at: Vec<u64> = Vec::with_capacity(net_count);

        let mut net_pin_ranges: Vec<CsrRange> = Vec::with_capacity(net_count);
        let mut csr_pin_rows: Vec<PinRow> = Vec::with_capacity(total_pins);
        let mut csr_pin_refs: Vec<(PartId, u32)> = Vec::with_capacity(total_pins);

        let mut net_label_ranges: Vec<CsrRange> = Vec::with_capacity(net_count);
        let mut csr_labels: Vec<LabelId> = Vec::new();

        let mut net_candidate_ranges: Vec<CsrRange> = Vec::with_capacity(net_count);
        let mut csr_candidates: Vec<StrId> = Vec::new();

        let mut net_ground_ranges: Vec<CsrRange> = Vec::with_capacity(net_count);
        let mut csr_ground: Vec<GroundEvidence> = Vec::new();

        let mut key_to_net: LookupMap<CircuitKey, NetRow> = LookupMap::with_capacity(net_count);
        let mut name_to_nets: LookupMap<StrId, SmallVec<[NetRow; 1]>> =
            LookupMap::with_capacity(net_count);

        let mut scratch_name = String::with_capacity(64);
        let mut seen_ground_parts: SmallVec<[PartId; 2]> = SmallVec::new();

        for (row_idx, &temp_idx) in net_order.iter().enumerate() {
            let net_row = NetRow(u32::try_from(row_idx).expect("net row overflow"));
            let temp = &temp_nets[temp_idx];

            // ---- Pin CSR ----
            let pin_csr_start = u32::try_from(csr_pin_rows.len()).expect("csr pin overflow");
            let mut has_ground_role_pin = false;
            let mut has_power_role_or_rail = false;
            seen_ground_parts.clear();
            let ground_start = u32::try_from(csr_ground.len()).expect("csr ground overflow");

            for &pin_row in &temp.pins {
                let p_idx = pin_row.index();
                pin_nets[p_idx] = net_row;
                let part_row = pin_part_rows[p_idx];
                let part_id = part_ids[part_row.index()];
                let pin_idx = pin_indices[p_idx];
                csr_pin_rows.push(pin_row);
                csr_pin_refs.push((part_id, pin_idx));

                match part_symbol_classes[part_row.index()] {
                    SymbolClass::Ground => {
                        if !seen_ground_parts.contains(&part_id) {
                            seen_ground_parts.push(part_id);
                            csr_ground.push(GroundEvidence::GroundSymbol(part_id));
                        }
                    }
                    SymbolClass::PowerRail => {
                        has_power_role_or_rail = true;
                    }
                    SymbolClass::Component | SymbolClass::Instrument | SymbolClass::Probe => {}
                }
                match pin_roles[p_idx] {
                    PinRole::Ground => has_ground_role_pin = true,
                    PinRole::Power => has_power_role_or_rail = true,
                    _ => {}
                }
            }
            let pin_csr_len = u32::try_from(temp.pins.len()).expect("net pin count overflow");
            net_pin_ranges.push(CsrRange::packed(pin_csr_start, pin_csr_len));

            // ---- Label CSR & candidate names ----
            let label_csr_start = u32::try_from(csr_labels.len()).expect("csr label overflow");
            let cand_csr_start =
                u32::try_from(csr_candidates.len()).expect("csr candidate overflow");
            let mut has_ground_name_hint = false;

            // `temp.labels` is already sorted by (text bytes, at, id), so distinct texts appear
            // in ascending byte order; the first is the key & display name (CK1, CK2).
            let mut first_label_text: Option<&Arc<str>> = None;
            let mut last_seen_text: Option<&str> = None;

            for &label_id in &temp.labels {
                csr_labels.push(label_id);
                let label = doc.label(label_id).expect("live label exists");
                if label.text.as_ref() == "0" {
                    csr_ground.push(GroundEvidence::ZeroLabel(label_id));
                }
                if is_ground_name_hint(&label.text) {
                    has_ground_name_hint = true;
                }
                if first_label_text.is_none() {
                    first_label_text = Some(&label.text);
                    last_seen_text = Some(&label.text);
                } else if last_seen_text != Some(label.text.as_ref()) {
                    let cand_str_id = interner.intern(&label.text);
                    csr_candidates.push(cand_str_id);
                    push_unique_net_row(name_to_nets.get_or_default(cand_str_id), net_row);
                    last_seen_text = Some(&label.text);
                }
            }
            let label_csr_len = u32::try_from(temp.labels.len()).expect("net label count overflow");
            net_label_ranges.push(CsrRange::packed(label_csr_start, label_csr_len));

            let cand_csr_len = u32::try_from(csr_candidates.len()).expect("csr candidate overflow")
                - cand_csr_start;
            net_candidate_ranges.push(CsrRange::packed(cand_csr_start, cand_csr_len));

            let ground_len =
                u32::try_from(csr_ground.len()).expect("csr ground overflow") - ground_start;
            net_ground_ranges.push(CsrRange::packed(ground_start, ground_len));

            // ---- CK1 key and CK2 display name ----
            let (key, display_name_id) = if let Some(text) = first_label_text {
                let key = CircuitKey::Label(Arc::clone(text));
                let name_id = interner.intern(text);
                (key, name_id)
            } else if let Some(&anchor_pin_row) = temp.pins.first() {
                let p_idx = anchor_pin_row.index();
                let part_row = pin_part_rows[p_idx];
                let part_id = part_ids[part_row.index()];
                let pin_idx = pin_indices[p_idx];
                let des = interner.resolve(part_designators[part_row.index()]);
                let pin_num = pin_numbers[p_idx].map(|s| interner.resolve(s));
                format_pin_net_name(des, pin_num, pin_idx, &mut scratch_name);
                let name_id = interner.intern(&scratch_name);
                (
                    CircuitKey::Pin {
                        part: part_id,
                        pin: pin_idx,
                    },
                    name_id,
                )
            } else {
                let (wire_id, _) = temp
                    .anchor_wire
                    .expect("a net with no pins and no labels has at least one wire");
                format_wire_net_name(wire_id, &mut scratch_name);
                let name_id = interner.intern(&scratch_name);
                (CircuitKey::Wire(wire_id), name_id)
            };

            push_unique_net_row(name_to_nets.get_or_default(display_name_id), net_row);
            key_to_net.insert(key.clone(), net_row);
            net_id_to_row.insert(temp.doc_net_id, net_row);

            // ---- CK4 kind, evidence & CK7 flags ----
            let is_ground_reference = ground_len > 0;
            let (kind, evidence) = if is_ground_reference || has_ground_role_pin {
                (NetKind::Ground, EvidenceTier::Declared)
            } else if has_power_role_or_rail {
                (NetKind::Power, EvidenceTier::Declared)
            } else if has_ground_name_hint {
                (NetKind::Ground, EvidenceTier::Heuristic)
            } else {
                (NetKind::Signal, EvidenceTier::Declared)
            };
            let is_hub = matches!(kind, NetKind::Power | NetKind::Ground)
                || (pin_csr_len as usize) > HUB_PIN_THRESHOLD;

            net_ids.push(temp.doc_net_id);
            net_keys.push(key);
            net_display_names.push(display_name_id);
            net_kinds.push(kind);
            net_kind_evidence.push(evidence);
            net_flags.push(NetFlags::new(is_ground_reference, is_hub));
            net_changed_at.push(revision);
        }

        Self {
            revision,
            interner,
            part_id_to_row,
            part_ids,
            part_designators,
            part_values,
            part_symbols,
            part_symbol_classes,
            part_pin_ranges,
            pin_part_rows,
            pin_indices,
            pin_numbers,
            pin_roles,
            pin_nets,
            pin_positions,
            net_id_to_row,
            net_ids,
            net_keys,
            net_display_names,
            net_kinds,
            net_kind_evidence,
            net_flags,
            net_changed_at,
            net_pin_ranges,
            csr_pin_rows,
            csr_pin_refs,
            net_label_ranges,
            csr_labels,
            net_candidate_ranges,
            csr_candidates,
            net_ground_ranges,
            csr_ground,
            key_to_net,
            name_to_nets,
        }
    }

    /// Document revision this circuit was built or last updated from.
    #[inline]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Number of live parts in the circuit.
    #[inline]
    pub fn part_count(&self) -> usize {
        self.part_ids.len()
    }

    /// Number of live pins in the circuit.
    #[inline]
    pub fn pin_count(&self) -> usize {
        self.pin_part_rows.len()
    }

    /// Number of live logical nets in the circuit.
    #[inline]
    pub fn net_count(&self) -> usize {
        self.net_ids.len()
    }

    /// Read-only access to the string interner.
    #[inline]
    pub fn interner(&self) -> &Interner {
        &self.interner
    }

    /// Resolves an interned string handle.
    #[inline]
    pub fn str_of(&self, id: StrId) -> &str {
        self.interner.resolve(id)
    }

    // ---- Part accessors ----

    /// Looks up the dense [`PartRow`] for a live `PartId`.
    #[inline]
    pub fn part_row(&self, id: PartId) -> Option<PartRow> {
        let raw = *self.part_id_to_row.get(id.0 as usize)?;
        (raw != ROW_NONE).then_some(PartRow(raw))
    }

    /// Iterates all part rows in ascending `PartId` order.
    #[inline]
    pub fn part_rows(&self) -> impl ExactSizeIterator<Item = PartRow> {
        (0..self.part_ids.len()).map(|i| PartRow(i as u32))
    }

    #[inline]
    pub fn part_id(&self, row: PartRow) -> PartId {
        self.part_ids[row.index()]
    }

    #[inline]
    pub fn part_designator_id(&self, row: PartRow) -> StrId {
        self.part_designators[row.index()]
    }

    #[inline]
    pub fn part_designator(&self, row: PartRow) -> &str {
        self.interner.resolve(self.part_designators[row.index()])
    }

    #[inline]
    pub fn part_value_id(&self, row: PartRow) -> StrId {
        self.part_values[row.index()]
    }

    #[inline]
    pub fn part_value(&self, row: PartRow) -> &str {
        self.interner.resolve(self.part_values[row.index()])
    }

    #[inline]
    pub fn part_symbol(&self, row: PartRow) -> SymbolId {
        self.part_symbols[row.index()]
    }

    #[inline]
    pub fn part_symbol_class(&self, row: PartRow) -> SymbolClass {
        self.part_symbol_classes[row.index()]
    }

    /// Iterates the [`PinRow`]s of `row` in stored pin index order (`0..len`).
    #[inline]
    pub fn part_pin_rows(&self, row: PartRow) -> impl ExactSizeIterator<Item = PinRow> {
        let r = self.part_pin_ranges[row.index()];
        (r.start..(r.start + r.len)).map(PinRow)
    }

    // ---- Pin accessors ----

    /// Looks up the dense [`PinRow`] for `(part, pin_index)`.
    #[inline]
    pub fn pin_row(&self, part: PartId, pin_index: u32) -> Option<PinRow> {
        let part_row = self.part_row(part)?;
        let range = self.part_pin_ranges[part_row.index()];
        (pin_index < range.len).then(|| PinRow(range.start + pin_index))
    }

    #[inline]
    pub fn pin_part_row(&self, row: PinRow) -> PartRow {
        self.pin_part_rows[row.index()]
    }

    #[inline]
    pub fn pin_part_id(&self, row: PinRow) -> PartId {
        self.part_ids[self.pin_part_rows[row.index()].index()]
    }

    #[inline]
    pub fn pin_index(&self, row: PinRow) -> u32 {
        self.pin_indices[row.index()]
    }

    #[inline]
    pub fn pin_number_id(&self, row: PinRow) -> Option<StrId> {
        self.pin_numbers[row.index()]
    }

    #[inline]
    pub fn pin_number(&self, row: PinRow) -> Option<&str> {
        self.pin_numbers[row.index()].map(|id| self.interner.resolve(id))
    }

    #[inline]
    pub fn pin_role(&self, row: PinRow) -> PinRole {
        self.pin_roles[row.index()]
    }

    #[inline]
    pub fn pin_net_row(&self, row: PinRow) -> NetRow {
        self.pin_nets[row.index()]
    }

    #[inline]
    pub fn pin_position(&self, row: PinRow) -> Pt {
        self.pin_positions[row.index()]
    }

    /// O(1) point query: returns the [`NetRef`] of `(part, pin_index)` (CB3, section 12).
    #[inline]
    pub fn net_of_pin(&self, part: PartId, pin_index: u32) -> Option<NetRef> {
        let pin_row = self.pin_row(part, pin_index)?;
        Some(NetRef {
            row: self.pin_nets[pin_row.index()],
            revision: self.revision,
        })
    }

    // ---- Net accessors ----

    /// Iterates all net rows in CD18 net order (pin nets, then label-only nets, then wire-only
    /// islands).
    #[inline]
    pub fn net_rows(&self) -> impl ExactSizeIterator<Item = NetRow> {
        (0..self.net_ids.len()).map(|i| NetRow(i as u32))
    }

    /// O(1) lookup of a net row by its document `NetId` at this revision.
    #[inline]
    pub fn net_row_by_doc_id(&self, id: NetId) -> Option<NetRow> {
        self.net_id_to_row.get(&id).copied()
    }

    /// O(1) lookup of a net by its [`CircuitKey`] (CB3).
    #[inline]
    pub fn net_by_key(&self, key: &CircuitKey) -> Option<NetRef> {
        self.key_to_net.get(key).copied().map(|row| NetRef {
            row,
            revision: self.revision,
        })
    }

    /// O(1) reverse lookup from a display name or candidate label name to matching [`NetRow`]s,
    /// sorted in CD18 net order (CB3, section 11.2, CK13).
    #[inline]
    pub fn nets_by_name(&self, name: &str) -> &[NetRow] {
        let Some(id) = self.interner.get(name) else {
            return &[];
        };
        self.name_to_nets.get(&id).map_or(&[], SmallVec::as_slice)
    }

    #[inline]
    pub fn net_doc_id(&self, row: NetRow) -> NetId {
        self.net_ids[row.index()]
    }

    #[inline]
    pub fn net_key(&self, row: NetRow) -> &CircuitKey {
        &self.net_keys[row.index()]
    }

    #[inline]
    pub fn net_display_name_id(&self, row: NetRow) -> StrId {
        self.net_display_names[row.index()]
    }

    #[inline]
    pub fn net_display_name(&self, row: NetRow) -> &str {
        self.interner.resolve(self.net_display_names[row.index()])
    }

    /// Other distinct label texts on `row`, sorted in ascending byte order (CK2, CD4).
    #[inline]
    pub fn net_candidate_name_ids(&self, row: NetRow) -> &[StrId] {
        &self.csr_candidates[self.net_candidate_ranges[row.index()].as_usize_range()]
    }

    /// Iterates the other distinct label texts on `row` in ascending byte order (CK2, CD4).
    #[inline]
    pub fn net_candidate_names(&self, row: NetRow) -> impl ExactSizeIterator<Item = &str> {
        self.net_candidate_name_ids(row)
            .iter()
            .map(|&id| self.interner.resolve(id))
    }

    #[inline]
    pub fn net_kind(&self, row: NetRow) -> NetKind {
        self.net_kinds[row.index()]
    }

    #[inline]
    pub fn net_kind_evidence(&self, row: NetRow) -> EvidenceTier {
        self.net_kind_evidence[row.index()]
    }

    #[inline]
    pub fn net_flags(&self, row: NetRow) -> NetFlags {
        self.net_flags[row.index()]
    }

    #[inline]
    pub fn net_is_ground_reference(&self, row: NetRow) -> bool {
        self.net_flags[row.index()].is_ground_reference()
    }

    #[inline]
    pub fn net_is_hub(&self, row: NetRow) -> bool {
        self.net_flags[row.index()].is_hub()
    }

    #[inline]
    pub fn net_changed_at(&self, row: NetRow) -> u64 {
        self.net_changed_at[row.index()]
    }

    /// Contiguous CSR slice of member [`PinRow`]s on `row`, in CD18 pin order (CD7, CD18).
    #[inline]
    pub fn net_pin_rows(&self, row: NetRow) -> &[PinRow] {
        &self.csr_pin_rows[self.net_pin_ranges[row.index()].as_usize_range()]
    }

    /// Contiguous CSR slice of member `(PartId, stored_pin_index)` pairs on `row`, in CD18 pin
    /// order (CD7, CD18, ready for C1.9).
    #[inline]
    pub fn net_pins(&self, row: NetRow) -> &[(PartId, u32)] {
        &self.csr_pin_refs[self.net_pin_ranges[row.index()].as_usize_range()]
    }

    /// Contiguous CSR slice of member [`LabelId`]s on `row`, in CD18 label order (CD18).
    #[inline]
    pub fn net_labels(&self, row: NetRow) -> &[LabelId] {
        &self.csr_labels[self.net_label_ranges[row.index()].as_usize_range()]
    }

    /// Contiguous CSR slice of declared [`GroundEvidence`] items on `row` (CK4).
    #[inline]
    pub fn net_ground_evidence(&self, row: NetRow) -> &[GroundEvidence] {
        &self.csr_ground[self.net_ground_ranges[row.index()].as_usize_range()]
    }
}

#[inline]
fn classify_symbol(symbols: &SymbolLibrary, id: SymbolId) -> SymbolClass {
    if id.index() < symbols.len() && symbols.get(id).name == "Ground-COM-General" {
        SymbolClass::Ground
    } else {
        SymbolClass::Component
    }
}

#[inline]
fn push_unique_net_row(list: &mut SmallVec<[NetRow; 1]>, row: NetRow) {
    // Callers visit `NetRow`s in strictly ascending order, so checking the last entry suffices.
    if list.last().copied() != Some(row) {
        list.push(row);
    }
}

fn cmp_temp_nets<'a>(
    a: &TempNet,
    b: &TempNet,
    pin_key: &impl Fn(PinRow) -> PinSortKey<'a>,
    label_key: &impl Fn(LabelId) -> LabelSortKey<'a>,
) -> Ordering {
    let group_a = net_group(a);
    let group_b = net_group(b);
    group_a.cmp(&group_b).then_with(|| match group_a {
        0 => pin_key(a.pins[0]).cmp_cd18(&pin_key(b.pins[0])),
        1 => label_key(a.labels[0]).cmp_cd18(&label_key(b.labels[0])),
        _ => a
            .anchor_wire
            .expect("wire-only net has anchor wire")
            .1
            .cmp(&b.anchor_wire.expect("wire-only net has anchor wire").1),
    })
}

#[inline]
fn net_group(net: &TempNet) -> u8 {
    if !net.pins.is_empty() {
        0
    } else if !net.labels.is_empty() {
        1
    } else {
        2
    }
}
