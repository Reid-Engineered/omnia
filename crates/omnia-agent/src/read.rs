//! Read projections (A1.4): the summary, items paged in id order, the three query modes, the
//! canonical nets and the symbol catalog (mcp-plan.md sections 9.3, 10.1 and 10.2).
//!
//! Every function reads a `&Doc` through its public API only and returns the A1.3 DTOs; none
//! can change the document (AINV3). The bytes depend only on the document's content and ids
//! (AD8, AINV5):
//!
//! - items are listed by kind (part, wire, label) and then id, whatever order the spatial index
//!   or the arena hands them over in;
//! - nets are grouped through `NetId`s that never leave this module, each net's members are
//!   sorted, the net is keyed by its lowest pin `(part id, pin index)`, else its lowest wire,
//!   else its lowest label, and the nets are listed in key order (pin keys, then wire keys, then
//!   label keys). So neither `NetId` numbering, hash order nor insertion order shows.
//!
//! Paging (AK4) is keyset paging: the cursor is the id of the last item on the previous page,
//! for example `wire:7`, and the next page holds the items after it in canonical order. A cursor
//! therefore stays meaningful across edits: items added after it appear, removed ones vanish,
//! and none is repeated.
//!
//! Errors use envelope-space JSON Pointers (`/params/cursor`), as `Request::from_slice` does;
//! a transport answering the MCP form drops the leading `/params` (section 9.1).

use std::collections::BTreeMap;

use omnia_doc::{Doc, ItemId, Label, LabelId, NetId, Part, PartId, SymbolId, Wire, WireId};
use omnia_symbols::SymbolLibrary;
use serde::{Deserialize, Serialize};

use crate::{
    AgentError, DEFAULT_PAGE_SIZE, ErrorKind, MAX_PAGE_SIZE,
    error::Limit,
    schema::{
        Bounds, Include, InspectArgs, InspectResult, Item, ItemRef, ItemsPage, LabelBody,
        LabelItem, Net, NetKey, NetsArgs, NetsResult, PartBody, PartItem, PinRef, Point, QueryArgs,
        QueryMode, Summary, WireBody, WireItem,
    },
};

/// Pointer of a refused `cursor`, in envelope space.
pub const CURSOR_PATH: &str = "/params/cursor";

/// Pointer of a refused `limit`, in envelope space.
pub const LIMIT_PATH: &str = "/params/limit";

/// Pointer of a `nets` request naming both `at` and `wire`, in envelope space.
pub const NETS_PATH: &str = "/params";

// ---- summary ----

/// Item counts: the number of live parts, wires and labels (ATC-read-01, AB1).
pub fn summary(doc: &Doc) -> Summary {
    Summary {
        parts: count(doc.parts()),
        wires: count(doc.wires()),
        labels: count(doc.labels()),
    }
}

fn count<T>(it: impl Iterator<Item = T>) -> u64 {
    u64::try_from(it.count()).unwrap_or(u64::MAX)
}

// ---- items ----

/// One live item with its fields, or `None` for a missing or removed id.
///
/// A part's `symbol` is its symbol's name (AD11). A part whose symbol id is not in `symbols`
/// (a document built against another library) is `internal`: the read refuses rather than
/// invent a name.
pub fn item(doc: &Doc, symbols: &SymbolLibrary, id: ItemId) -> Result<Option<Item>, AgentError> {
    Ok(match id {
        ItemId::Part(p) => match doc.part(p) {
            Some(part) => Some(part_item(symbols, p, part)?),
            None => None,
        },
        ItemId::Wire(w) => doc.wire(w).map(|wire| wire_item(w, *wire)),
        ItemId::Label(l) => doc.label(l).map(|label| label_item(l, label)),
    })
}

fn part_item(symbols: &SymbolLibrary, id: PartId, part: &Part) -> Result<Item, AgentError> {
    Ok(Item::Part(PartItem {
        id: id.into(),
        part: PartBody {
            symbol: symbol_name(symbols, part.symbol)?.to_owned(),
            at: part.at.into(),
            orient: part.orient.into(),
            designator: part.designator.to_string(),
            value: part.value.to_string(),
        },
    }))
}

fn wire_item(id: WireId, wire: Wire) -> Item {
    Item::Wire(WireItem {
        id: id.into(),
        wire: WireBody {
            a: wire.a.into(),
            b: wire.b.into(),
        },
    })
}

fn label_item(id: LabelId, label: &Label) -> Item {
    Item::Label(LabelItem {
        id: id.into(),
        label: LabelBody {
            at: label.at.into(),
            text: label.text.to_string(),
        },
    })
}

fn symbol_name(symbols: &SymbolLibrary, id: SymbolId) -> Result<&'static str, AgentError> {
    if id.index() < symbols.len() {
        Ok(symbols.get(id).name)
    } else {
        Err(AgentError::with_message(
            ErrorKind::Internal,
            format!(
                "part uses symbol {} which the library does not have",
                id.raw()
            ),
        ))
    }
}

/// Every live item id in canonical order: parts, then wires, then labels, each by id.
fn all_ids(doc: &Doc) -> impl Iterator<Item = ItemRef> + '_ {
    doc.parts()
        .map(|(id, _)| ItemRef(ItemId::Part(id)))
        .chain(doc.wires().map(|(id, _)| ItemRef(ItemId::Wire(id))))
        .chain(doc.labels().map(|(id, _)| ItemRef(ItemId::Label(id))))
}

/// A checked page request: the exclusive lower bound and the page size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PageSpec {
    after: Option<ItemRef>,
    limit: usize,
}

/// Checks `cursor` and `limit` in the decoder's order: a page size over AK4 is
/// `limit_exceeded` before any schema check; then the pointers in byte order, `/params/cursor`
/// before `/params/limit`.
fn page_spec(cursor: Option<&str>, limit: Option<u32>) -> Result<PageSpec, AgentError> {
    if let Some(n) = limit
        && n > MAX_PAGE_SIZE
    {
        return Err(AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::PageSize,
            value: u64::from(n),
        }));
    }
    let after = match cursor {
        None => None,
        Some(c) => Some(c.parse::<ItemRef>().map_err(|_| {
            AgentError::with_message(
                ErrorKind::SchemaViolation {
                    path: CURSOR_PATH.to_owned(),
                },
                "not a cursor: pass back a next_cursor exactly as it was returned",
            )
        })?),
    };
    let limit = match limit {
        None => DEFAULT_PAGE_SIZE,
        Some(0) => {
            return Err(AgentError::with_message(
                ErrorKind::SchemaViolation {
                    path: LIMIT_PATH.to_owned(),
                },
                "a page holds at least one item",
            ));
        }
        Some(n) => n,
    };
    Ok(PageSpec {
        after,
        limit: usize::try_from(limit).unwrap_or(usize::MAX),
    })
}

/// One page of `ids`, which must be ascending and free of repeats.
fn paginate(
    doc: &Doc,
    symbols: &SymbolLibrary,
    ids: impl Iterator<Item = ItemRef>,
    spec: PageSpec,
) -> Result<ItemsPage, AgentError> {
    let mut ids = ids.filter(|id| spec.after.is_none_or(|after| *id > after));
    let mut items = Vec::with_capacity(spec.limit.min(64));
    for id in ids.by_ref().take(spec.limit) {
        if let Some(item) = item(doc, symbols, id.0)? {
            items.push(item);
        }
    }
    let next_cursor = match ids.next() {
        Some(_) => items.last().map(|i| i.id().to_string()),
        None => None,
    };
    Ok(ItemsPage { items, next_cursor })
}

/// The ids `mode` finds in `area`, sorted by kind and then id, without repeats.
///
/// `envelope` is [`Doc::query`] (index bounds: a part's body and its pins), `within` is
/// [`Doc::query_within`] (WD8 Enclosed) and `touching` is [`Doc::query_touching`] (WD8
/// Intersecting); their edge rules are those functions' rules.
pub fn query_ids(doc: &Doc, area: Bounds, mode: QueryMode) -> Vec<ItemRef> {
    let mut out = Vec::new();
    match mode {
        QueryMode::Envelope => doc.query(area.rect(), &mut out),
        QueryMode::Within => doc.query_within(area.rect(), &mut out),
        QueryMode::Touching => doc.query_touching(area.rect(), &mut out),
    }
    let mut ids: Vec<ItemRef> = out.into_iter().map(ItemRef).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// The result of `query`: one page of the items `mode` finds in `area`, in canonical order.
pub fn query(
    doc: &Doc,
    symbols: &SymbolLibrary,
    args: &QueryArgs,
) -> Result<ItemsPage, AgentError> {
    let spec = page_spec(args.cursor.as_deref(), args.limit)?;
    paginate(
        doc,
        symbols,
        query_ids(doc, args.area, args.mode).into_iter(),
        spec,
    )
}

/// One page of every live item in canonical order (`omnia://doc/{doc_id}/items`).
pub fn items(
    doc: &Doc,
    symbols: &SymbolLibrary,
    cursor: Option<&str>,
    limit: Option<u32>,
) -> Result<ItemsPage, AgentError> {
    let spec = page_spec(cursor, limit)?;
    paginate(doc, symbols, all_ids(doc), spec)
}

/// The result of `inspect`: exactly the parts `include` names.
///
/// `area`, `cursor` and `limit` page the items; with `area` the items are those the
/// `envelope` mode finds there, as `query` would. The summary and the nets always cover the
/// whole document. `cursor` and `limit` are checked even when `include` leaves out the items,
/// so a request is refused or accepted on its own terms.
pub fn inspect(
    doc: &Doc,
    symbols: &SymbolLibrary,
    args: &InspectArgs,
) -> Result<InspectResult, AgentError> {
    let spec = page_spec(args.cursor.as_deref(), args.limit)?;
    let wants = |part| args.include.contains(&part);
    let items = if wants(Include::Items) {
        Some(match args.area {
            Some(area) => paginate(
                doc,
                symbols,
                query_ids(doc, area, QueryMode::Envelope).into_iter(),
                spec,
            )?,
            None => paginate(doc, symbols, all_ids(doc), spec)?,
        })
    } else {
        None
    };
    Ok(InspectResult {
        summary: wants(Include::Summary).then(|| summary(doc)),
        items,
        nets: wants(Include::Nets).then(|| all_nets(doc)),
    })
}

// ---- nets ----

/// A net's members while grouping; pushed in canonical order, so already sorted.
#[derive(Default)]
struct Members {
    pins: Vec<PinRef>,
    wires: Vec<WireId>,
    labels: Vec<LabelId>,
}

impl Members {
    fn into_net(self) -> Net {
        let key = if let Some(pin) = self.pins.first() {
            NetKey::Pin {
                part: pin.part,
                pin: pin.pin,
            }
        } else if let Some(&wire) = self.wires.first() {
            NetKey::Wire { wire: wire.into() }
        } else {
            let label = *self.labels.first().expect("a net has at least one member");
            NetKey::Label {
                label: label.into(),
            }
        };
        Net {
            key,
            pins: self.pins,
            wires: self.wires.into_iter().map(Into::into).collect(),
            labels: self.labels.into_iter().map(Into::into).collect(),
        }
    }
}

/// The order of net keys: pin keys by `(part, pin)`, then wire keys, then label keys (AD8).
pub fn net_key_order(key: NetKey) -> (u8, u32, u32) {
    match key {
        NetKey::Pin { part, pin } => (0, part.0, pin),
        NetKey::Wire { wire } => (1, wire.0, 0),
        NetKey::Label { label } => (2, label.0, 0),
    }
}

/// Every net of the document, canonical (AD8): members sorted, keyed by the lowest pin, else
/// wire, else label, and listed in key order.
///
/// A net is what `omnia-doc`'s connectivity calls one (D2): the points of wire ends, pins and
/// labels, joined by wires and by equal label text. Every live pin, wire and label is in
/// exactly one net; a pin or label alone at its point is a net of its own.
pub fn all_nets(doc: &Doc) -> Vec<Net> {
    collect_nets(doc, None)
}

fn collect_nets(doc: &Doc, only: Option<NetId>) -> Vec<Net> {
    let keep = |net: NetId| only.is_none_or(|o| o == net);
    let mut groups: BTreeMap<NetId, Members> = BTreeMap::new();
    for (id, _) in doc.parts() {
        for (index, &at) in doc.pins(id).unwrap_or_default().iter().enumerate() {
            if let Some(net) = doc.net_at(at)
                && keep(net)
            {
                groups.entry(net).or_default().pins.push(PinRef {
                    part: id.into(),
                    pin: u32::try_from(index).expect("pin index fits u32"),
                });
            }
        }
    }
    for (id, _) in doc.wires() {
        if let Some(net) = doc.net_of(id)
            && keep(net)
        {
            groups.entry(net).or_default().wires.push(id);
        }
    }
    for (id, label) in doc.labels() {
        if let Some(net) = doc.net_at(label.at)
            && keep(net)
        {
            groups.entry(net).or_default().labels.push(id);
        }
    }
    let mut nets: Vec<Net> = groups.into_values().map(Members::into_net).collect();
    nets.sort_unstable_by_key(|net| net_key_order(net.key));
    nets
}

/// The result of `nets`: every net; with `at`, the net at that point (none when no wire end,
/// pin or label sits exactly there, since a wire's interior is not a connection point); with
/// `wire`, that wire's net (`unknown_item` when the wire does not exist).
pub fn nets(doc: &Doc, args: &NetsArgs) -> Result<NetsResult, AgentError> {
    let only = match (args.at, args.wire) {
        (Some(_), Some(_)) => {
            return Err(AgentError::with_message(
                ErrorKind::SchemaViolation {
                    path: NETS_PATH.to_owned(),
                },
                "name at most one of at and wire",
            ));
        }
        (Some(at), None) => match doc.net_at(at.into()) {
            Some(net) => Some(net),
            None => return Ok(NetsResult { nets: Vec::new() }),
        },
        (None, Some(wire)) => Some(doc.net_of(wire.into()).ok_or_else(|| {
            AgentError::with_message(ErrorKind::UnknownItem, format!("{wire} does not exist"))
        })?),
        (None, None) => None,
    };
    Ok(NetsResult {
        nets: collect_nets(doc, only),
    })
}

// ---- symbols ----

/// One pin of a symbol, in symbol-local nanometres, in the symbol's pin order (the index a
/// [`PinRef`] names).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolPin {
    pub number: String,
    pub at: Point,
}

/// A symbol as agents see it (`omnia://symbols`, section 10.2): its name (what `place_part`
/// takes, AD11), its local bounds, its placed size (K7: the bounds' width and height in
/// nanometres) and its pins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SymbolInfo {
    pub name: String,
    pub bounds: Bounds,
    pub width: u32,
    pub height: u32,
    pub pins: Vec<SymbolPin>,
}

fn symbol_info(def: &omnia_symbols::SymbolDef) -> SymbolInfo {
    let bounds = Bounds::new(def.bounds).expect("a built-in symbol's bounds lie inside K1");
    let extent = |lo: i32, hi: i32| {
        u32::try_from(i64::from(hi) - i64::from(lo)).expect("min <= max, and K1 spans fit u32")
    };
    SymbolInfo {
        name: def.name.to_owned(),
        bounds,
        width: extent(def.bounds.min.x, def.bounds.max.x),
        height: extent(def.bounds.min.y, def.bounds.max.y),
        pins: def
            .pins
            .iter()
            .map(|pin| SymbolPin {
                number: pin.number.to_owned(),
                at: pin.at.into(),
            })
            .collect(),
    }
}

/// Every symbol of the library, in symbol id order.
pub fn symbols(symbols: &SymbolLibrary) -> Vec<SymbolInfo> {
    symbols.iter().map(symbol_info).collect()
}

/// The symbol called `name`, if the library has one.
pub fn symbol(symbols: &SymbolLibrary, name: &str) -> Option<SymbolInfo> {
    symbols.by_name(name).map(|id| symbol_info(symbols.get(id)))
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, HashMap},
        sync::Arc,
    };

    use omnia_doc::{Command, Orient, PIN_LATTICE_NM, Pt, Rect};
    use proptest::prelude::*;
    use serde_json::Value;

    use super::*;
    use crate::{
        OpName, Output, Response,
        schema::{DocId, LabelRef, OrientName, PartRef, WireRef, schema_text, validate::validate},
    };

    const L: i32 = PIN_LATTICE_NM;

    fn lib() -> Arc<SymbolLibrary> {
        SymbolLibrary::builtin()
    }

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * L, y: y * L }
    }

    fn bounds(min: Pt, max: Pt) -> Bounds {
        Bounds::new(Rect::new(min, max).unwrap()).unwrap()
    }

    type PartSpec = (Part, Vec<Pt>, Rect);

    fn part_spec(
        symbols: &SymbolLibrary,
        sym: usize,
        at: Pt,
        orient: Orient,
        n: usize,
    ) -> PartSpec {
        let def = symbols.iter().nth(sym % symbols.len()).unwrap();
        (
            Part {
                symbol: def.id,
                at,
                orient,
                designator: format!("U{n}").into(),
                value: format!("{n}k").into(),
            },
            def.pins.iter().map(|p| p.at).collect(),
            def.bounds,
        )
    }

    fn wire(a: Pt, b: Pt) -> Wire {
        Wire { a, b }
    }

    fn label(at: Pt, text: &str) -> Label {
        Label {
            at,
            text: text.into(),
        }
    }

    /// Wraps `output` in a response, checks it against the published `<op>.response` schema
    /// text with the test-only validator, decodes it back and returns the bytes.
    fn checked_json(op: OpName, revision: u64, output: Output) -> String {
        let response = Response::new(Some(DocId(1)), Some(revision), Ok(output)).unwrap();
        let json = response.to_json();
        let schema: Value =
            serde_json::from_str(&schema_text(&format!("{}.response", op.as_str())).unwrap())
                .unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        validate(&schema, &value).unwrap_or_else(|v| panic!("{op:?} result off schema: {v:?}"));
        assert_eq!(Response::from_slice(op, json.as_bytes()).unwrap(), response);
        json
    }

    fn inspect_all(doc: &Doc, symbols: &SymbolLibrary) -> InspectResult {
        inspect(
            doc,
            symbols,
            &InspectArgs {
                doc_id: DocId(1),
                include: vec![Include::Summary, Include::Items, Include::Nets],
                area: None,
                cursor: None,
                limit: Some(MAX_PAGE_SIZE),
            },
        )
        .unwrap()
    }

    fn inspect_args(
        include: Vec<Include>,
        area: Option<Bounds>,
        cursor: Option<String>,
        limit: Option<u32>,
    ) -> InspectArgs {
        InspectArgs {
            doc_id: DocId(1),
            include,
            area,
            cursor,
            limit,
        }
    }

    fn query_args(
        area: Bounds,
        mode: QueryMode,
        cursor: Option<String>,
        limit: Option<u32>,
    ) -> QueryArgs {
        QueryArgs {
            doc_id: DocId(1),
            area,
            mode,
            cursor,
            limit,
        }
    }

    fn nets_args(at: Option<Pt>, wire: Option<u32>) -> NetsArgs {
        NetsArgs {
            doc_id: DocId(1),
            at: at.map(Into::into),
            wire: wire.map(WireRef),
        }
    }

    // ---- ATC-read-01 ----

    /// The `100` stress sheet's geometry (omnia-bench `generate`, `Parts100`, which this crate
    /// cannot depend on): 10 columns at a 12-lattice pitch, the symbols and orientations in
    /// turn, and two unit wires from each part's first pin.
    fn sheet_100(symbols: &SymbolLibrary) -> Doc {
        let pitch = 12;
        let mut parts = Vec::new();
        let mut wires = Vec::new();
        for i in 0..100 {
            let at = pt((i % 10) as i32 * pitch - 54, (i / 10) as i32 * pitch - 54);
            let orient = Orient::ALL[i % 8];
            let spec = part_spec(symbols, i, at, orient, i + 1);
            let pin = at + orient.apply(spec.1[0]);
            wires.push(wire(pin, pin + pt(1, 0)));
            wires.push(wire(pin, pin + pt(0, 1)));
            parts.push(spec);
        }
        Doc::from_items(parts, wires, std::iter::empty::<Label>()).unwrap()
    }

    #[test]
    fn atc_read_01_summary_of_an_opened_100_sheet() {
        let symbols = lib();
        // An edit and its undo advance the file's revision and leave the content as generated.
        let mut sheet = sheet_100(&symbols);
        sheet
            .apply(Command::AddLabel(label(pt(0, 0), "X")))
            .unwrap();
        sheet.undo().unwrap();
        let saved = sheet.revision();
        assert!(saved > 0);
        let mut bytes = Vec::new();
        sheet.save(&mut bytes).unwrap();
        let doc = Doc::load(bytes.as_slice()).unwrap();

        let result = inspect(
            &doc,
            &symbols,
            &inspect_args(vec![Include::Summary], None, None, None),
        )
        .unwrap();
        let summary = result.summary.unwrap();
        assert_eq!(summary.parts, doc.parts().count() as u64);
        assert_eq!(summary.wires, doc.wires().count() as u64);
        assert_eq!(summary.labels, doc.labels().count() as u64);
        assert_eq!(
            (summary.parts, summary.wires, summary.labels),
            (100, 200, 0)
        );
        assert!(result.items.is_none() && result.nets.is_none());
        assert_eq!(doc.revision(), saved, "the revision is the file's");
        let json = checked_json(OpName::Inspect, doc.revision(), Output::Inspect(result));
        assert!(json.contains(&format!(r#""revision":{saved},"#)), "{json}");
        assert!(
            json.contains(r#""result":{"summary":{"parts":100,"wires":200,"labels":0}}"#),
            "{json}"
        );
    }

    // ---- ATC-net-01 ----

    #[test]
    fn atc_net_01_wires_crossing_at_interiors_are_two_nets() {
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![
            wire(pt(-2, 0), pt(2, 0)),
            wire(pt(0, -2), pt(0, 2)),
        ]))
        .unwrap();
        assert_eq!(
            doc.wires().count(),
            2,
            "a pure interior crossing splits nothing (INV2)"
        );
        let result = nets(&doc, &nets_args(None, None)).unwrap();
        assert_eq!(result.nets.len(), 2);
        let keys: Vec<_> = result.nets.iter().map(|n| n.key).collect();
        assert_eq!(
            keys,
            [
                NetKey::Wire { wire: WireRef(0) },
                NetKey::Wire { wire: WireRef(1) }
            ]
        );
        for (i, net) in result.nets.iter().enumerate() {
            assert_eq!(net.wires, [WireRef(i as u32)]);
            assert!(net.pins.is_empty() && net.labels.is_empty());
        }
        // The crossing point is no connection point.
        assert!(
            nets(&doc, &nets_args(Some(Pt::ZERO), None))
                .unwrap()
                .nets
                .is_empty()
        );
        checked_json(OpName::Nets, doc.revision(), Output::Nets(result));
    }

    // ---- nets ----

    #[test]
    fn nets_keys_prefer_pins_then_wires_then_labels() {
        let symbols = lib();
        let r = part_spec(&symbols, 5, pt(0, 0), Orient::IDENTITY, 1);
        let pin0 = r.1[0];
        let doc = Doc::from_items(
            [r],
            [wire(pt(20, 0), pt(21, 0)), wire(pin0, pin0 + pt(0, 3))],
            [
                label(pt(40, 0), "LONE"),
                label(pt(21, 0), "N"),
                label(pin0 + pt(0, 3), "N"),
            ],
        )
        .unwrap();
        let all = all_nets(&doc);
        // The pin net: pin 0, wire 1 and, through the shared text "N", wire 0 and labels 1, 2.
        assert_eq!(
            all[0].key,
            NetKey::Pin {
                part: PartRef(0),
                pin: 0
            }
        );
        assert_eq!(
            all[0].pins,
            [PinRef {
                part: PartRef(0),
                pin: 0
            }]
        );
        assert_eq!(all[0].wires, [WireRef(0), WireRef(1)]);
        assert_eq!(all[0].labels, [LabelRef(1), LabelRef(2)]);
        assert_eq!(
            all[1].key,
            NetKey::Pin {
                part: PartRef(0),
                pin: 1
            }
        );
        let last = all.last().unwrap();
        assert_eq!(last.key, NetKey::Label { label: LabelRef(0) });
        assert_eq!(all.len(), doc.net_count());

        // One net by wire and by point; the same net either way.
        let by_wire = nets(&doc, &nets_args(None, Some(0))).unwrap().nets;
        let by_pin = nets(&doc, &nets_args(Some(pin0), None)).unwrap().nets;
        assert_eq!(by_wire, [all[0].clone()]);
        assert_eq!(by_pin, by_wire);
        let err = nets(&doc, &nets_args(None, Some(9))).unwrap_err();
        assert_eq!(err.kind, ErrorKind::UnknownItem);
        let err = nets(&doc, &nets_args(Some(pin0), Some(0))).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::SchemaViolation {
                path: NETS_PATH.to_owned()
            }
        );
        for result in [
            nets(&doc, &nets_args(None, None)).unwrap(),
            NetsResult { nets: by_pin },
        ] {
            checked_json(OpName::Nets, 0, Output::Nets(result));
        }
    }

    /// Each net as its sorted member lists.
    type Partition = BTreeSet<(Vec<PinRef>, Vec<WireRef>, Vec<LabelRef>)>;

    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }

    fn union(parent: &mut [usize], a: usize, b: usize) {
        let (a, b) = (find(parent, a), find(parent, b));
        parent[a.max(b)] = a.min(b);
    }

    /// An independent oracle: union-find over the points of wire ends, pins and labels, joined
    /// by wires and by equal label text (D2). It shares no code with `collect_nets` and reads
    /// no `NetId`.
    fn brute_force_partition(doc: &Doc) -> Partition {
        let mut node: HashMap<(i32, i32), usize> = HashMap::new();
        let mut parent: Vec<usize> = Vec::new();
        let mut at = |p: Pt, parent: &mut Vec<usize>| {
            *node.entry((p.x, p.y)).or_insert_with(|| {
                parent.push(parent.len());
                parent.len() - 1
            })
        };
        let mut pins = Vec::new();
        for (id, _) in doc.parts() {
            for (i, &p) in doc.pins(id).unwrap().iter().enumerate() {
                pins.push((
                    PinRef {
                        part: id.into(),
                        pin: i as u32,
                    },
                    at(p, &mut parent),
                ));
            }
        }
        let mut wires = Vec::new();
        for (id, w) in doc.wires() {
            let (a, b) = (at(w.a, &mut parent), at(w.b, &mut parent));
            union(&mut parent, a, b);
            wires.push((WireRef::from(id), a));
        }
        let mut labels = Vec::new();
        let mut by_text: HashMap<String, usize> = HashMap::new();
        for (id, l) in doc.labels() {
            let n = at(l.at, &mut parent);
            let first = *by_text.entry(l.text.to_string()).or_insert(n);
            union(&mut parent, first, n);
            labels.push((LabelRef::from(id), n));
        }
        type Group = (Vec<PinRef>, Vec<WireRef>, Vec<LabelRef>);
        let mut groups: BTreeMap<usize, Group> = BTreeMap::new();
        for (p, n) in pins {
            groups.entry(find(&mut parent, n)).or_default().0.push(p);
        }
        for (w, n) in wires {
            groups.entry(find(&mut parent, n)).or_default().1.push(w);
        }
        for (l, n) in labels {
            groups.entry(find(&mut parent, n)).or_default().2.push(l);
        }
        groups
            .into_values()
            .map(|(mut p, mut w, mut l)| {
                p.sort();
                w.sort();
                l.sort();
                (p, w, l)
            })
            .collect()
    }

    /// Random content on a small lattice, so pins, wire ends and labels often coincide.
    fn content() -> impl Strategy<Value = (Vec<PartSpec>, Vec<Wire>, Vec<Label>)> {
        let symbols = lib();
        let parts = prop::collection::vec((0usize..64, -5i32..5, -5i32..5, 0usize..8), 0..6);
        let wires = prop::collection::vec((-6i32..6, -6i32..6, any::<bool>(), 1i32..4), 0..14);
        let labels = prop::collection::vec((-6i32..6, -6i32..6, 0usize..3), 0..6);
        (parts, wires, labels).prop_map(move |(parts, wires, labels)| {
            let parts = parts
                .into_iter()
                .enumerate()
                .map(|(n, (s, x, y, o))| {
                    part_spec(&symbols, s, pt(x * 2, y * 2), Orient::ALL[o], n)
                })
                .collect();
            let wires = wires
                .into_iter()
                .map(|(x, y, h, len)| {
                    let a = pt(x, y);
                    wire(a, if h { a + pt(len, 0) } else { a + pt(0, len) })
                })
                .collect();
            let labels = labels
                .into_iter()
                .map(|(x, y, t)| label(pt(x, y), ["A", "B", "C"][t]))
                .collect();
            (parts, wires, labels)
        })
    }

    fn as_partition(nets: &[Net]) -> Partition {
        nets.iter()
            .map(|n| (n.pins.clone(), n.wires.clone(), n.labels.clone()))
            .collect()
    }

    /// The canonical form of a net list, computed independently of `collect_nets`.
    fn canonical_nets(mut nets: Vec<Net>) -> Vec<Net> {
        for net in &mut nets {
            net.pins.sort();
            net.wires.sort();
            net.labels.sort();
            net.key = match (net.pins.first(), net.wires.first(), net.labels.first()) {
                (Some(p), _, _) => NetKey::Pin {
                    part: p.part,
                    pin: p.pin,
                },
                (None, Some(&wire), _) => NetKey::Wire { wire },
                (None, None, Some(&label)) => NetKey::Label { label },
                (None, None, None) => panic!("an empty net"),
            };
        }
        nets.sort_by_key(|n| net_key_order(n.key));
        nets
    }

    fn canonical_items(mut page: ItemsPage) -> ItemsPage {
        page.items.sort_by_key(Item::id);
        page
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Done-when: nets equal a brute-force partition of pins, wires and labels, also after
        /// a delete has renumbered the document's nets.
        #[test]
        fn nets_equal_a_brute_force_partition(
            (parts, wires, labels) in content(),
            delete in prop::collection::vec(any::<bool>(), 26),
        ) {
            let mut doc = Doc::from_items(parts, wires, labels).unwrap();
            for after_delete in [false, true] {
                if after_delete {
                    let ids: Vec<ItemId> = all_ids(&doc)
                        .zip(delete.iter())
                        .filter(|(_, del)| **del)
                        .map(|(id, _)| id.0)
                        .collect();
                    if !ids.is_empty() {
                        doc.apply(Command::Delete { items: ids, with_wires: false }).unwrap();
                    }
                }
                let all = all_nets(&doc);
                prop_assert_eq!(as_partition(&all), brute_force_partition(&doc));
                prop_assert_eq!(all.len(), doc.net_count());
                prop_assert_eq!(&canonical_nets(all.clone()), &all, "already canonical");
                // Every member is in exactly one net.
                let members: usize =
                    all.iter().map(|n| n.pins.len() + n.wires.len() + n.labels.len()).sum();
                let pins: usize = doc.parts().map(|(id, _)| doc.pins(id).unwrap().len()).sum();
                prop_assert_eq!(members, pins + doc.wires().count() + doc.labels().count());
                // `nets {wire}` and `nets {at}` pick that one net out of the full list.
                for (id, w) in doc.wires() {
                    let one = nets(&doc, &nets_args(None, Some(id.0))).unwrap().nets;
                    prop_assert_eq!(one.len(), 1);
                    prop_assert!(one[0].wires.contains(&id.into()));
                    prop_assert!(all.contains(&one[0]));
                    prop_assert_eq!(nets(&doc, &nets_args(Some(w.b), None)).unwrap().nets, one);
                }
                checked_json(OpName::Nets, doc.revision(), Output::Nets(NetsResult { nets: all }));
            }
        }

        /// AINV5: a sheet rebuilt with each kind inserted in reverse order gives byte-identical
        /// output up to the id mapping, and the same request twice gives the same bytes.
        #[test]
        fn ainv5_reverse_insertion_gives_identical_bytes_up_to_ids(
            (parts, wires, labels) in content(),
            area in (-30i32..30, -30i32..30, 0i32..30, 0i32..30),
            probe in (-6i32..6, -6i32..6),
            pick in any::<prop::sample::Index>(),
        ) {
            let symbols = lib();
            let (np, nw, nl) = (parts.len() as u32, wires.len() as u32, labels.len() as u32);
            let a = Doc::from_items(parts.clone(), wires.clone(), labels.clone()).unwrap();
            let b = Doc::from_items(
                parts.into_iter().rev(),
                wires.into_iter().rev(),
                labels.into_iter().rev(),
            )
            .unwrap();
            // b's id i is a's id n - 1 - i, per kind.
            let rp = |p: PartRef| PartRef(np - 1 - p.0);
            let rw = |w: WireRef| WireRef(nw - 1 - w.0);
            let rl = |l: LabelRef| LabelRef(nl - 1 - l.0);
            let map_item = |item: Item| match item {
                Item::Part(mut i) => {
                    i.id = rp(i.id);
                    Item::Part(i)
                }
                Item::Wire(mut i) => {
                    i.id = rw(i.id);
                    Item::Wire(i)
                }
                Item::Label(mut i) => {
                    i.id = rl(i.id);
                    Item::Label(i)
                }
            };
            let map_page = |page: ItemsPage| {
                assert!(page.next_cursor.is_none());
                canonical_items(ItemsPage {
                    items: page.items.into_iter().map(map_item).collect(),
                    next_cursor: None,
                })
            };
            let map_nets = |nets: Vec<Net>| {
                canonical_nets(
                    nets.into_iter()
                        .map(|n| Net {
                            key: n.key,
                            pins: n
                                .pins
                                .into_iter()
                                .map(|p| PinRef { part: rp(p.part), pin: p.pin })
                                .collect(),
                            wires: n.wires.into_iter().map(rw).collect(),
                            labels: n.labels.into_iter().map(rl).collect(),
                        })
                        .collect(),
                )
            };
            let inspect_json = |r: InspectResult| checked_json(OpName::Inspect, 0, Output::Inspect(r));
            let query_json = |r: ItemsPage| checked_json(OpName::Query, 0, Output::Query(r));
            let nets_json = |nets: Vec<Net>| checked_json(OpName::Nets, 0, Output::Nets(NetsResult { nets }));

            // inspect: summary, items and nets.
            let ia = inspect_all(&a, &symbols);
            let ib = inspect_all(&b, &symbols);
            let ib = InspectResult {
                summary: ib.summary,
                items: Some(map_page(ib.items.unwrap())),
                nets: Some(map_nets(ib.nets.unwrap())),
            };
            prop_assert_eq!(&canonical_items(ia.items.clone().unwrap()), ia.items.as_ref().unwrap());
            prop_assert_eq!(inspect_json(ia.clone()), inspect_json(ib));
            prop_assert_eq!(
                inspect_json(ia.clone()),
                inspect_json(inspect_all(&a, &symbols)),
                "the same request twice"
            );
            prop_assert_eq!(inspect_json(ia), inspect_json(inspect_all(&a.fork(), &symbols)));

            // query, every mode.
            let (x, y, w, h) = area;
            let area = bounds(pt(x, y), pt(x + w, y + h));
            for mode in [QueryMode::Envelope, QueryMode::Within, QueryMode::Touching] {
                let args = query_args(area, mode, None, Some(MAX_PAGE_SIZE));
                let qa = query(&a, &symbols, &args).unwrap();
                let qb = query(&b, &symbols, &args).unwrap();
                prop_assert_eq!(query_json(qa), query_json(map_page(qb)));
            }

            // nets at a point and of a wire.
            let at = Some(pt(probe.0, probe.1));
            let na = nets(&a, &nets_args(at, None)).unwrap().nets;
            let nb = nets(&b, &nets_args(at, None)).unwrap().nets;
            prop_assert_eq!(nets_json(na), nets_json(map_nets(nb)));
            if nw > 0 {
                let w = pick.index(nw as usize) as u32;
                let na = nets(&a, &nets_args(None, Some(w))).unwrap().nets;
                let nb = nets(&b, &nets_args(None, Some(rw(WireRef(w)).0))).unwrap().nets;
                prop_assert_eq!(nets_json(na), nets_json(map_nets(nb)));
            }
        }

        /// Query modes are exactly `Doc::query`, `query_within` and `query_touching`, sorted,
        /// with each item's stored fields.
        #[test]
        fn query_modes_project_the_doc_queries(
            (parts, wires, labels) in content(),
            area in (-30i32..30, -30i32..30, 0i32..30, 0i32..30),
        ) {
            let symbols = lib();
            let doc = Doc::from_items(parts, wires, labels).unwrap();
            let (x, y, w, h) = area;
            let area = bounds(pt(x, y), pt(x + w, y + h));
            for mode in [QueryMode::Envelope, QueryMode::Within, QueryMode::Touching] {
                let mut raw = Vec::new();
                match mode {
                    QueryMode::Envelope => doc.query(area.rect(), &mut raw),
                    QueryMode::Within => doc.query_within(area.rect(), &mut raw),
                    QueryMode::Touching => doc.query_touching(area.rect(), &mut raw),
                }
                let mut want: Vec<ItemRef> = raw.into_iter().map(ItemRef).collect();
                want.sort();
                want.dedup();
                let args = query_args(area, mode, None, Some(MAX_PAGE_SIZE));
                let page = query(&doc, &symbols, &args).unwrap();
                let got: Vec<ItemRef> = page.items.iter().map(Item::id).collect();
                prop_assert_eq!(&got, &want);
                for it in &page.items {
                    prop_assert_eq!(Some(it.clone()), item(&doc, &symbols, it.id().0).unwrap());
                }
            }
        }
    }

    // ---- items and paging ----

    fn paging_doc(symbols: &SymbolLibrary) -> Doc {
        Doc::from_items(
            (0..7).map(|i| part_spec(symbols, i, pt(i as i32 * 10, 0), Orient::IDENTITY, i)),
            (0..9).map(|i| wire(pt(i * 2, 20), pt(i * 2 + 1, 20))),
            (0..4).map(|i| label(pt(i * 3, 30), "X")),
        )
        .unwrap()
    }

    fn collect_pages(
        mut next: impl FnMut(Option<String>) -> Result<ItemsPage, AgentError>,
        limit: usize,
    ) -> Vec<Vec<Item>> {
        let mut pages = Vec::new();
        let mut cursor = None;
        loop {
            let page = next(cursor.take()).unwrap();
            assert!(page.items.len() <= limit);
            let last = page.items.last().map(|i| i.id().to_string());
            pages.push(page.items);
            match page.next_cursor {
                Some(c) => {
                    assert_eq!(
                        Some(&c),
                        last.as_ref(),
                        "the cursor is the last id on the page"
                    );
                    cursor = Some(c);
                }
                None => break,
            }
        }
        pages
    }

    #[test]
    fn items_are_paged_in_canonical_order_at_every_limit() {
        let symbols = lib();
        let doc = paging_doc(&symbols);
        let full = items(&doc, &symbols, None, None).unwrap();
        assert!(full.next_cursor.is_none());
        assert_eq!(full.items.len(), 20);
        let ids: Vec<String> = full.items.iter().map(|i| i.id().to_string()).collect();
        assert_eq!(ids[0], "part:0");
        assert_eq!(ids[6], "part:6");
        assert_eq!(ids[7], "wire:0");
        assert_eq!(ids[16], "label:0");
        assert!(full.items.windows(2).all(|w| w[0].id() < w[1].id()));
        let summary = summary(&doc);
        assert_eq!((summary.parts, summary.wires, summary.labels), (7, 9, 4));

        for limit in 1..=21u32 {
            let pages = collect_pages(
                |c| items(&doc, &symbols, c.as_deref(), Some(limit)),
                limit as usize,
            );
            assert_eq!(
                pages.len(),
                20usize.div_ceil(limit as usize),
                "limit {limit}"
            );
            assert_eq!(pages.concat(), full.items, "limit {limit}");
            for page in &pages[..pages.len() - 1] {
                assert_eq!(page.len(), limit as usize, "only the last page is short");
            }
            // The same pages through `inspect`, each one on schema.
            let via_inspect = collect_pages(
                |cursor| {
                    let args = inspect_args(vec![Include::Items], None, cursor, Some(limit));
                    inspect(&doc, &symbols, &args).map(|r| {
                        let page = r.items.clone().unwrap();
                        checked_json(OpName::Inspect, 0, Output::Inspect(r));
                        page
                    })
                },
                limit as usize,
            );
            assert_eq!(via_inspect, pages);
        }
    }

    #[test]
    fn default_page_is_ak4_and_a_full_last_page_has_no_cursor() {
        let symbols = lib();
        let doc = Doc::from_items(
            std::iter::empty::<PartSpec>(),
            std::iter::empty::<Wire>(),
            (0..DEFAULT_PAGE_SIZE as i32 + 1).map(|i| label(pt(i, 0), "L")),
        )
        .unwrap();
        let first = items(&doc, &symbols, None, None).unwrap();
        assert_eq!(first.items.len(), DEFAULT_PAGE_SIZE as usize);
        assert_eq!(first.next_cursor.as_deref(), Some("label:499"));
        let second = items(&doc, &symbols, first.next_cursor.as_deref(), None).unwrap();
        assert_eq!(second.items.len(), 1);
        assert!(second.next_cursor.is_none());
        // Exactly one page's worth: no cursor; a cursor at the end gives an empty last page.
        let exact = items(&doc, &symbols, None, Some(DEFAULT_PAGE_SIZE + 1)).unwrap();
        assert_eq!(exact.items.len(), DEFAULT_PAGE_SIZE as usize + 1);
        assert!(exact.next_cursor.is_none());
        let past = items(&doc, &symbols, Some("label:500"), None).unwrap();
        assert!(past.items.is_empty() && past.next_cursor.is_none());
        let max = items(&doc, &symbols, None, Some(MAX_PAGE_SIZE)).unwrap();
        assert_eq!(max.items.len(), DEFAULT_PAGE_SIZE as usize + 1);
        checked_json(OpName::Query, 0, Output::Query(first));
        checked_json(OpName::Query, 0, Output::Query(past));
    }

    #[test]
    fn a_cursor_survives_edits() {
        let symbols = lib();
        let mut doc = paging_doc(&symbols);
        let first = items(&doc, &symbols, None, Some(8)).unwrap();
        let cursor = first.next_cursor.unwrap();
        assert_eq!(cursor, "wire:0");
        // Remove the item the cursor names and the one after it.
        doc.apply(Command::Delete {
            items: vec![ItemId::Wire(WireId(0)), ItemId::Wire(WireId(1))],
            with_wires: false,
        })
        .unwrap();
        let rest = items(&doc, &symbols, Some(&cursor), None).unwrap();
        let ids: Vec<String> = rest.items.iter().map(|i| i.id().to_string()).collect();
        assert_eq!(ids.first().map(String::as_str), Some("wire:2"));
        assert_eq!(ids.len(), 7 + 4);
    }

    #[test]
    fn bad_cursors_and_limits_are_refused() {
        let symbols = lib();
        let doc = paging_doc(&symbols);
        let violation = |path: &str| ErrorKind::SchemaViolation {
            path: path.to_owned(),
        };
        for bad in [
            "",
            "bogus",
            "part:",
            "part:x",
            "net:1",
            "wire:-1",
            "wire:4294967296",
            " part:1",
        ] {
            let err = items(&doc, &symbols, Some(bad), None).unwrap_err();
            assert_eq!(err.kind, violation(CURSOR_PATH), "{bad:?}");
        }
        let err = items(&doc, &symbols, None, Some(0)).unwrap_err();
        assert_eq!(err.kind, violation(LIMIT_PATH));
        let over = MAX_PAGE_SIZE + 1;
        let err = items(&doc, &symbols, Some("bogus"), Some(over)).unwrap_err();
        assert_eq!(
            err.kind,
            ErrorKind::LimitExceeded {
                limit: Limit::PageSize,
                value: u64::from(over)
            },
            "AK4 comes first, as in the decoder"
        );
        let err = items(&doc, &symbols, Some("bogus"), Some(0)).unwrap_err();
        assert_eq!(
            err.kind,
            violation(CURSOR_PATH),
            "/params/cursor sorts before /params/limit"
        );
        // Every entry point checks the page, inspect even without items.
        let area = bounds(pt(-1, -1), pt(1, 1));
        let args = query_args(area, QueryMode::Within, Some("x".into()), None);
        assert_eq!(
            query(&doc, &symbols, &args).unwrap_err().kind,
            violation(CURSOR_PATH)
        );
        let args = inspect_args(vec![Include::Summary], None, None, Some(0));
        assert_eq!(
            inspect(&doc, &symbols, &args).unwrap_err().kind,
            violation(LIMIT_PATH)
        );
    }

    // ---- query modes ----

    #[test]
    fn query_modes_differ_as_wd8_says() {
        let symbols = lib();
        // A part with a pin stub outside its body (the built-in symbols keep their pins on the
        // body's edge): the envelope finds the part by that pin, within and touching do not.
        let mut spec = part_spec(&symbols, 0, pt(0, 0), Orient::IDENTITY, 1);
        let stub = pt(-4, 0);
        spec.1 = vec![stub];
        spec.2 = Rect::new(pt(-2, -1), pt(2, 1)).unwrap();
        let at = pt(0, 0);
        let doc = Doc::from_items(
            [spec],
            [wire(pt(40, -2), pt(40, 2))],
            [label(pt(60, 0), "N")],
        )
        .unwrap();
        let pid = ItemRef(ItemId::Part(PartId(0)));
        let wid = ItemRef(ItemId::Wire(WireId(0)));
        let lid = ItemRef(ItemId::Label(LabelId(0)));
        let ids = |area, mode| query_ids(&doc, area, mode);

        let pin_only = bounds(at + stub, at + stub);
        assert_eq!(ids(pin_only, QueryMode::Envelope), [pid]);
        assert!(ids(pin_only, QueryMode::Touching).is_empty());
        assert!(ids(pin_only, QueryMode::Within).is_empty());

        let body = Bounds::new(doc.part_body(PartId(0)).unwrap()).unwrap();
        assert_eq!(ids(body, QueryMode::Within), [pid], "inclusive edges");
        assert_eq!(ids(body, QueryMode::Touching), [pid]);

        // A box across the wire's middle touches it but does not enclose it.
        let across = bounds(pt(39, -1), pt(41, 1));
        assert_eq!(ids(across, QueryMode::Touching), [wid]);
        assert!(ids(across, QueryMode::Within).is_empty());
        assert_eq!(ids(bounds(pt(39, -2), pt(41, 2)), QueryMode::Within), [wid]);

        // A label is found by its anchor only.
        assert_eq!(ids(bounds(pt(60, 0), pt(60, 0)), QueryMode::Within), [lid]);
        assert!(ids(bounds(pt(61, 0), pt(62, 1)), QueryMode::Touching).is_empty());

        // Everything, sorted by kind then id, through `query`.
        let all = bounds(pt(-100, -100), pt(100, 100));
        for mode in [QueryMode::Envelope, QueryMode::Within, QueryMode::Touching] {
            let page = query(&doc, &symbols, &query_args(all, mode, None, None)).unwrap();
            let got: Vec<_> = page.items.iter().map(Item::id).collect();
            assert_eq!(got, [pid, wid, lid], "{mode:?}");
            checked_json(OpName::Query, 0, Output::Query(page));
        }
        // `inspect {area}` pages the envelope mode.
        let args = inspect_args(vec![Include::Items], Some(pin_only), None, None);
        let inspected = inspect(&doc, &symbols, &args).unwrap();
        let got: Vec<_> = inspected
            .items
            .unwrap()
            .items
            .iter()
            .map(Item::id)
            .collect();
        assert_eq!(got, [pid]);
    }

    // ---- item fields, inspect shape, symbols ----

    #[test]
    fn items_carry_their_stored_fields() {
        let symbols = lib();
        let spec = part_spec(&symbols, 4, pt(3, -2), Orient::ALL[5], 9);
        let doc = Doc::from_items(
            [spec.clone()],
            [wire(pt(0, 0), pt(0, 4))],
            [label(pt(1, 1), "VCC")],
        )
        .unwrap();
        let page = items(&doc, &symbols, None, None).unwrap();
        let json = serde_json::to_string(&page).unwrap();
        let name = symbols.get(spec.0.symbol).name;
        let orient = OrientName::from(Orient::ALL[5]).as_str();
        let expect = format!(
            concat!(
                r#"{{"items":[{{"id":"part:0","part":{{"symbol":"{name}","at":[{x},{y}],"#,
                r#""orient":"{o}","designator":"U9","value":"9k"}}}},"#,
                r#"{{"id":"wire:0","wire":{{"a":[0,0],"b":[0,{w}]}}}},"#,
                r#"{{"id":"label:0","label":{{"at":[{l},{l}],"text":"VCC"}}}}]}}"#
            ),
            name = name,
            x = 3 * L,
            y = -2 * L,
            o = orient,
            w = 4 * L,
            l = L,
        );
        assert_eq!(json, expect);
        checked_json(OpName::Query, 0, Output::Query(page));
    }

    #[test]
    fn inspect_returns_only_what_include_names() {
        let symbols = lib();
        let doc = paging_doc(&symbols);
        let all = inspect_all(&doc, &symbols);
        for include in [
            vec![Include::Summary],
            vec![Include::Items],
            vec![Include::Nets],
            vec![Include::Nets, Include::Summary],
        ] {
            let args = inspect_args(include.clone(), None, None, Some(MAX_PAGE_SIZE));
            let r = inspect(&doc, &symbols, &args).unwrap();
            assert_eq!(r.summary.is_some(), include.contains(&Include::Summary));
            assert_eq!(r.items.is_some(), include.contains(&Include::Items));
            assert_eq!(r.nets.is_some(), include.contains(&Include::Nets));
            if let Some(s) = r.summary {
                assert_eq!(Some(s), all.summary);
            }
            if let Some(n) = &r.nets {
                assert_eq!(Some(n), all.nets.as_ref());
            }
            checked_json(OpName::Inspect, 0, Output::Inspect(r));
        }
        checked_json(OpName::Inspect, 0, Output::Inspect(all));
    }

    #[test]
    fn a_part_with_a_foreign_symbol_is_internal_not_a_guess() {
        let symbols = lib();
        let mut spec = part_spec(&symbols, 0, pt(0, 0), Orient::IDENTITY, 1);
        spec.0.symbol = SymbolId(u16::try_from(symbols.len()).unwrap());
        let doc = Doc::from_items(
            [spec],
            std::iter::empty::<Wire>(),
            std::iter::empty::<Label>(),
        )
        .unwrap();
        let err = items(&doc, &symbols, None, None).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Internal);
        // The summary and the nets need no symbol names.
        assert_eq!(summary(&doc).parts, 1);
        assert!(!all_nets(&doc).is_empty());
    }

    #[test]
    fn symbols_project_the_library_in_id_order() {
        let symbols = lib();
        let all = super::symbols(&symbols);
        assert_eq!(all.len(), symbols.len());
        for (info, def) in all.iter().zip(symbols.iter()) {
            assert_eq!(info.name, def.name);
            assert_eq!(info.bounds.rect(), def.bounds);
            let span = |lo: i32, hi: i32| i64::from(hi) - i64::from(lo);
            assert_eq!(
                i64::from(info.width),
                span(def.bounds.min.x, def.bounds.max.x)
            );
            assert_eq!(
                i64::from(info.height),
                span(def.bounds.min.y, def.bounds.max.y)
            );
            assert_eq!(info.pins.len(), def.pins.len());
            for (p, d) in info.pins.iter().zip(def.pins) {
                assert_eq!((p.number.as_str(), Pt::from(p.at)), (d.number, d.at));
            }
            assert_eq!(symbol(&symbols, def.name).as_ref(), Some(info));
            let json = serde_json::to_string(info).unwrap();
            let back: SymbolInfo = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, info);
            assert_eq!(serde_json::to_string(&back).unwrap(), json, "stable bytes");
        }
        assert!(symbol(&symbols, "No-Such-Symbol").is_none());
        let json = serde_json::to_string(&all[0]).unwrap();
        assert!(json.starts_with(r#"{"name":""#), "{json}");
        assert!(json.contains(r#","bounds":{"min":["#), "{json}");
    }
}
