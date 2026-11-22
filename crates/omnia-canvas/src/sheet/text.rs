//! D4/D6/K8/K9/K11/G14/G15: cached horizontal schematic labels with direct glyph emission.
//!
//! Draws part designators, part values, net labels and pin numbers in the sheet's top paint layer:
//! - Typography (K8, Q3): designators and values in Inter SemiBold 17 px; net labels and pin
//!   numbers in Inter Medium 13 px (logical pixels at 100% zoom).
//! - Bucketing (K11, D6): logical font sizes are quantised to 0.5 px steps, capped at
//!   [`MAX_FONT_PX`] so the non-evicting glyph atlas stays bounded across K4's zoom range.
//!   Each label caches one [`ShapedLine`] for its current bucket and reshapes only when its
//!   bucket, text, or window scale factor changes. Pin numbers share one cached line per
//!   `(symbol, pin)` across all parts of that symbol.
//! - LOD (K9, D5, T0.6): text hides below 6.5 px; pin numbers also hide once K9 drops junction
//!   dots (< 2.5 px); at most 700 labels draw per frame, hiding smallest first deterministically
//!   with ties broken by kind, world geometry and lowest id.
//! - Direct glyph emission (D4, S5): paints cached glyphs with `Window::paint_glyph` inside one
//!   layer rather than opening a nested layer per label via `ShapedLine::paint`.

use std::{cmp::Reverse, sync::Arc};

use gpui::{
    Bounds, Font, FontWeight, Hsla, Pixels, ShapedLine, SharedString, TextRun, Window, point, px,
};
use omnia_doc::{Doc, ItemId, LabelId, PartId, Pt, SymbolId};
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

/// K9: text hides below this logical font size.
pub const MIN_TEXT_PX: f64 = 6.5;
/// K11: font sizes quantise to 0.5 logical px buckets.
pub const FONT_BUCKET_STEP_PX: f64 = 0.5;
/// Plan section 11: upper bound on rasterised logical font size so the non-evicting glyph atlas
/// stays bounded through K4's 3200% zoom sweep.
pub const MAX_FONT_PX: f64 = 128.0;
/// K9 (T0.6): maximum visible labels painted per frame; past this, smallest hide first.
pub const MAX_VISIBLE_LABELS: usize = 700;

/// Quantised K11 font-size bucket stored as exact half-pixel steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct FontBucket {
    pub(super) half_px: u16,
}

impl FontBucket {
    /// Returns the K11 0.5 px bucket for `logical_px`, or `None` when hidden by K9 (< 6.5 px).
    pub(super) fn from_logical(logical_px: f64) -> Option<Self> {
        if !logical_px.is_finite() || logical_px < MIN_TEXT_PX {
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

/// K9 visibility and K11 bucket for pin numbers at `camera`'s zoom.
///
/// Respects both K9's 6.5 px text floor and K9's 2.5 px junction/pin-number threshold so pin
/// numbers never emit sub-6.5 px glyphs and always hide once junctions hide.
pub(super) fn pin_number_bucket(camera: &Camera) -> Option<FontBucket> {
    lod::junction_diameter(camera)?;
    FontBucket::from_logical(PIN_NUMBER_PX * camera.zoom())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
enum LabelKind {
    Designator = 0,
    Value = 1,
    NetLabel = 2,
    PinNumber = 3,
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

/// Deterministic keep-order for K9's visible-label cap: larger font size wins, then kind
/// (designator > value > net label > pin number), then top-to-bottom / left-to-right world
/// geometry, then lowest id and pin index.
fn priority_key(c: &Candidate) -> (Reverse<u16>, u8, i32, i32, u32, u16) {
    (
        Reverse(c.bucket.half_px),
        c.kind as u8,
        c.world.y,
        c.world.x,
        c.id,
        c.extra,
    )
}

#[derive(Debug, Default)]
struct CachedLine {
    /// 0 means empty or invalidated; valid buckets are >= 13 (6.5 px).
    bucket: u16,
    line: Option<ShapedLine>,
}

#[derive(Debug, Default)]
struct PartLines {
    designator: CachedLine,
    value: CachedLine,
}

#[derive(Clone, Copy, Debug)]
struct Buckets {
    part: Option<FontBucket>,
    net: Option<FontBucket>,
    pin: Option<FontBucket>,
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

    /// Resets cached buckets when scale factor changes (T7.4) and grows slot tables once per
    /// document revision so steady-state frames never allocate (B10).
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
            let max_label = doc
                .labels()
                .map(|(id, _)| id.0 as usize + 1)
                .max()
                .unwrap_or(0);
            if self.label_lines.len() < max_label {
                self.label_lines.resize_with(max_label, CachedLine::default);
            }
        }
    }

    /// Collects visible label candidates from the already-culled `Doc::query` slice (D6), enforces
    /// K9's 700-label cap (hiding smallest first), shapes only cache misses, and emits glyphs
    /// directly inside one paint layer (D4, S5).
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

        let margin = 64.0f64;
        let v_left = f64::from(frame.viewport.left()) - margin;
        let v_right = f64::from(frame.viewport.right()) + margin;
        let v_top = f64::from(frame.viewport.top()) - margin;
        let v_bottom = f64::from(frame.viewport.bottom()) + margin;

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
                let has_desig = !single_line(&part.designator).is_empty();
                let has_value = !single_line(&part.value).is_empty();
                if let Some(bucket) = buckets.part {
                    let pair_flag = u16::from(has_desig && has_value);
                    if has_desig {
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
                    if has_value {
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
                if let Some(bucket) = buckets.pin
                    && def.pins.len() > 1
                {
                    for (pin_idx, pin) in def.pins.iter().enumerate() {
                        if single_line(pin.number).is_empty() {
                            continue;
                        }
                        let pin_world = part.at + part.orient.apply(pin.at);
                        let pin_screen = frame.camera.world_to_screen(pin_world);
                        if pin_screen.x < v_left
                            || pin_screen.x > v_right
                            || pin_screen.y < v_top
                            || pin_screen.y > v_bottom
                        {
                            continue;
                        }
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
            for &item in frame.labels {
                let ItemId::Label(label_id) = item else {
                    continue;
                };
                let Some(label) = frame.doc.label(label_id) else {
                    continue;
                };
                if single_line(&label.text).is_empty() {
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
                    let trimmed = single_line(arc_text);
                    if slot.bucket == c.bucket.half_px
                        && slot
                            .line
                            .as_ref()
                            .is_some_and(|l| l.text.as_str() == trimmed)
                    {
                        continue;
                    }
                    let shaped = shape_arc_line(
                        arc_text,
                        trimmed,
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
                    let trimmed = single_line(&label.text);
                    if slot.bucket == c.bucket.half_px
                        && slot
                            .line
                            .as_ref()
                            .is_some_and(|l| l.text.as_str() == trimmed)
                    {
                        continue;
                    }
                    let shaped = shape_arc_line(
                        &label.text,
                        trimmed,
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
                    let text = single_line(self.library.get(c.symbol).pins[pin_idx].number);
                    let shaped = shape_static_line(
                        text,
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
                    let (left_x, baseline_y) = part_label_origin(
                        part,
                        def,
                        c.kind == LabelKind::Value,
                        c.extra != 0,
                        line,
                        frame.camera,
                    );
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
                    let (left_x, baseline_y) =
                        pin_number_origin(part, def, def.pins[pin_idx].at, line, frame.camera);
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

/// Strips anything from the first line break onward (`WindowTextSystem::shape_line` forbids `\n`).
fn single_line(text: &str) -> &str {
    match text.find(['\n', '\r']) {
        Some(end) => &text[..end],
        None => text,
    }
}

fn shape_arc_line(
    full: &Arc<str>,
    trimmed: &str,
    bucket: FontBucket,
    font: &Font,
    color: Hsla,
    window: &Window,
    scope: &impl SheetPaintScope,
) -> ShapedLine {
    let mut out = None;
    scope.gpui(|| {
        let shared = if trimmed.len() == full.len() {
            SharedString::from(full.clone())
        } else {
            SharedString::new(trimmed)
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
    bucket: FontBucket,
    font: &Font,
    color: Hsla,
    window: &Window,
    scope: &impl SheetPaintScope,
) -> ShapedLine {
    let mut out = None;
    scope.gpui(|| {
        let shared = SharedString::new_static(text);
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

/// Computes horizontal screen `(left_x, baseline_y)` for a part's designator or value (D6).
fn part_label_origin(
    part: &omnia_doc::Part,
    def: &SymbolDef,
    is_value: bool,
    has_both: bool,
    line: &ShapedLine,
    camera: &Camera,
) -> (f64, f64) {
    let zoom = camera.zoom();
    let scale = camera.scale();
    let anchor = camera.world_to_screen(part.at);
    let c0 = part.orient.apply(def.bounds.min);
    let c1 = part.orient.apply(def.bounds.max);
    let box_min_x_nm = c0.x.min(c1.x);
    let box_max_x_nm = c0.x.max(c1.x);
    let box_min_y_nm = c0.y.min(c1.y);
    let box_max_y_nm = c0.y.max(c1.y);

    let box_min_x = anchor.x + f64::from(box_min_x_nm) * scale;
    let box_max_x = anchor.x + f64::from(box_max_x_nm) * scale;
    let box_min_y = anchor.y + f64::from(box_min_y_nm) * scale;
    let box_max_y = anchor.y + f64::from(box_max_y_nm) * scale;
    let cx = (box_min_x + box_max_x) * 0.5;
    let cy = (box_min_y + box_max_y) * 0.5;
    let box_w = box_max_x - box_min_x;
    let box_h = box_max_y - box_min_y;

    let ascent = f64::from(line.ascent);
    let descent = f64::from(line.descent).max(0.0);
    let line_h = ascent + descent;
    let line_step = f64::from(line.font_size) * PART_LINE_STEP_RATIO;
    let stack_h = if has_both { line_step + line_h } else { line_h };
    let row_offset = if is_value && has_both { line_step } else { 0.0 };

    let is_vertical = part.orient.apply(Pt { x: 1, y: 0 }).x == 0;
    if is_vertical {
        let left_x = cx + 0.25 * box_w;
        let top_y = cy - stack_h * 0.5 + row_offset;
        return (left_x, top_y + ascent);
    }

    let center_y_nm =
        i64::from(box_min_y_nm) + (i64::from(box_max_y_nm) - i64::from(box_min_y_nm)) / 2;
    let top_threshold = (i64::from(box_max_y_nm) - i64::from(box_min_y_nm)) * 2 / 5;
    let has_top_pin = def.pins.iter().any(|pin| {
        let py = i64::from(part.orient.apply(pin.at).y);
        py - center_y_nm < -top_threshold
    });
    if has_top_pin {
        let left_x = cx + 0.25 * box_w;
        let top_y = box_min_y + row_offset;
        return (left_x, top_y + ascent);
    }

    let left_x = cx - f64::from(line.width) * 0.5;
    let stack_bottom = cy - 0.25 * box_h - 4.0 * zoom;
    let top_y = stack_bottom - stack_h + row_offset;
    (left_x, top_y + ascent)
}

/// Computes horizontal screen `(left_x, baseline_y)` for one pin number, nudged inward along its
/// lead so it stays clear of the lead and connected wire.
fn pin_number_origin(
    part: &omnia_doc::Part,
    def: &SymbolDef,
    local_pin: Pt,
    line: &ShapedLine,
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
            pin_screen.x - offset - f64::from(line.width)
        } else {
            pin_screen.x + offset
        };
        (left_x, baseline_y)
    } else {
        let left_x = pin_screen.x + offset;
        let baseline_y = if inward_y > 0 {
            pin_screen.y + vgap + f64::from(line.ascent)
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
    fn k9_k11_bucket_thresholds_and_rounding() {
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
        // Pin numbers hide below K9's 6.5 px text threshold and below 2.5 px junction threshold.
        let cam_100 = Camera::new(Pt::ZERO, 1.0, viewport());
        assert_eq!(pin_number_bucket(&cam_100).unwrap().pixels(), px(13.0));
        let cam_50 = Camera::new(Pt::ZERO, 0.5, viewport());
        assert_eq!(pin_number_bucket(&cam_50).unwrap().pixels(), px(6.5));
        let cam_45 = Camera::new(Pt::ZERO, 0.45, viewport());
        assert_eq!(pin_number_bucket(&cam_45), None);
        assert!(FontBucket::from_logical(PART_LABEL_PX * 0.45).is_some());
        let cam_30 = Camera::new(Pt::ZERO, 0.30, viewport());
        assert_eq!(pin_number_bucket(&cam_30), None);
        assert_eq!(FontBucket::from_logical(PART_LABEL_PX * 0.30), None);
        let cam_min = Camera::new(Pt::ZERO, MIN_ZOOM, viewport());
        assert_eq!(pin_number_bucket(&cam_min), None);
    }

    #[test]
    fn k9_visible_label_cap_hides_smallest_first_deterministically() {
        let library = SymbolLibrary::builtin();
        let resistor = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let mut doc = Doc::new();
        // 400 parts -> 400 designators (17 px) + 400 values (17 px) + 800 pin numbers (13 px),
        // plus 200 net labels (13 px) = 1,800 total labels. Cap 700 must keep ONLY 17 px labels
        // (400 designators + 300 highest-priority values) and hide every 13 px label.
        for i in 0..400 {
            let at = Pt {
                x: (i % 20 - 10) * 6 * omnia_doc::GRID_NM,
                y: (i / 20 - 10) * 6 * omnia_doc::GRID_NM,
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
        for i in 0..200 {
            doc.apply(Command::AddLabel(Label {
                at: Pt {
                    x: (i % 20 - 10) * 6 * omnia_doc::GRID_NM,
                    y: (i / 20 - 5) * 6 * omnia_doc::GRID_NM,
                },
                text: format!("N{i}").into(),
            }))
            .unwrap();
        }

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(4000.0), px(4000.0)));
        let camera = Camera::new(Pt::ZERO, 1.0, bounds);
        let mut items = Vec::new();
        doc.query(camera.visible(), &mut items);
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
        let initial_cap = cache.candidates.capacity();
        cache.collect_candidates(&frame, buckets);
        assert_eq!(cache.candidates.len(), MAX_VISIBLE_LABELS);
        assert_eq!(
            cache.candidates.capacity(),
            initial_cap,
            "candidate buffer must never reallocate"
        );
        // Every kept label is 17 px (designator or value); every 13 px label was hidden first.
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
        assert_eq!(desigs, 400);
        assert_eq!(values, 300);
        assert!(
            cache
                .candidates
                .iter()
                .all(|c| c.bucket == FontBucket::from_logical(PART_LABEL_PX).unwrap())
        );

        // Reversed input slice produces the exact same kept set (determinism).
        let mut reversed_parts = items[..parts_end].to_vec();
        reversed_parts.reverse();
        let frame_rev = TextFrame {
            parts: &reversed_parts,
            ..frame
        };
        let mut cache2 = TextCache::new();
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
    fn d6_reshapes_only_when_bucket_text_or_scale_factor_changes() {
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
        doc.apply(Command::AddLabel(Label {
            at: Pt {
                x: omnia_doc::GRID_NM,
                y: 0,
            },
            text: "OUT".into(),
        }))
        .unwrap();

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
        let items = [ItemId::Part(part_id), ItemId::Label(LabelId(0))];
        let ink = crate::sheet::gpui_color(omnia_theme::GRAPHITE.stage_ink);
        let ink2 = crate::sheet::gpui_color(omnia_theme::GRAPHITE.stage_ink_secondary);

        let run_step =
            |cache: &mut TextCache, doc: &Doc, cam: &Camera, sf: f32, window: &Window| {
                let frame = TextFrame {
                    doc,
                    parts: &items[..1],
                    labels: &items[1..],
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
            // Frame 1 at 100%: 1 designator + 1 value + 2 pin numbers + 1 net label = 5 shapes.
            let mut cam = Camera::new(Pt::ZERO, 1.0, bounds);
            run_step(&mut cache, &doc, &cam, 1.0, window);
            assert_eq!(cache.shape_count, 5);

            // Pan at same zoom: 0 reshapes.
            cam.pan_by(point(13.5, -7.25));
            run_step(&mut cache, &doc, &cam, 1.0, window);
            assert_eq!(cache.shape_count, 5);

            // Slight zoom within the same 0.5 px bucket (17.0 * 1.01 = 17.17 -> 17.0,
            // 13.0 * 1.01 = 13.13 -> 13.0): 0 reshapes.
            let cam_same = Camera::new(Pt::ZERO, 1.01, bounds);
            run_step(&mut cache, &doc, &cam_same, 1.0, window);
            assert_eq!(cache.shape_count, 5);

            // Zoom to a new bucket (17 * 1.2 = 20.5, 13 * 1.2 = 15.5): reshapes all 5.
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
        });
    }
}
