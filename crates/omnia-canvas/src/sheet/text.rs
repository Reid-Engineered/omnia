//! D4/D6/K8/K9/K11/G14/G15: cached horizontal schematic labels with direct glyph emission.
//!
//! Draws part designators, part values, net labels and pin numbers in the sheet's top paint layer:
//! - Typography (K8, Q3): designators and values in Inter SemiBold 17 px (`stage/ink`), stacked on
//!   a shared left edge at a 22 px step (`build-omnia.mjs`); net labels and pin numbers in Inter
//!   Medium 13 px (`stage/ink-secondary`). `Part` currently has a single `value` field (K8's 12 px
//!   secondary value style is reserved for future multi-field parts). Single-pin symbols (such as
//!   ground) intentionally omit pin numbers; multi-pin symbols show every pin number.
//! - Bucketing (K11, D6): logical font sizes are quantised to 0.5 px steps, capped at
//!   [`MAX_FONT_PX`] so the non-evicting glyph atlas stays bounded across K4's zoom range.
//!   Each label caches one [`ShapedLine`] for its current bucket and reshapes only when its
//!   bucket, text, or window scale factor changes. Pin numbers share one cached line per
//!   `(symbol, pin)` across all parts of that symbol.
//! - LOD (K9, D5, T0.6): ordinary text (designators, values, net labels) hides below 6.5 px font
//!   size; pin numbers then stay visible down to K9's 2.5 px pin-number and junction-dot threshold
//!   (31.25% zoom), matching D5's drop order. Visible labels are viewport-culled before enforcing
//!   K9's 700-label per-frame cap, which hides smallest first and breaks equal-size ties by world
//!   geometry `(y, x)` and then lowest id.
//! - Placement: on each `Doc::revision` change (never on steady-state frames), part labels pick a
//!   deterministic clearance-maximising side via local `Doc::query` so labels avoid neighbouring
//!   symbols, wires and rails while keeping warm frames allocation-free (B10).
//! - Direct glyph emission (D4, S5): paints cached glyphs with `Window::paint_glyph` /
//!   `Window::paint_emoji` inside one layer rather than opening a nested layer per label.

use std::{cmp::Reverse, sync::Arc};

use gpui::{
    Bounds, Font, FontWeight, Hsla, Pixels, ShapedLine, SharedString, TextRun, Window, point, px,
};
use omnia_doc::{Doc, GRID_NM, ItemId, LabelId, Part, PartId, Pt, Rect, SHEET_LIMIT_NM, SymbolId};
use omnia_symbols::{SymbolDef, SymbolLibrary};

use crate::{
    camera::Camera,
    sheet::{SheetPaintScope, lod},
    snap_px::snap_logical,
};

/// K8/Q3: part designator and value size at 100% zoom, in logical pixels.
pub const PART_LABEL_PX: f64 = 17.0;
/// K8: net label size at 100% zoom, in logical pixels.
pub const NET_LABEL_PX: f64 = 13.0;
/// K8: pin number size at 100% zoom, in logical pixels.
pub const PIN_NUMBER_PX: f64 = 13.0;
/// `build-omnia.mjs` (`lab`): 22 px line step for 17 px stacked designator and value.
const PART_LINE_STEP_RATIO: f64 = 22.0 / 17.0;
/// K3: 1 logical px at 100% zoom in nanometres (2,540,000 / 20).
const PX_TO_NM_100: f64 = GRID_NM as f64 / 20.0;

/// K9: ordinary text hides below this logical font size.
pub const MIN_TEXT_PX: f64 = 6.5;
/// K11: font sizes quantise to 0.5 logical px buckets.
pub const FONT_BUCKET_STEP_PX: f64 = 0.5;
/// K11 (T3.1): upper bound on rasterised logical font size so the non-evicting glyph atlas stays
/// bounded through K4's 3200% zoom sweep.
pub const MAX_FONT_PX: f64 = 128.0;
/// K9 (T0.6): maximum visible labels painted per frame; past this, smallest hide first.
pub const MAX_VISIBLE_LABELS: usize = 700;

/// Quantised K11 font-size bucket stored as exact half-pixel steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FontBucket {
    pub(super) half_px: u16,
}

impl FontBucket {
    /// Returns the K11 0.5 px bucket for ordinary text, or `None` when hidden by K9 (< 6.5 px).
    pub(super) fn from_logical(logical_px: f64) -> Option<Self> {
        if !logical_px.is_finite() || logical_px < MIN_TEXT_PX {
            return None;
        }
        Self::quantise(logical_px)
    }

    /// Quantises any positive logical font size to the nearest K11 0.5 px step, capped at
    /// [`MAX_FONT_PX`]. Used by pin numbers, which follow K9's 2.5 px pin/junction threshold
    /// rather than the 6.5 px ordinary-text floor (D5).
    pub(super) fn quantise(logical_px: f64) -> Option<Self> {
        if !logical_px.is_finite() || logical_px < FONT_BUCKET_STEP_PX {
            return None;
        }
        let clamped = logical_px.min(MAX_FONT_PX);
        let half_px = (clamped / FONT_BUCKET_STEP_PX).round() as u16;
        debug_assert!(half_px > 0);
        Some(Self { half_px })
    }

    pub(super) fn pixels(self) -> Pixels {
        px(f32::from(self.half_px) * FONT_BUCKET_STEP_PX as f32)
    }
}

/// K9/D5 visibility and K11 bucket for pin numbers at `camera`'s zoom.
///
/// Ordinary text hides below 6.5 px font size first; pin numbers then stay visible down to K9's
/// 2.5 px pin-number and junction-dot threshold (31.25% zoom), hiding together with junctions.
pub(super) fn pin_number_bucket(camera: &Camera) -> Option<FontBucket> {
    lod::junction_diameter(camera)?;
    FontBucket::quantise(PIN_NUMBER_PX * camera.zoom())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum LabelKind {
    Designator = 0,
    Value = 1,
    NetLabel = 2,
    PinNumber = 3,
}

/// Deterministic world-space side chosen for a part's stacked designator + value block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
enum LabelPlacement {
    #[default]
    AboveCenter,
    BelowCenter,
    BelowLeft,
    BelowRight,
    AboveRight,
    AboveLeft,
    RightCenter,
    LeftCenter,
    UpperRight,
    LowerRight,
    UpperLeft,
    LowerLeft,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    kind: LabelKind,
    bucket: FontBucket,
    world: Pt,
    id: u32,
    extra: u16,
    symbol: SymbolId,
}

/// Deterministic keep-order for K9's visible-label cap: largest font size wins, then equal-size
/// ties break by world geometry `(y, x)` first, then lowest document id (per AGENTS.md), with
/// kind and pin index breaking same-point / same-part ties.
fn priority_key(c: &Candidate) -> (Reverse<u16>, i32, i32, u32, u8, u16) {
    (
        Reverse(c.bucket.half_px),
        c.world.y,
        c.world.x,
        c.id,
        c.kind as u8,
        c.extra,
    )
}

#[derive(Debug, Default)]
struct CachedLine {
    /// 0 means empty or invalidated; valid buckets are >= 1 (0.5 px).
    bucket: u16,
    line: Option<ShapedLine>,
}

impl CachedLine {
    fn clear(&mut self) {
        self.bucket = 0;
        self.line = None;
    }
}

#[derive(Debug, Default)]
struct PartLines {
    designator: CachedLine,
    value: CachedLine,
    placement: LabelPlacement,
    placed_world: Option<Rect>,
}

impl PartLines {
    fn clear(&mut self) {
        self.designator.clear();
        self.value.clear();
        self.placement = LabelPlacement::default();
        self.placed_world = None;
    }
}

#[derive(Clone, Copy, Debug)]
struct Buckets {
    part: Option<FontBucket>,
    net: Option<FontBucket>,
    pin: Option<FontBucket>,
}

#[derive(Clone, Copy, Debug)]
struct LineMetrics {
    width: f64,
    ascent: f64,
    descent: f64,
    font_size: f64,
}

#[derive(Clone, Copy, Debug)]
struct PartLabelLayout {
    placement: LabelPlacement,
    is_value: bool,
    has_both: bool,
    block_w: f64,
}

/// Per-frame inputs for [`TextCache::paint`].
pub(super) struct TextFrame<'a> {
    pub(super) doc: &'a Doc,
    pub(super) parts: &'a [ItemId],
    pub(super) labels: &'a [ItemId],
    pub(super) camera: &'a Camera,
    pub(super) viewport: Bounds<Pixels>,
    pub(super) scale_factor: f32,
    pub(super) ink: Hsla,
    pub(super) ink_secondary: Hsla,
}

/// Persistent per-sheet shaped-line cache and reusable candidate buffer (D6, B10).
#[derive(Debug)]
pub(super) struct TextCache {
    library: Arc<SymbolLibrary>,
    pin_offsets: Vec<usize>,
    pin_lines: Vec<CachedLine>,
    part_lines: Vec<PartLines>,
    label_lines: Vec<CachedLine>,
    candidates: Vec<Candidate>,
    query_buf: Vec<ItemId>,
    flatten_buf: String,
    doc_revision: Option<u64>,
    scale_factor_bits: u32,
    semibold: Font,
    medium: Font,
    #[cfg(test)]
    shape_count: u64,
}

impl TextCache {
    pub(super) fn new() -> Self {
        let library = SymbolLibrary::builtin();
        let mut pin_offsets = Vec::with_capacity(library.len());
        let mut total_pins = 0usize;
        for def in library.iter() {
            pin_offsets.push(total_pins);
            total_pins += def.pins.len();
        }
        let mut pin_lines = Vec::with_capacity(total_pins);
        pin_lines.resize_with(total_pins, CachedLine::default);
        let inter = SharedString::new_static("Inter");
        Self {
            library,
            pin_offsets,
            pin_lines,
            part_lines: Vec::new(),
            label_lines: Vec::new(),
            candidates: Vec::with_capacity(MAX_VISIBLE_LABELS * 2),
            query_buf: Vec::new(),
            flatten_buf: String::new(),
            doc_revision: None,
            scale_factor_bits: 0,
            semibold: Font {
                weight: FontWeight::SEMIBOLD,
                ..gpui::font(inter.clone())
            },
            medium: Font {
                weight: FontWeight::MEDIUM,
                ..gpui::font(inter)
            },
            #[cfg(test)]
            shape_count: 0,
        }
    }

    /// Resets cached buckets on scale-factor changes (T7.4), releases cached `ShapedLine`s for
    /// deleted or emptied items on `Doc::revision` changes while preserving slot capacity, and
    /// computes deterministic part-label placements once per revision (B10).
    fn sync(&mut self, doc: &Doc, sf: f32) {
        let sf_bits = sf.to_bits();
        if self.scale_factor_bits != sf_bits {
            self.scale_factor_bits = sf_bits;
            for slot in &mut self.pin_lines {
                slot.bucket = 0;
            }
            for slot in &mut self.part_lines {
                slot.designator.bucket = 0;
                slot.value.bucket = 0;
            }
            for slot in &mut self.label_lines {
                slot.bucket = 0;
            }
        }
        let revision = doc.revision();
        if self.doc_revision != Some(revision) {
            self.doc_revision = Some(revision);
            let max_part = doc
                .parts()
                .map(|(id, _)| id.0 as usize + 1)
                .max()
                .unwrap_or(0);
            if self.part_lines.len() < max_part {
                self.part_lines.resize_with(max_part, PartLines::default);
            }
            for (idx, slot) in self.part_lines.iter_mut().enumerate() {
                match doc.part(PartId(idx as u32)) {
                    None => slot.clear(),
                    Some(part) => {
                        if part.designator.is_empty() {
                            slot.designator.clear();
                        }
                        if part.value.is_empty() {
                            slot.value.clear();
                        }
                        slot.placed_world = None;
                    }
                }
            }

            let max_label = doc
                .labels()
                .map(|(id, _)| id.0 as usize + 1)
                .max()
                .unwrap_or(0);
            if self.label_lines.len() < max_label {
                self.label_lines.resize_with(max_label, CachedLine::default);
            }
            for (idx, slot) in self.label_lines.iter_mut().enumerate() {
                match doc.label(LabelId(idx as u32)) {
                    None => slot.clear(),
                    Some(label) if label.text.is_empty() => slot.clear(),
                    Some(_) => {}
                }
            }

            self.place_part_labels(doc);
        }
    }

    /// Computes a deterministic clearance-maximising [`LabelPlacement`] for each labeled part once
    /// per document revision using local `Doc::query` checks against neighbouring symbols, wires,
    /// labels, and already-placed sibling part labels.
    fn place_part_labels(&mut self, doc: &Doc) {
        let doc_bounds = document_visual_bounds(doc, &self.library);
        let pad_nm = (4.0 * PX_TO_NM_100).round() as i32;

        for idx in 0..self.part_lines.len() {
            let part_id = PartId(idx as u32);
            let Some(part) = doc.part(part_id) else {
                continue;
            };
            if part.symbol.index() >= self.library.len()
                || (part.designator.is_empty() && part.value.is_empty())
            {
                self.part_lines[idx].placed_world = None;
                continue;
            }
            let def = self.library.get(part.symbol);
            let has_both = !part.designator.is_empty() && !part.value.is_empty();
            let max_chars = flattened_char_len(&part.designator)
                .max(flattened_char_len(&part.value))
                .max(1);
            let block_w_nm = ((max_chars as f64 * 10.5) * PX_TO_NM_100).round() as i32;
            let block_h_nm = (if has_both { 40.0 } else { 18.0 } * PX_TO_NM_100).round() as i32;

            let candidates = candidate_placements(part, def);
            let mut best = (
                u32::MAX,
                0usize,
                candidates[0],
                Rect {
                    min: part.at,
                    max: part.at,
                },
            );

            for (order, &placement) in candidates.iter().enumerate() {
                let rect = placement_world_rect(part, def, placement, block_w_nm, block_h_nm);
                let query_rect = expand_rect(rect, pad_nm);
                doc.query(query_rect, &mut self.query_buf);

                let mut score = 0u32;
                for &item in &self.query_buf {
                    match item {
                        ItemId::Part(other_id) if other_id != part_id => {
                            if let Some(other) = doc.part(other_id)
                                && other.symbol.index() < self.library.len()
                            {
                                let other_def = self.library.get(other.symbol);
                                let body = part_visual_body(other, other_def);
                                if rects_overlap(query_rect, body) {
                                    score = score.saturating_add(100);
                                }
                            }
                            if let Some(placed) = self
                                .part_lines
                                .get(other_id.0 as usize)
                                .and_then(|s| s.placed_world)
                                && rects_overlap(query_rect, placed)
                            {
                                score = score.saturating_add(80);
                            }
                        }
                        ItemId::Part(_) => {}
                        ItemId::Wire(wire_id) => {
                            if let Some(wire) = doc.wire(wire_id)
                                && segment_hits_rect(wire.a, wire.b, query_rect)
                            {
                                score = score.saturating_add(50);
                            }
                        }
                        ItemId::Label(label_id) => {
                            if let Some(label) = doc.label(label_id) {
                                let lw = (flattened_char_len(&label.text).max(1) as f64
                                    * 8.0
                                    * PX_TO_NM_100)
                                    .round() as i32;
                                let lh = (14.0 * PX_TO_NM_100).round() as i32;
                                let lrect = Rect {
                                    min: Pt {
                                        x: label.at.x,
                                        y: label.at.y.saturating_sub(lh),
                                    },
                                    max: Pt {
                                        x: label.at.x.saturating_add(lw),
                                        y: label.at.y,
                                    },
                                };
                                if rects_overlap(query_rect, lrect) {
                                    score = score.saturating_add(40);
                                }
                            }
                        }
                    }
                }

                if let Some(bounds) = doc_bounds {
                    if rect.min.x < bounds.min.x {
                        score = score.saturating_add(10);
                    }
                    if rect.max.x > bounds.max.x {
                        score = score.saturating_add(10);
                    }
                    if rect.min.y < bounds.min.y {
                        score = score.saturating_add(10);
                    }
                    if rect.max.y > bounds.max.y {
                        score = score.saturating_add(10);
                    }
                }

                if (score, order) < (best.0, best.1) {
                    best = (score, order, placement, rect);
                    if score == 0 {
                        break;
                    }
                }
            }

            self.part_lines[idx].placement = best.2;
            self.part_lines[idx].placed_world = Some(best.3);
        }
    }

    /// Collects viewport-visible label candidates from the already-culled `Doc::query` slice (D6),
    /// enforces K9's 700-label cap (hiding smallest first, ties by geometry then id), shapes only
    /// cache misses, and emits glyphs directly inside one paint layer (D4, S5).
    pub(super) fn paint(
        &mut self,
        frame: &TextFrame<'_>,
        window: &mut Window,
        scope: &impl SheetPaintScope,
    ) {
        self.sync(frame.doc, frame.scale_factor);

        let buckets = Buckets {
            part: FontBucket::from_logical(PART_LABEL_PX * frame.camera.zoom()),
            net: FontBucket::from_logical(NET_LABEL_PX * frame.camera.zoom()),
            pin: pin_number_bucket(frame.camera),
        };
        if buckets.part.is_none() && buckets.net.is_none() && buckets.pin.is_none() {
            return;
        }

        self.collect_candidates(frame, buckets);
        if self.candidates.is_empty() {
            return;
        }

        // Shape any cache misses before opening the paint layer.
        self.shape_missing(frame, window, scope);

        // Paint all kept labels in one sheet text layer via direct glyph emission (D4, G5, S5).
        scope.gpui(|| {
            window.paint_layer(|window| {
                scope.omnia(|| {
                    self.emit_glyphs(frame, window, scope);
                });
            });
        });
    }

    fn collect_candidates(&mut self, frame: &TextFrame<'_>, buckets: Buckets) {
        self.candidates.clear();

        if buckets.part.is_some() || buckets.pin.is_some() {
            for &item in frame.parts {
                let ItemId::Part(part_id) = item else {
                    continue;
                };
                let Some(part) = frame.doc.part(part_id) else {
                    continue;
                };
                if part.symbol.index() >= self.library.len() {
                    continue;
                }
                let def = self.library.get(part.symbol);
                let has_desig = !part.designator.is_empty();
                let has_value = !part.value.is_empty();
                if let Some(bucket) = buckets.part
                    && (has_desig || has_value)
                {
                    let slots = self.part_lines.get(part_id.0 as usize);
                    let placement = slots.map_or(LabelPlacement::default(), |s| s.placement);
                    let desig_m = has_desig.then(|| {
                        line_metrics(slots.map(|s| &s.designator), &part.designator, bucket)
                    });
                    let value_m = has_value
                        .then(|| line_metrics(slots.map(|s| &s.value), &part.value, bucket));
                    let block_w = desig_m
                        .map_or(0.0, |m| m.width)
                        .max(value_m.map_or(0.0, |m| m.width));
                    let has_both = has_desig && has_value;
                    let pair_flag = u16::from(has_both);

                    if let Some(m) = desig_m {
                        let layout = PartLabelLayout {
                            placement,
                            is_value: false,
                            has_both,
                            block_w,
                        };
                        let (left_x, baseline_y) =
                            part_label_origin(part, def, layout, m, frame.camera);
                        if label_intersects_viewport(
                            left_x,
                            baseline_y,
                            m,
                            frame.scale_factor,
                            frame.viewport,
                        ) {
                            push_candidate(
                                &mut self.candidates,
                                Candidate {
                                    kind: LabelKind::Designator,
                                    bucket,
                                    world: part.at,
                                    id: part_id.0,
                                    extra: pair_flag,
                                    symbol: part.symbol,
                                },
                            );
                        }
                    }
                    if let Some(m) = value_m {
                        let layout = PartLabelLayout {
                            placement,
                            is_value: true,
                            has_both,
                            block_w,
                        };
                        let (left_x, baseline_y) =
                            part_label_origin(part, def, layout, m, frame.camera);
                        if label_intersects_viewport(
                            left_x,
                            baseline_y,
                            m,
                            frame.scale_factor,
                            frame.viewport,
                        ) {
                            push_candidate(
                                &mut self.candidates,
                                Candidate {
                                    kind: LabelKind::Value,
                                    bucket,
                                    world: part.at,
                                    id: part_id.0,
                                    extra: pair_flag,
                                    symbol: part.symbol,
                                },
                            );
                        }
                    }
                }
                if let Some(bucket) = buckets.pin
                    && def.pins.len() > 1
                {
                    let pin_base = self.pin_offsets[part.symbol.index()];
                    for (pin_idx, pin) in def.pins.iter().enumerate() {
                        if pin.number.is_empty() {
                            continue;
                        }
                        let m = line_metrics(
                            self.pin_lines.get(pin_base + pin_idx),
                            pin.number,
                            bucket,
                        );
                        let (left_x, baseline_y) =
                            pin_number_origin(part, def, pin.at, m, frame.camera);
                        if !label_intersects_viewport(
                            left_x,
                            baseline_y,
                            m,
                            frame.scale_factor,
                            frame.viewport,
                        ) {
                            continue;
                        }
                        let pin_world = part.at + part.orient.apply(pin.at);
                        push_candidate(
                            &mut self.candidates,
                            Candidate {
                                kind: LabelKind::PinNumber,
                                bucket,
                                world: pin_world,
                                id: part_id.0,
                                extra: pin_idx as u16,
                                symbol: part.symbol,
                            },
                        );
                    }
                }
            }
        }

        if let Some(bucket) = buckets.net {
            let zoom = frame.camera.zoom();
            for &item in frame.labels {
                let ItemId::Label(label_id) = item else {
                    continue;
                };
                let Some(label) = frame.doc.label(label_id) else {
                    continue;
                };
                if label.text.is_empty() {
                    continue;
                }
                let m = line_metrics(
                    self.label_lines.get(label_id.0 as usize),
                    &label.text,
                    bucket,
                );
                let anchor = frame.camera.world_to_screen(label.at);
                if !label_intersects_viewport(
                    anchor.x + 2.0 * zoom,
                    anchor.y,
                    m,
                    frame.scale_factor,
                    frame.viewport,
                ) {
                    continue;
                }
                push_candidate(
                    &mut self.candidates,
                    Candidate {
                        kind: LabelKind::NetLabel,
                        bucket,
                        world: label.at,
                        id: label_id.0,
                        extra: 0,
                        symbol: SymbolId(0),
                    },
                );
            }
        }

        if self.candidates.len() > MAX_VISIBLE_LABELS {
            self.candidates
                .select_nth_unstable_by_key(MAX_VISIBLE_LABELS, priority_key);
            self.candidates.truncate(MAX_VISIBLE_LABELS);
        }
        // Deterministic paint order regardless of select_nth_unstable partitioning.
        self.candidates
            .sort_unstable_by_key(|c| (c.kind, c.id, c.extra));
    }

    fn shape_missing(
        &mut self,
        frame: &TextFrame<'_>,
        window: &Window,
        scope: &impl SheetPaintScope,
    ) {
        for index in 0..self.candidates.len() {
            let c = self.candidates[index];
            match c.kind {
                LabelKind::Designator | LabelKind::Value => {
                    let Some(part) = frame.doc.part(PartId(c.id)) else {
                        continue;
                    };
                    let Some(slots) = self.part_lines.get_mut(c.id as usize) else {
                        continue;
                    };
                    let (arc_text, slot) = if c.kind == LabelKind::Designator {
                        (&part.designator, &mut slots.designator)
                    } else {
                        (&part.value, &mut slots.value)
                    };
                    if slot.bucket == c.bucket.half_px
                        && slot
                            .line
                            .as_ref()
                            .is_some_and(|l| matches_flattened(l.text.as_str(), arc_text))
                    {
                        continue;
                    }
                    let shaped = shape_arc_line(
                        arc_text,
                        &mut self.flatten_buf,
                        c.bucket,
                        &self.semibold,
                        frame.ink,
                        window,
                        scope,
                    );
                    slot.bucket = c.bucket.half_px;
                    slot.line = Some(shaped);
                    #[cfg(test)]
                    {
                        self.shape_count += 1;
                    }
                }
                LabelKind::NetLabel => {
                    let Some(label) = frame.doc.label(LabelId(c.id)) else {
                        continue;
                    };
                    let Some(slot) = self.label_lines.get_mut(c.id as usize) else {
                        continue;
                    };
                    if slot.bucket == c.bucket.half_px
                        && slot
                            .line
                            .as_ref()
                            .is_some_and(|l| matches_flattened(l.text.as_str(), &label.text))
                    {
                        continue;
                    }
                    let shaped = shape_arc_line(
                        &label.text,
                        &mut self.flatten_buf,
                        c.bucket,
                        &self.medium,
                        frame.ink_secondary,
                        window,
                        scope,
                    );
                    slot.bucket = c.bucket.half_px;
                    slot.line = Some(shaped);
                    #[cfg(test)]
                    {
                        self.shape_count += 1;
                    }
                }
                LabelKind::PinNumber => {
                    let pin_idx = usize::from(c.extra);
                    let slot_idx = self.pin_offsets[c.symbol.index()] + pin_idx;
                    let slot = &mut self.pin_lines[slot_idx];
                    if slot.bucket == c.bucket.half_px && slot.line.is_some() {
                        continue;
                    }
                    let text = self.library.get(c.symbol).pins[pin_idx].number;
                    let shaped = shape_static_line(
                        text,
                        &mut self.flatten_buf,
                        c.bucket,
                        &self.medium,
                        frame.ink_secondary,
                        window,
                        scope,
                    );
                    slot.bucket = c.bucket.half_px;
                    slot.line = Some(shaped);
                    #[cfg(test)]
                    {
                        self.shape_count += 1;
                    }
                }
            }
        }
    }

    fn emit_glyphs(
        &self,
        frame: &TextFrame<'_>,
        window: &mut Window,
        scope: &impl SheetPaintScope,
    ) {
        let zoom = frame.camera.zoom();
        let v_left = f32::from(frame.viewport.left());
        let v_right = f32::from(frame.viewport.right());
        let v_top = f32::from(frame.viewport.top());
        let v_bottom = f32::from(frame.viewport.bottom());

        for &c in &self.candidates {
            let (line, color, left_x, baseline_y) = match c.kind {
                LabelKind::Designator | LabelKind::Value => {
                    let Some(part) = frame.doc.part(PartId(c.id)) else {
                        continue;
                    };
                    let Some(slots) = self.part_lines.get(c.id as usize) else {
                        continue;
                    };
                    let slot = if c.kind == LabelKind::Designator {
                        &slots.designator
                    } else {
                        &slots.value
                    };
                    let Some(line) = slot.line.as_ref() else {
                        continue;
                    };
                    let def = self.library.get(part.symbol);
                    let desig_w = if part.designator.is_empty() {
                        0.0
                    } else {
                        line_metrics(Some(&slots.designator), &part.designator, c.bucket).width
                    };
                    let value_w = if part.value.is_empty() {
                        0.0
                    } else {
                        line_metrics(Some(&slots.value), &part.value, c.bucket).width
                    };
                    let block_w = desig_w.max(value_w);
                    let m = metrics_from_shaped(line);
                    let layout = PartLabelLayout {
                        placement: slots.placement,
                        is_value: c.kind == LabelKind::Value,
                        has_both: c.extra != 0,
                        block_w,
                    };
                    let (left_x, baseline_y) =
                        part_label_origin(part, def, layout, m, frame.camera);
                    (line, frame.ink, left_x, baseline_y)
                }
                LabelKind::NetLabel => {
                    let Some(label) = frame.doc.label(LabelId(c.id)) else {
                        continue;
                    };
                    let Some(line) = self
                        .label_lines
                        .get(c.id as usize)
                        .and_then(|s| s.line.as_ref())
                    else {
                        continue;
                    };
                    let anchor = frame.camera.world_to_screen(label.at);
                    (line, frame.ink_secondary, anchor.x + 2.0 * zoom, anchor.y)
                }
                LabelKind::PinNumber => {
                    let Some(part) = frame.doc.part(PartId(c.id)) else {
                        continue;
                    };
                    let pin_idx = usize::from(c.extra);
                    let slot_idx = self.pin_offsets[c.symbol.index()] + pin_idx;
                    let Some(line) = self.pin_lines[slot_idx].line.as_ref() else {
                        continue;
                    };
                    let def = self.library.get(part.symbol);
                    let (left_x, baseline_y) = pin_number_origin(
                        part,
                        def,
                        def.pins[pin_idx].at,
                        metrics_from_shaped(line),
                        frame.camera,
                    );
                    (line, frame.ink_secondary, left_x, baseline_y)
                }
            };

            // Snap each label origin to whole device pixels so glyphs hit deterministic subpixel
            // X variants across pans and repeated zoom sweeps.
            let origin_x = snap_logical(left_x, frame.scale_factor) as f32;
            let baseline_y = snap_logical(baseline_y, frame.scale_factor) as f32;
            let top_y = baseline_y - f32::from(line.ascent);
            let bottom_y = baseline_y + f32::from(line.descent).max(0.0);
            let right_x = origin_x + f32::from(line.width);
            if right_x < v_left || origin_x > v_right || bottom_y < v_top || top_y > v_bottom {
                continue;
            }

            for run in &line.runs {
                for glyph in &run.glyphs {
                    let pos = point(
                        px(origin_x) + glyph.position.x,
                        px(baseline_y) + glyph.position.y,
                    );
                    scope.gpui(|| {
                        let result = if glyph.is_emoji {
                            window.paint_emoji(pos, run.font_id, glyph.id, line.font_size)
                        } else {
                            window.paint_glyph(pos, run.font_id, glyph.id, line.font_size, color)
                        };
                        if let Err(error) = result {
                            eprintln!("sheet glyph: {error}");
                        }
                    });
                }
            }
        }
    }
}

/// Keeps `candidates` bounded to `2 * MAX_VISIBLE_LABELS` with in-place selection so it never
/// reallocates regardless of sheet size (B10, K9).
fn push_candidate(candidates: &mut Vec<Candidate>, candidate: Candidate) {
    if candidates.len() == MAX_VISIBLE_LABELS * 2 {
        candidates.select_nth_unstable_by_key(MAX_VISIBLE_LABELS, priority_key);
        candidates.truncate(MAX_VISIBLE_LABELS);
    }
    candidates.push(candidate);
}

fn has_line_break(text: &str) -> bool {
    text.bytes().any(|b| b == b'\n' || b == b'\r')
}

/// Flattens line breaks into visible escapes (`\n` -> `\\n`, `\r` -> `\\r`) so single-line
/// shaping never panics and distinct multiline net names remain visually distinct.
fn flatten_into(text: &str, out: &mut String) {
    out.clear();
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
}

/// Allocation-free check whether `cached` equals `flatten_into(raw)`.
fn matches_flattened(cached: &str, raw: &str) -> bool {
    if !has_line_break(raw) {
        return cached == raw;
    }
    let mut rest = cached.as_bytes();
    for &b in raw.as_bytes() {
        let expected: &[u8] = match b {
            b'\n' => b"\\n",
            b'\r' => b"\\r",
            _ => {
                let Some((&head, tail)) = rest.split_first() else {
                    return false;
                };
                if head != b {
                    return false;
                }
                rest = tail;
                continue;
            }
        };
        let Some(tail) = rest.strip_prefix(expected) else {
            return false;
        };
        rest = tail;
    }
    rest.is_empty()
}

fn flattened_char_len(text: &str) -> usize {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' { 2 } else { 1 })
        .sum()
}

fn metrics_from_shaped(line: &ShapedLine) -> LineMetrics {
    LineMetrics {
        width: f64::from(line.width),
        ascent: f64::from(line.ascent),
        descent: f64::from(line.descent).max(0.0),
        font_size: f64::from(line.font_size),
    }
}

fn line_metrics(slot: Option<&CachedLine>, raw_text: &str, bucket: FontBucket) -> LineMetrics {
    if let Some(slot) = slot
        && slot.bucket == bucket.half_px
        && let Some(line) = slot.line.as_ref()
    {
        return metrics_from_shaped(line);
    }
    let font_size = f64::from(bucket.pixels());
    LineMetrics {
        width: flattened_char_len(raw_text) as f64 * font_size * 0.62,
        ascent: font_size * 0.9,
        descent: font_size * 0.25,
        font_size,
    }
}

fn label_intersects_viewport(
    left_x: f64,
    baseline_y: f64,
    m: LineMetrics,
    sf: f32,
    viewport: Bounds<Pixels>,
) -> bool {
    let origin_x = snap_logical(left_x, sf) as f32;
    let baseline_y = snap_logical(baseline_y, sf) as f32;
    let top_y = baseline_y - m.ascent as f32;
    let bottom_y = baseline_y + m.descent as f32;
    let right_x = origin_x + m.width as f32;
    !(right_x < f32::from(viewport.left())
        || origin_x > f32::from(viewport.right())
        || bottom_y < f32::from(viewport.top())
        || top_y > f32::from(viewport.bottom()))
}

fn shape_arc_line(
    full: &Arc<str>,
    flatten_buf: &mut String,
    bucket: FontBucket,
    font: &Font,
    color: Hsla,
    window: &Window,
    scope: &impl SheetPaintScope,
) -> ShapedLine {
    let mut out = None;
    let multiline = has_line_break(full);
    if multiline {
        scope.gpui(|| flatten_into(full, flatten_buf));
    }
    scope.gpui(|| {
        let shared = if multiline {
            SharedString::new(flatten_buf.as_str())
        } else {
            SharedString::from(full.clone())
        };
        let run = TextRun {
            len: shared.len(),
            font: font.clone(),
            color,
            ..Default::default()
        };
        out = Some(
            window
                .text_system()
                .shape_line(shared, bucket.pixels(), &[run], None),
        );
    });
    out.expect("scope.gpui runs closure")
}

fn shape_static_line(
    text: &'static str,
    flatten_buf: &mut String,
    bucket: FontBucket,
    font: &Font,
    color: Hsla,
    window: &Window,
    scope: &impl SheetPaintScope,
) -> ShapedLine {
    let mut out = None;
    let multiline = has_line_break(text);
    if multiline {
        scope.gpui(|| flatten_into(text, flatten_buf));
    }
    scope.gpui(|| {
        let shared = if multiline {
            SharedString::new(flatten_buf.as_str())
        } else {
            SharedString::new_static(text)
        };
        let run = TextRun {
            len: shared.len(),
            font: font.clone(),
            color,
            ..Default::default()
        };
        out = Some(
            window
                .text_system()
                .shape_line(shared, bucket.pixels(), &[run], None),
        );
    });
    out.expect("scope.gpui runs closure")
}

fn oriented_part_bounds_nm(part: &Part, def: &SymbolDef) -> (i32, i32, i32, i32) {
    let c0 = part.orient.apply(def.bounds.min);
    let c1 = part.orient.apply(def.bounds.max);
    (
        c0.x.min(c1.x),
        c0.x.max(c1.x),
        c0.y.min(c1.y),
        c0.y.max(c1.y),
    )
}

/// Tight visual body box in world nm (excludes the empty square padding on 2-pin symbols).
fn part_visual_body(part: &Part, def: &SymbolDef) -> Rect {
    let (mut min_x, mut max_x, mut min_y, mut max_y) = oriented_part_bounds_nm(part, def);
    if def.pins.len() == 2 {
        let is_vertical = part.orient.apply(Pt { x: 1, y: 0 }).x == 0;
        if is_vertical {
            let cx = min_x.midpoint(max_x);
            let half = (max_x - min_x) / 4;
            min_x = cx - half;
            max_x = cx + half;
        } else {
            let cy = min_y.midpoint(max_y);
            let half = (max_y - min_y) / 4;
            min_y = cy - half;
            max_y = cy + half;
        }
    }
    Rect {
        min: Pt {
            x: part.at.x.saturating_add(min_x),
            y: part.at.y.saturating_add(min_y),
        },
        max: Pt {
            x: part.at.x.saturating_add(max_x),
            y: part.at.y.saturating_add(max_y),
        },
    }
}

fn document_visual_bounds(doc: &Doc, library: &SymbolLibrary) -> Option<Rect> {
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    let mut any = false;

    for (_, part) in doc.parts() {
        if part.symbol.index() >= library.len() {
            continue;
        }
        let body = part_visual_body(part, library.get(part.symbol));
        min_x = min_x.min(body.min.x);
        min_y = min_y.min(body.min.y);
        max_x = max_x.max(body.max.x);
        max_y = max_y.max(body.max.y);
        any = true;
    }
    for (_, wire) in doc.wires() {
        min_x = min_x.min(wire.a.x).min(wire.b.x);
        min_y = min_y.min(wire.a.y).min(wire.b.y);
        max_x = max_x.max(wire.a.x).max(wire.b.x);
        max_y = max_y.max(wire.a.y).max(wire.b.y);
        any = true;
    }
    for (_, label) in doc.labels() {
        min_x = min_x.min(label.at.x);
        min_y = min_y.min(label.at.y);
        max_x = max_x.max(label.at.x);
        max_y = max_y.max(label.at.y);
        any = true;
    }

    any.then_some(Rect {
        min: Pt { x: min_x, y: min_y },
        max: Pt { x: max_x, y: max_y },
    })
}

fn expand_rect(r: Rect, pad: i32) -> Rect {
    Rect {
        min: Pt {
            x: r.min.x.saturating_sub(pad).max(-SHEET_LIMIT_NM),
            y: r.min.y.saturating_sub(pad).max(-SHEET_LIMIT_NM),
        },
        max: Pt {
            x: r.max.x.saturating_add(pad).min(SHEET_LIMIT_NM),
            y: r.max.y.saturating_add(pad).min(SHEET_LIMIT_NM),
        },
    }
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

fn segment_hits_rect(a: Pt, b: Pt, r: Rect) -> bool {
    let seg_min_x = a.x.min(b.x);
    let seg_max_x = a.x.max(b.x);
    let seg_min_y = a.y.min(b.y);
    let seg_max_y = a.y.max(b.y);
    seg_min_x <= r.max.x && seg_max_x >= r.min.x && seg_min_y <= r.max.y && seg_max_y >= r.min.y
}

fn candidate_placements(part: &Part, def: &SymbolDef) -> &'static [LabelPlacement] {
    let is_vertical = part.orient.apply(Pt { x: 1, y: 0 }).x == 0;
    if is_vertical {
        return &[
            LabelPlacement::RightCenter,
            LabelPlacement::LeftCenter,
            LabelPlacement::AboveCenter,
            LabelPlacement::BelowCenter,
        ];
    }
    let (_, _, min_y, max_y) = oriented_part_bounds_nm(part, def);
    let center_y = i64::from(min_y) + (i64::from(max_y) - i64::from(min_y)) / 2;
    let top_threshold = (i64::from(max_y) - i64::from(min_y)) * 2 / 5;
    let has_top_pin = def.pins.iter().any(|pin| {
        let py = i64::from(part.orient.apply(pin.at).y);
        py - center_y < -top_threshold
    });
    if has_top_pin {
        &[
            LabelPlacement::UpperRight,
            LabelPlacement::LowerRight,
            LabelPlacement::UpperLeft,
            LabelPlacement::LowerLeft,
            LabelPlacement::AboveCenter,
            LabelPlacement::BelowCenter,
        ]
    } else {
        &[
            LabelPlacement::AboveCenter,
            LabelPlacement::BelowCenter,
            LabelPlacement::BelowLeft,
            LabelPlacement::BelowRight,
            LabelPlacement::AboveRight,
            LabelPlacement::AboveLeft,
            LabelPlacement::RightCenter,
            LabelPlacement::LeftCenter,
        ]
    }
}

/// Computes `(left_x, top_y)` for a part's label block in any uniform scale unit (screen px or
/// world nm), given the part's oriented box `(min_x, max_x, min_y, max_y)`, block size `(w, h)`,
/// and margin gap.
fn block_origin_in_units(
    box_rect: (f64, f64, f64, f64),
    placement: LabelPlacement,
    block_size: (f64, f64),
    gap: f64,
) -> (f64, f64) {
    let (box_min_x, box_max_x, box_min_y, box_max_y) = box_rect;
    let (block_w, block_h) = block_size;
    let cx = (box_min_x + box_max_x) * 0.5;
    let cy = (box_min_y + box_max_y) * 0.5;
    let box_w = box_max_x - box_min_x;
    let box_h = box_max_y - box_min_y;
    let side_gap = 0.25 * box_w + gap;
    let vert_gap = 0.25 * box_h + gap;
    let below_shift = 0.25 * box_w;
    let above_shift = 0.42 * box_w;

    match placement {
        LabelPlacement::AboveCenter => (cx - block_w * 0.5, cy - vert_gap - block_h),
        LabelPlacement::BelowCenter => (cx - block_w * 0.5, cy + vert_gap),
        LabelPlacement::BelowLeft => (cx - below_shift - block_w, cy + vert_gap),
        LabelPlacement::BelowRight => (cx + below_shift, cy + vert_gap),
        LabelPlacement::AboveRight => (cx + above_shift, cy - vert_gap - block_h),
        LabelPlacement::AboveLeft => (cx - above_shift - block_w, cy - vert_gap - block_h),
        LabelPlacement::RightCenter => (cx + side_gap, cy - block_h * 0.5),
        LabelPlacement::LeftCenter => (cx - side_gap - block_w, cy - block_h * 0.5),
        LabelPlacement::UpperRight => (cx + 0.25 * box_w, box_min_y),
        LabelPlacement::LowerRight => (cx + 0.25 * box_w, box_max_y - block_h),
        LabelPlacement::UpperLeft => (cx - 0.25 * box_w - block_w, box_min_y),
        LabelPlacement::LowerLeft => (cx - 0.25 * box_w - block_w, box_max_y - block_h),
    }
}

fn placement_world_rect(
    part: &Part,
    def: &SymbolDef,
    placement: LabelPlacement,
    block_w_nm: i32,
    block_h_nm: i32,
) -> Rect {
    let (min_x, max_x, min_y, max_y) = oriented_part_bounds_nm(part, def);
    let gap_nm = 4.0 * PX_TO_NM_100;
    let (lx, ty) = block_origin_in_units(
        (
            f64::from(part.at.x) + f64::from(min_x),
            f64::from(part.at.x) + f64::from(max_x),
            f64::from(part.at.y) + f64::from(min_y),
            f64::from(part.at.y) + f64::from(max_y),
        ),
        placement,
        (f64::from(block_w_nm), f64::from(block_h_nm)),
        gap_nm,
    );
    let left = lx.round() as i32;
    let top = ty.round() as i32;
    Rect {
        min: Pt { x: left, y: top },
        max: Pt {
            x: left.saturating_add(block_w_nm),
            y: top.saturating_add(block_h_nm),
        },
    }
}

/// Computes horizontal screen `(left_x, baseline_y)` for a part's designator or value (D6).
/// Stacked designator + value lines share one `left_x` (`build-omnia.mjs:170`).
fn part_label_origin(
    part: &Part,
    def: &SymbolDef,
    layout: PartLabelLayout,
    m: LineMetrics,
    camera: &Camera,
) -> (f64, f64) {
    let zoom = camera.zoom();
    let scale = camera.scale();
    let anchor = camera.world_to_screen(part.at);
    let (min_x, max_x, min_y, max_y) = oriented_part_bounds_nm(part, def);

    let line_h = m.ascent + m.descent;
    let line_step = m.font_size * PART_LINE_STEP_RATIO;
    let stack_h = if layout.has_both {
        line_step + line_h
    } else {
        line_h
    };
    let row_offset = if layout.is_value && layout.has_both {
        line_step
    } else {
        0.0
    };

    let (left_x, block_top_y) = block_origin_in_units(
        (
            anchor.x + f64::from(min_x) * scale,
            anchor.x + f64::from(max_x) * scale,
            anchor.y + f64::from(min_y) * scale,
            anchor.y + f64::from(max_y) * scale,
        ),
        layout.placement,
        (layout.block_w, stack_h),
        4.0 * zoom,
    );
    (left_x, block_top_y + row_offset + m.ascent)
}

/// Computes horizontal screen `(left_x, baseline_y)` for one pin number, nudged inward along its
/// lead so it stays clear of the lead and connected wire.
fn pin_number_origin(
    part: &Part,
    def: &SymbolDef,
    local_pin: Pt,
    m: LineMetrics,
    camera: &Camera,
) -> (f64, f64) {
    let zoom = camera.zoom();
    let pin_local = part.orient.apply(local_pin);
    let mid_local = part.orient.apply(Pt {
        x: def.bounds.min.x.midpoint(def.bounds.max.x),
        y: def.bounds.min.y.midpoint(def.bounds.max.y),
    });
    let inward_x = i64::from(mid_local.x) - i64::from(pin_local.x);
    let inward_y = i64::from(mid_local.y) - i64::from(pin_local.y);
    let pin_screen = camera.world_to_screen(part.at + pin_local);
    let offset = 4.0 * zoom;
    let vgap = 3.0 * zoom;

    if inward_x.abs() >= inward_y.abs() {
        let baseline_y = pin_screen.y - vgap;
        let left_x = if inward_x < 0 {
            pin_screen.x - offset - m.width
        } else {
            pin_screen.x + offset
        };
        (left_x, baseline_y)
    } else {
        let left_x = pin_screen.x + offset;
        let baseline_y = if inward_y > 0 {
            pin_screen.y + vgap + m.ascent
        } else {
            pin_screen.y - vgap
        };
        (left_x, baseline_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{MAX_ZOOM, MIN_ZOOM};
    use gpui::{Bounds, size};
    use omnia_doc::{Command, Label, Orient, Part};

    fn viewport() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0)))
    }

    #[test]
    fn k9_k11_bucket_thresholds_and_d5_drop_order() {
        assert_eq!(FontBucket::from_logical(6.49), None);
        assert_eq!(FontBucket::from_logical(0.0), None);
        assert_eq!(FontBucket::from_logical(-5.0), None);
        assert_eq!(FontBucket::from_logical(f64::NAN), None);
        assert_eq!(FontBucket::from_logical(6.5).unwrap().pixels(), px(6.5));
        assert_eq!(FontBucket::from_logical(6.74).unwrap().pixels(), px(6.5));
        assert_eq!(FontBucket::from_logical(6.75).unwrap().pixels(), px(7.0));
        assert_eq!(FontBucket::from_logical(17.0).unwrap().pixels(), px(17.0));
        assert_eq!(FontBucket::from_logical(13.0).unwrap().pixels(), px(13.0));
        // Upper cap bounds the atlas at K4's 3200% zoom.
        assert_eq!(
            FontBucket::from_logical(PART_LABEL_PX * MAX_ZOOM)
                .unwrap()
                .pixels(),
            px(MAX_FONT_PX as f32)
        );

        // D5/K9 drop order: net labels (< 50%), then designators/values (< 38.24%), then pin
        // numbers together with junction dots (< 31.25% = 2.5 px junction threshold).
        let cam_100 = Camera::new(Pt::ZERO, 1.0, viewport());
        assert_eq!(pin_number_bucket(&cam_100).unwrap().pixels(), px(13.0));

        let cam_50 = Camera::new(Pt::ZERO, 0.5, viewport());
        assert_eq!(
            FontBucket::from_logical(NET_LABEL_PX * 0.5)
                .unwrap()
                .pixels(),
            px(6.5)
        );
        assert_eq!(pin_number_bucket(&cam_50).unwrap().pixels(), px(6.5));

        // 0.45x: 13 px net labels hide (< 6.5 px), 17 px part labels and pin numbers remain.
        let cam_45 = Camera::new(Pt::ZERO, 0.45, viewport());
        assert_eq!(FontBucket::from_logical(NET_LABEL_PX * 0.45), None);
        assert!(FontBucket::from_logical(PART_LABEL_PX * 0.45).is_some());
        assert_eq!(pin_number_bucket(&cam_45).unwrap().pixels(), px(6.0));

        // 0.35x: all ordinary text hides (< 6.5 px), while pin numbers remain visible above K9's
        // 2.5 px pin/junction threshold (D5 drop order).
        let cam_35 = Camera::new(Pt::ZERO, 0.35, viewport());
        assert_eq!(FontBucket::from_logical(PART_LABEL_PX * 0.35), None);
        assert_eq!(FontBucket::from_logical(NET_LABEL_PX * 0.35), None);
        assert_eq!(pin_number_bucket(&cam_35).unwrap().pixels(), px(4.5));

        // Exact 31.25% boundary: junction diameter = 2.5 px -> pin numbers visible.
        let cam_boundary = Camera::new(Pt::ZERO, 0.3125, viewport());
        assert_eq!(pin_number_bucket(&cam_boundary).unwrap().pixels(), px(4.0));

        // 0.30x (< 31.25%): junction diameter = 2.4 px < 2.5 px -> pin numbers hidden too.
        let cam_30 = Camera::new(Pt::ZERO, 0.30, viewport());
        assert_eq!(FontBucket::from_logical(PART_LABEL_PX * 0.30), None);
        assert_eq!(pin_number_bucket(&cam_30), None);

        let cam_min = Camera::new(Pt::ZERO, MIN_ZOOM, viewport());
        assert_eq!(pin_number_bucket(&cam_min), None);
    }

    #[test]
    fn k9_visible_label_cap_culls_offscreen_first_and_breaks_ties_by_geometry_then_id() {
        let library = SymbolLibrary::builtin();
        let resistor = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let mut doc = Doc::new();
        // 400 on-screen parts -> 400 designators (17 px) + 400 values (17 px) + 800 pins (13 px),
        // plus 200 on-screen net labels (13 px) = 1,800 visible candidates.
        for i in 0..400 {
            let at = Pt {
                x: (i % 20 - 10) * 6 * GRID_NM,
                y: (i / 20 - 10) * 6 * GRID_NM,
            };
            doc.apply(Command::PlacePart {
                part: Part {
                    symbol: resistor.id,
                    at,
                    orient: Orient::IDENTITY,
                    designator: format!("R{i}").into(),
                    value: format!("{i}k").into(),
                },
                local_pins: resistor.pins.iter().map(|p| p.at).collect(),
                local_bounds: resistor.bounds,
            })
            .unwrap();
        }
        // Add an off-screen part placed far above the viewport (even better y priority!) whose
        // label is outside the viewport; it must be culled BEFORE the 700-label cap.
        let offscreen_change = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: resistor.id,
                    at: Pt {
                        x: 0,
                        y: -150 * GRID_NM,
                    },
                    orient: Orient::IDENTITY,
                    designator: "ROFF".into(),
                    value: "999k".into(),
                },
                local_pins: resistor.pins.iter().map(|p| p.at).collect(),
                local_bounds: resistor.bounds,
            })
            .unwrap();
        let ItemId::Part(offscreen_id) = offscreen_change.added[0] else {
            unreachable!()
        };
        for i in 0..200 {
            doc.apply(Command::AddLabel(Label {
                at: Pt {
                    x: (i % 20 - 10) * 6 * GRID_NM,
                    y: (i / 20 - 5) * 6 * GRID_NM,
                },
                text: format!("N{i}").into(),
            }))
            .unwrap();
        }

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(4000.0), px(4000.0)));
        let camera = Camera::new(Pt::ZERO, 1.0, bounds);
        let mut items = Vec::new();
        doc.query(camera.visible(), &mut items);
        // Deliberately inject the off-screen part into the query slice to prove collect_candidates
        // itself rejects off-screen labels before applying the 700 cap.
        items.push(ItemId::Part(offscreen_id));
        items.sort_unstable_by_key(|id| match *id {
            ItemId::Part(id) => (0, id.0),
            ItemId::Wire(id) => (1, id.0),
            ItemId::Label(id) => (2, id.0),
        });
        let parts_end = items.partition_point(|id| matches!(id, ItemId::Part(_)));
        let wires_end = items.partition_point(|id| !matches!(id, ItemId::Label(_)));

        let ink = crate::sheet::gpui_color(omnia_theme::GRAPHITE.stage_ink);
        let ink_secondary = crate::sheet::gpui_color(omnia_theme::GRAPHITE.stage_ink_secondary);
        let buckets = Buckets {
            part: FontBucket::from_logical(PART_LABEL_PX),
            net: FontBucket::from_logical(NET_LABEL_PX),
            pin: pin_number_bucket(&camera),
        };
        let frame = TextFrame {
            doc: &doc,
            parts: &items[..parts_end],
            labels: &items[wires_end..],
            camera: &camera,
            viewport: bounds,
            scale_factor: 1.0,
            ink,
            ink_secondary,
        };
        let mut cache = TextCache::new();
        cache.sync(&doc, 1.0);
        let initial_cap = cache.candidates.capacity();
        cache.collect_candidates(&frame, buckets);
        assert_eq!(cache.candidates.len(), MAX_VISIBLE_LABELS);
        assert_eq!(
            cache.candidates.capacity(),
            initial_cap,
            "candidate buffer must never reallocate"
        );
        // Off-screen label never enters the 700 quota.
        assert!(
            cache.candidates.iter().all(|c| c.id != offscreen_id.0),
            "off-screen label must be culled before the 700-label cap"
        );
        // Every kept label is 17 px; every 13 px label was hidden first.
        assert!(
            cache
                .candidates
                .iter()
                .all(|c| c.bucket == FontBucket::from_logical(PART_LABEL_PX).unwrap())
        );
        // Equal-size ties break by world geometry (y, x) BEFORE kind: the top 350 parts keep BOTH
        // their Designator and Value (350 + 350 = 700), rather than keeping bottom-row designators
        // over top-row values.
        let desigs = cache
            .candidates
            .iter()
            .filter(|c| c.kind == LabelKind::Designator)
            .count();
        let values = cache
            .candidates
            .iter()
            .filter(|c| c.kind == LabelKind::Value)
            .count();
        assert_eq!(desigs, 350);
        assert_eq!(values, 350);

        // Reversed input slice produces the exact same kept set (determinism).
        let mut reversed_parts = items[..parts_end].to_vec();
        reversed_parts.reverse();
        let frame_rev = TextFrame {
            parts: &reversed_parts,
            ..frame
        };
        let mut cache2 = TextCache::new();
        cache2.sync(&doc, 1.0);
        cache2.collect_candidates(&frame_rev, buckets);
        let keys = |c: &TextCache| -> Vec<_> {
            c.candidates
                .iter()
                .map(|x| (x.kind, x.id, x.extra, x.bucket))
                .collect()
        };
        assert_eq!(keys(&cache), keys(&cache2));
    }

    #[test]
    fn multiline_net_labels_flatten_distinctly_without_truncating() {
        let mut a = String::new();
        let mut b = String::new();
        let mut c = String::new();
        let mut d = String::new();
        flatten_into("BUS\nA", &mut a);
        flatten_into("BUS\nB", &mut b);
        flatten_into("BUS\r\nA", &mut c);
        flatten_into("BUS A", &mut d);

        for s in [&a, &b, &c, &d] {
            assert!(!s.contains('\n') && !s.contains('\r'));
        }
        assert_ne!(a, b, "BUS\\nA and BUS\\nB must stay visually distinct");
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_eq!(a, "BUS\\nA");
        assert_eq!(b, "BUS\\nB");
        assert_eq!(c, "BUS\\r\\nA");

        assert!(matches_flattened(&a, "BUS\nA"));
        assert!(!matches_flattened(&a, "BUS\nB"));
        assert!(matches_flattened(&c, "BUS\r\nA"));
        assert!(matches_flattened("VCC", "VCC"));
        assert!(!matches_flattened("VCC", "VCC\n2"));
    }

    #[test]
    fn d6_reshapes_only_when_bucket_text_or_scale_factor_changes_and_clears_deleted_items() {
        let library = SymbolLibrary::builtin();
        let resistor = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let mut doc = Doc::new();
        let change = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: resistor.id,
                    at: Pt::ZERO,
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                local_pins: resistor.pins.iter().map(|p| p.at).collect(),
                local_bounds: resistor.bounds,
            })
            .unwrap();
        let ItemId::Part(part_id) = change.added[0] else {
            unreachable!()
        };
        let label_change = doc
            .apply(Command::AddLabel(Label {
                at: Pt { x: GRID_NM, y: 0 },
                text: "BUS\nA".into(),
            }))
            .unwrap();
        let ItemId::Label(label_id) = label_change.added[0] else {
            unreachable!()
        };

        let mut app = gpui::TestApp::new();
        let bounds = viewport();
        let mut window = app.open_window_with_options(
            gpui::WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, _| gpui::Empty,
        );
        let mut cache = TextCache::new();
        let ink = crate::sheet::gpui_color(omnia_theme::GRAPHITE.stage_ink);
        let ink2 = crate::sheet::gpui_color(omnia_theme::GRAPHITE.stage_ink_secondary);

        let run_step =
            |cache: &mut TextCache, doc: &Doc, cam: &Camera, sf: f32, window: &Window| {
                let parts = [ItemId::Part(part_id)];
                let labels = [ItemId::Label(label_id)];
                let frame = TextFrame {
                    doc,
                    parts: &parts,
                    labels: &labels,
                    camera: cam,
                    viewport: bounds,
                    scale_factor: sf,
                    ink,
                    ink_secondary: ink2,
                };
                let buckets = Buckets {
                    part: FontBucket::from_logical(PART_LABEL_PX * cam.zoom()),
                    net: FontBucket::from_logical(NET_LABEL_PX * cam.zoom()),
                    pin: pin_number_bucket(cam),
                };
                cache.sync(doc, sf);
                cache.collect_candidates(&frame, buckets);
                cache.shape_missing(&frame, window, &());
            };

        window.update(|_, window, _| {
            // Frame 1 at 100%: 1 designator + 1 value + 2 pin numbers + 1 multiline net = 5 shapes.
            let mut cam = Camera::new(Pt::ZERO, 1.0, bounds);
            run_step(&mut cache, &doc, &cam, 1.0, window);
            assert_eq!(cache.shape_count, 5);
            assert_eq!(
                cache.label_lines[label_id.0 as usize]
                    .line
                    .as_ref()
                    .unwrap()
                    .text
                    .as_str(),
                "BUS\\nA"
            );

            // Pan at same zoom (including multiline net label): 0 reshapes.
            cam.pan_by(point(13.5, -7.25));
            run_step(&mut cache, &doc, &cam, 1.0, window);
            assert_eq!(cache.shape_count, 5);

            // Slight zoom within the same 0.5 px bucket: 0 reshapes.
            let cam_same = Camera::new(Pt::ZERO, 1.01, bounds);
            run_step(&mut cache, &doc, &cam_same, 1.0, window);
            assert_eq!(cache.shape_count, 5);

            // Zoom to a new bucket: reshapes all 5.
            let cam_new = Camera::new(Pt::ZERO, 1.2, bounds);
            run_step(&mut cache, &doc, &cam_new, 1.0, window);
            assert_eq!(cache.shape_count, 10);

            // Same new bucket again: 0 reshapes.
            run_step(&mut cache, &doc, &cam_new, 1.0, window);
            assert_eq!(cache.shape_count, 10);

            // Text edit reshapes only the edited field.
            doc.apply(Command::SetText {
                item: ItemId::Part(part_id),
                field: omnia_doc::TextField::Value,
                text: "47k".into(),
            })
            .unwrap();
            run_step(&mut cache, &doc, &cam_new, 1.0, window);
            assert_eq!(cache.shape_count, 11);

            // Scale-factor change invalidates and reshapes all 5.
            run_step(&mut cache, &doc, &cam_new, 1.5, window);
            assert_eq!(cache.shape_count, 16);

            // Revision sync clears emptied field without shrinking slot capacity.
            let part_cap = cache.part_lines.capacity();
            let label_cap = cache.label_lines.capacity();
            doc.apply(Command::SetText {
                item: ItemId::Part(part_id),
                field: omnia_doc::TextField::Value,
                text: "".into(),
            })
            .unwrap();
            cache.sync(&doc, 1.5);
            assert!(cache.part_lines[part_id.0 as usize].value.line.is_none());
            assert!(
                cache.part_lines[part_id.0 as usize]
                    .designator
                    .line
                    .is_some()
            );

            // Revision sync releases deleted part & label lines while preserving slot capacity.
            doc.apply(Command::Delete {
                items: vec![ItemId::Part(part_id), ItemId::Label(label_id)],
                with_wires: false,
            })
            .unwrap();
            cache.sync(&doc, 1.5);
            assert!(
                cache.part_lines[part_id.0 as usize]
                    .designator
                    .line
                    .is_none()
            );
            assert!(cache.part_lines[part_id.0 as usize].placed_world.is_none());
            assert!(cache.label_lines[label_id.0 as usize].line.is_none());
            assert_eq!(cache.part_lines.capacity(), part_cap);
            assert_eq!(cache.label_lines.capacity(), label_cap);
        });
    }
}
