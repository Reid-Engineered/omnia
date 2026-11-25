//! `SheetView`: paper, grid, and document content (T1.4, T2.5).
//!
//! The element implements [`Element`] directly (G8) and groups primitives into paint layers
//! (G5). Grid dots are plain square quads of
//! whole device pixels (D4, G3), placed with [`crate::snap_px`] so they never move by a
//! fraction of a pixel and never vary in size (D5).
//!
//! Grid density (K10): every `2^k`-th grid point is shown, with `k` chosen so the on-screen
//! pitch stays in [20, 40) logical px. Above 200% zoom `k` is negative and the dots sit on
//! the binary subdivisions of the grid (`k = -1` is the K2 pin lattice); `2,540,000 nm` is
//! `2^5 × 79,375`, so every level down to `k = -5`, which K4's 3200% needs, is a whole number
//! of nm. Levels are nested: zooming only adds or removes dots, it never moves one. While
//! the pitch is in the top quarter of the range, [35, 40), the dots of the next finer level
//! (the ones that drop when zooming out past 40 px) are drawn too, their alpha rising from
//! 0 at 35 px to full at 40 px, so the switch between levels is continuous.
//!
//! The grid walks the lattice directly. Document paint reuses its query and paint buffers
//! and uses prebuilt raster keys and SVG bytes (B10, INV7).

pub(crate) mod lod;
pub(crate) mod symbols;
mod text;
mod wires;

use std::{cell::RefCell, rc::Rc};

use crate::camera::Camera;
use crate::snap_px::{centered_square, stroke_device_px};
use gpui::{
    App, Bounds, ContentMask, Element, ElementId, Entity, GlobalElementId, Hsla,
    InspectorElementId, LayoutId, PaintQuad, Pixels, Style, Window, fill, relative,
};
use omnia_doc::{Doc, GRID_NM, ItemId, Pt, Rect, SHEET_LIMIT_NM};
use omnia_theme::Theme;

/// K10: the on-screen grid pitch stays at or above this many logical px.
pub const MIN_PITCH: f64 = 20.0;
/// K10: the on-screen grid pitch stays below this many logical px.
pub const MAX_PITCH: f64 = 40.0;
/// K10: the next finer level fades in over the top quarter of the pitch range.
pub const FADE_START: f64 = MAX_PITCH - (MAX_PITCH - MIN_PITCH) / 4.0;
/// K6: grid dot size in logical px; at least one device pixel.
pub const GRID_DOT_PX: f64 = 1.2;
/// Finest grid level: `GRID_NM / 32`, reached at K4's 3200%.
pub const MIN_LEVEL: i32 = -5;
/// Coarsest grid level: `GRID_NM × 32`, reached at K4's 5%.
pub const MAX_LEVEL: i32 = 5;

const _: () = assert!(GRID_NM % (1 << -MIN_LEVEL) == 0);
const _: () = assert!((GRID_NM as i64) << MAX_LEVEL <= i32::MAX as i64);

/// The grid level shown at one camera scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridLevel {
    /// `k`: every `2^k`-th grid point is shown (negative: grid subdivisions).
    pub level: i32,
    /// World distance between shown dots, `GRID_NM × 2^k`.
    pub step_nm: i32,
    /// On-screen distance between shown dots, in logical px.
    pub pitch: f64,
    /// Alpha factor in [0, 1] for the dots of the next finer level, which are drawn only
    /// when this is above zero. It reaches 1 only by f32 rounding just below 40 px.
    pub fade: f32,
}

/// K10 level selection for a camera scale in logical px per nm.
pub fn grid_level(scale: f64) -> GridLevel {
    assert!(scale.is_finite() && scale > 0.0);
    let mut level = 0;
    let mut pitch = f64::from(GRID_NM) * scale;
    // Doubling and halving are exact in f64, so `pitch == step_nm × scale` to the last bit.
    while pitch < MIN_PITCH && level < MAX_LEVEL {
        pitch *= 2.0;
        level += 1;
    }
    while pitch >= MAX_PITCH && level > MIN_LEVEL {
        pitch /= 2.0;
        level -= 1;
    }
    let step_nm = if level >= 0 {
        GRID_NM << level
    } else {
        GRID_NM >> -level
    };
    let fade = if level > MIN_LEVEL && pitch > FADE_START {
        ((pitch - FADE_START) / (MAX_PITCH - FADE_START)) as f32
    } else {
        0.0
    };
    GridLevel {
        level,
        step_nm,
        pitch,
        fade,
    }
}

/// Converts an `omnia-theme` token into GPUI's colour type.
fn gpui_color(c: omnia_theme::Rgba) -> Hsla {
    gpui::Rgba {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
    .into()
}

/// Keeps the live document and the renderer's persistent resources together. Create once
/// per sheet and clone when building a new element. The owning view controls invalidation.
#[derive(Clone, Debug)]
pub struct SheetDocument {
    doc: Entity<Doc>,
    cache: Rc<DocumentCache>,
}

impl SheetDocument {
    pub fn new(doc: Entity<Doc>) -> Self {
        Self {
            doc,
            cache: Rc::new(DocumentCache::default()),
        }
    }

    /// Shared symbol sprite masks for overlay ghosts (T5.3, zero duplicate atlas buffers).
    pub(crate) fn masks(&self) -> &symbols::Masks {
        &self.cache.masks
    }
}

/// Zero-allocation membership test against a canonically sorted slice of hidden items (T5.3, B10).
#[inline]
pub(super) fn is_hidden(hidden: &[ItemId], item: ItemId) -> bool {
    if hidden.is_empty() {
        return false;
    }
    let key = crate::selection::item_sort_key(item);
    hidden
        .binary_search_by_key(&key, |&id| crate::selection::item_sort_key(id))
        .is_ok()
}

#[derive(Debug)]
struct DocumentCache {
    masks: symbols::Masks,
    text: RefCell<text::TextCache>,
    scratch: RefCell<Scratch>,
}

impl Default for DocumentCache {
    fn default() -> Self {
        Self {
            masks: symbols::Masks::new(),
            text: RefCell::new(text::TextCache::new()),
            scratch: RefCell::default(),
        }
    }
}

#[derive(Debug, Default)]
struct Scratch {
    items: Vec<ItemId>,
    parts_end: usize,
    wires_end: usize,
    segments: wires::MergedSegments,
    junctions: Vec<Pt>,
}

#[derive(Clone, Copy)]
enum Layer {
    WiresAndBoxes,
    SymbolsAndDots,
}

enum Mark<'a> {
    Quad(Bounds<Pixels>),
    Sprite(symbols::Shape<'a>),
}

/// Lets a caller attribute paint work separately from GPUI work (B10, Q5).
pub trait SheetPaintScope {
    fn omnia(&self, paint: impl FnOnce()) {
        paint();
    }

    fn gpui(&self, paint: impl FnOnce()) {
        paint();
    }
}

impl SheetPaintScope for () {}

fn paint_mark(
    window: &mut Window,
    cx: &App,
    mark: Mark<'_>,
    sf: f32,
    ink: Hsla,
    scope: &impl SheetPaintScope,
) {
    match mark {
        Mark::Quad(bounds) => {
            let quad = fill(bounds, ink);
            scope.gpui(|| window.paint_quad(quad));
        }
        Mark::Sprite(shape) => {
            let (bounds, transform) = shape.sprite(sf);
            let key = shape.mask.key.clone();
            scope.gpui(|| {
                if let Err(error) =
                    window.paint_svg(bounds, key, Some(&shape.mask.svg), transform, ink, cx)
                {
                    eprintln!("sheet mask: {error}");
                }
            });
        }
    }
}

type EmitMarks<'sprite, 'emit> = dyn FnMut(&mut dyn FnMut(Mark<'sprite>)) + 'emit;

impl DocumentCache {
    fn draw<'a>(
        &'a self,
        doc: &Doc,
        camera: &Camera,
        sf: f32,
        hidden: &[ItemId],
        mut layer: impl FnMut(Layer, &mut EmitMarks<'a, '_>),
    ) {
        let mut scratch = self.scratch.borrow_mut();
        let Scratch {
            items,
            parts_end,
            wires_end,
            segments,
            junctions,
        } = &mut *scratch;
        let diameter = lod::junction_diameter(camera);
        let wire_width = wires::width(camera, sf);
        // Include ink whose centreline is just outside the viewport, including the one-device
        // pixel floor and the separately culled junction dots.
        let margin_px = f64::from(wire_width).max(diameter.unwrap_or(0.0) * f64::from(sf));
        let margin_nm = (margin_px / (f64::from(sf) * camera.scale())).ceil() as i32;
        let view = camera.visible();
        let area = Rect {
            min: Pt {
                x: view.min.x.saturating_sub(margin_nm).max(-SHEET_LIMIT_NM),
                y: view.min.y.saturating_sub(margin_nm).max(-SHEET_LIMIT_NM),
            },
            max: Pt {
                x: view.max.x.saturating_add(margin_nm).min(SHEET_LIMIT_NM),
                y: view.max.y.saturating_add(margin_nm).min(SHEET_LIMIT_NM),
            },
        };
        doc.query(area, items);
        if !hidden.is_empty() {
            items.retain(|&id| !is_hidden(hidden, id));
        }
        items.sort_unstable_by_key(|id| match *id {
            ItemId::Part(id) => (0, id.0),
            ItemId::Wire(id) => (1, id.0),
            ItemId::Label(id) => (2, id.0),
        });
        *parts_end = items.partition_point(|id| matches!(id, ItemId::Part(_)));
        *wires_end = items.partition_point(|id| !matches!(id, ItemId::Label(_)));
        let (parts_end, wires_end) = (*parts_end, *wires_end);
        let part_shapes = || {
            items[..parts_end].iter().filter_map(|id| match *id {
                ItemId::Part(id) => self.masks.symbol(doc.part(id)?, camera, sf),
                _ => None,
            })
        };
        let wire_segments = || {
            items[parts_end..wires_end]
                .iter()
                .filter_map(|id| match *id {
                    ItemId::Wire(id) => doc.wire(id).copied().map(wires::Segment::from),
                    _ => None,
                })
        };

        layer(Layer::WiresAndBoxes, &mut |sink| {
            let mut draw = |segment: wires::Segment| {
                sink(Mark::Quad(segment.bounds(camera, sf, wire_width)));
            };
            if diameter.is_some() {
                wire_segments().for_each(&mut draw);
            } else {
                segments
                    .visible(wire_segments())
                    .iter()
                    .copied()
                    .filter(|segment| lod::wire_run(segment.length_px(camera)))
                    .for_each(draw);
            }
            for shape in part_shapes().filter(|s| lod::symbol_box(s.logical_side)) {
                let side = stroke_device_px(shape.logical_side, sf);
                let scale = f64::from(sf);
                sink(Mark::Quad(centered_square(
                    gpui::point(shape.x / scale, shape.y / scale),
                    side,
                    sf,
                )));
            }
        });

        layer(Layer::SymbolsAndDots, &mut |sink| {
            for shape in part_shapes().filter(|s| !lod::symbol_box(s.logical_side)) {
                sink(Mark::Sprite(shape));
            }
            if let Some(diameter) = diameter {
                doc.junctions(area, junctions);
                junctions.sort_unstable_by_key(|p| (p.x, p.y));
                let side = stroke_device_px(diameter, sf);
                for &p in junctions.iter() {
                    let center = camera.world_to_screen(p);
                    if side < 3 {
                        sink(Mark::Quad(centered_square(center, side, sf)));
                    } else {
                        sink(Mark::Sprite(
                            self.masks.dot(center.x, center.y, diameter, sf),
                        ));
                    }
                }
            }
        });
    }
}

/// The sheet element. Document-backed instances share resources across frames.
#[derive(Clone, Debug)]
pub struct SheetView {
    camera: Camera,
    theme: &'static Theme,
    document: Option<SheetDocument>,
    /// T5.3: canonically sorted items currently hidden during an active drag (D7, B10).
    hidden: Option<Rc<[ItemId]>>,
}

impl SheetView {
    /// The camera's viewport is replaced by the element's bounds at paint time.
    pub fn new(camera: Camera, theme: &'static Theme) -> Self {
        Self {
            camera,
            theme,
            document: None,
            hidden: None,
        }
    }

    /// Builds a sheet element backed by the live document.
    pub fn with_document(camera: Camera, theme: &'static Theme, document: SheetDocument) -> Self {
        Self {
            camera,
            theme,
            document: Some(document),
            hidden: None,
        }
    }

    /// Hides canonically sorted `hidden` items while they are rendered as overlay ghosts (T5.3, D7).
    pub(crate) fn with_hidden(mut self, hidden: Option<Rc<[ItemId]>>) -> Self {
        self.hidden = hidden;
        self
    }

    pub fn camera(&self) -> &Camera {
        &self.camera
    }

    /// Paints the same sheet as the element, with accounting scopes around canvas and GPUI work.
    pub fn paint_with_scope(
        &self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &App,
        scope: &impl SheetPaintScope,
    ) {
        let scale_factor = window.scale_factor();
        scope.gpui(|| {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                window.paint_layer(|window| {
                    scope.omnia(|| {
                        self.draw_background(bounds, scale_factor, |bounds, color, dot| {
                            let mark = if let (Some(document), Some(side)) = (&self.document, dot)
                                && side >= 3
                            {
                                let center = bounds.center();
                                let diameter = f64::from(side) / f64::from(scale_factor);
                                Mark::Sprite(document.cache.masks.dot(
                                    f64::from(center.x),
                                    f64::from(center.y),
                                    diameter,
                                    scale_factor,
                                ))
                            } else {
                                Mark::Quad(bounds)
                            };
                            paint_mark(window, cx, mark, scale_factor, color, scope);
                        });
                    });
                });
                if let Some(document) = &self.document {
                    scope.omnia(|| {
                        let mut camera = self.camera.clone();
                        camera.set_viewport(bounds);
                        let doc = document.doc.read(cx);
                        let ink = gpui_color(self.theme.stage_ink);
                        let ink_secondary = gpui_color(self.theme.stage_ink_secondary);
                        let hidden = self.hidden.as_deref().unwrap_or(&[]);
                        document
                            .cache
                            .draw(doc, &camera, scale_factor, hidden, |_, emit| {
                                scope.gpui(|| {
                                    window.paint_layer(|window| {
                                        scope.omnia(|| {
                                            emit(&mut |mark| {
                                                paint_mark(
                                                    window,
                                                    cx,
                                                    mark,
                                                    scale_factor,
                                                    ink,
                                                    scope,
                                                );
                                            });
                                        });
                                    });
                                });
                            });
                        let scratch = document.cache.scratch.borrow();
                        let text_frame = text::TextFrame {
                            doc,
                            parts: &scratch.items[..scratch.parts_end],
                            labels: &scratch.items[scratch.wires_end..],
                            hidden,
                            camera: &camera,
                            viewport: bounds,
                            scale_factor,
                            ink,
                            ink_secondary,
                        };
                        document
                            .cache
                            .text
                            .borrow_mut()
                            .paint(&text_frame, window, scope);
                    });
                }
            });
        });
    }

    /// The background paint body, with the GPUI call left to `sink`, so benches and tests can
    /// separate our allocations from GPUI's (Q5). [`Element::paint`] passes
    /// `Window::paint_quad`. Emits the paper first, then one quad per visible dot.
    pub fn paint_quads(
        &self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        mut sink: impl FnMut(PaintQuad),
    ) {
        self.draw_background(bounds, scale_factor, |bounds, color, _| {
            sink(fill(bounds, color));
        });
    }

    fn draw_background(
        &self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        mut sink: impl FnMut(Bounds<Pixels>, Hsla, Option<u32>),
    ) {
        assert!(scale_factor.is_finite() && scale_factor > 0.0);
        sink(bounds, gpui_color(self.theme.stage_paper), None);

        let mut camera = self.camera.clone();
        camera.set_viewport(bounds);
        if self.document.is_some()
            && lod::junction_diameter(&camera).is_none()
            && f64::from(GRID_NM) * camera.scale() < MIN_PITCH
        {
            return;
        }
        let level = grid_level(camera.scale());
        let side = stroke_device_px(GRID_DOT_PX, scale_factor);
        let grid = gpui_color(self.theme.stage_grid);
        let area = camera.visible();
        if level.fade > 0.0 {
            // The finer level, with the dots that are not on the shown level faded.
            let faded = grid.opacity(level.fade);
            for_each_dot(&camera, area, level.step_nm / 2, |i, j, p| {
                let color = if i % 2 == 0 && j % 2 == 0 {
                    grid
                } else {
                    faded
                };
                sink(centered_square(p, side, scale_factor), color, Some(side));
            });
        } else {
            for_each_dot(&camera, area, level.step_nm, |_, _, p| {
                sink(centered_square(p, side, scale_factor), grid, Some(side));
            });
        }
    }
}

/// Calls `f(i, j, screen)` for every lattice point `(i × step, j × step)` inside `area`.
fn for_each_dot(
    camera: &Camera,
    area: Rect,
    step: i32,
    mut f: impl FnMut(i64, i64, gpui::Point<f64>),
) {
    let step = i64::from(step);
    let range = |min: i32, max: i32| {
        let first =
            i64::from(min).div_euclid(step) + i64::from(i64::from(min).rem_euclid(step) != 0);
        let last = i64::from(max).div_euclid(step);
        first..=last
    };
    let columns = range(area.min.x, area.max.x);
    for j in range(area.min.y, area.max.y) {
        for i in columns.clone() {
            // In range: `area` is clipped to K1 and the products lie inside it.
            let p = Pt {
                x: (i * step) as i32,
                y: (j * step) as i32,
            };
            f(i, j, camera.world_to_screen(p));
        }
    }
}

impl Element for SheetView {
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
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
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
        self.paint_with_scope(bounds, window, cx, &());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{BASE_SCALE, MAX_ZOOM, MIN_ZOOM};
    use gpui::{Background, point, px, size};
    use omnia_doc::{Command, Orient, Part, Wire};
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    struct Counting;

    thread_local! {
        static ALLOCS: Cell<Option<usize>> = const { Cell::new(None) };
    }

    fn count_allocation() {
        let _ = ALLOCS.try_with(|counter| {
            if let Some(value) = counter.get() {
                counter.set(Some(value + 1));
            }
        });
    }

    // SAFETY: the counter changes no allocator inputs or results.
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count_allocation();
            // SAFETY: the caller's contract is forwarded to System.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            count_allocation();
            // SAFETY: the caller's contract is forwarded to System.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller's contract is forwarded to System.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count_allocation();
            // SAFETY: the caller's contract is forwarded to System.
            unsafe { System.realloc(ptr, layout, size) }
        }
    }

    #[global_allocator]
    static ALLOCATOR: Counting = Counting;

    #[test]
    fn inv7_document_paint_reuses_buffers_for_1k_parts() {
        let sources = [
            include_str!("sheet.rs"),
            include_str!("sheet/wires.rs"),
            include_str!("sheet/symbols.rs"),
            include_str!("sheet/lod.rs"),
            include_str!("sheet/text.rs"),
        ];
        for source in sources {
            assert!(!source.contains(concat!("paint_", "path")));
            assert!(!source.contains(concat!("canvas", "(")));
        }

        let mut doc = Doc::new();
        let library = omnia_symbols::SymbolLibrary::builtin();
        let def = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let mut wires = Vec::new();
        for y in -12..=12 {
            for x in -20..20 {
                let at = Pt {
                    x: x * 6 * GRID_NM,
                    y: y * 6 * GRID_NM,
                };
                doc.apply(Command::PlacePart {
                    part: Part {
                        symbol: def.id,
                        at,
                        orient: Orient::IDENTITY,
                        designator: "R".into(),
                        value: "".into(),
                    },
                    local_pins: def.pins.iter().map(|pin| pin.at).collect(),
                    local_bounds: def.bounds,
                })
                .unwrap();
                wires.push(Wire {
                    a: Pt {
                        x: at.x - GRID_NM,
                        y: at.y + 3 * GRID_NM,
                    },
                    b: Pt {
                        x: at.x + GRID_NM,
                        y: at.y + 3 * GRID_NM,
                    },
                });
            }
        }
        doc.apply(Command::AddWires(wires)).unwrap();
        assert_eq!(doc.parts().count(), 1000);
        assert_eq!(doc.wires().count(), 1000);

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0)));
        let mut camera = Camera::new(Pt::ZERO, 0.2, bounds);
        let cache = DocumentCache::default();
        let paint = |camera: &Camera| {
            let mut marks = 0;
            cache.draw(&doc, camera, 1.25, &[], |_, emit| {
                emit(&mut |mark| {
                    match mark {
                        Mark::Quad(bounds) => {
                            std::hint::black_box(bounds);
                        }
                        Mark::Sprite(shape) => {
                            std::hint::black_box((shape.sprite(1.25), shape.mask.key.clone()));
                        }
                    }
                    marks += 1;
                })
            });
            marks
        };
        // Warm both the far zoom wire-run buffer and the close-zoom junction/query buffers.
        assert_eq!(paint(&camera), 2000);
        camera.zoom_about(point(960.0, 540.0), 2.0);
        paint(&camera);
        let mut marks = 0;
        ALLOCS.set(Some(0));
        for i in 0..80 {
            camera.pan_by(point(0.25, -0.125));
            camera.zoom_about(point(960.0, 540.0), if i % 2 == 0 { 0.5 } else { 2.0 });
            marks += paint(&camera);
        }
        let allocs = ALLOCS.replace(None);
        assert!(marks > 0);
        assert_eq!(allocs, Some(0));
    }

    #[test]
    fn d4_far_zoom_merges_wire_quads_without_editing_the_junction() {
        let mut doc = Doc::new();
        let wire = |a, b| Wire { a, b };
        let pt = |x, y| Pt {
            x: x * GRID_NM,
            y: y * GRID_NM,
        };
        doc.apply(Command::AddWires(vec![
            wire(pt(-1, 0), pt(1, 0)),
            wire(pt(0, 0), pt(0, 1)),
        ]))
        .unwrap();
        let cache = DocumentCache::default();
        let count = |zoom| {
            let (sheet, _) = sheet(zoom, (0.0, 0.0));
            let mut quads = 0;
            let mut sprites = 0;
            cache.draw(&doc, sheet.camera(), 1.5, &[], |_, emit| {
                emit(&mut |mark| match mark {
                    Mark::Quad(_) => quads += 1,
                    Mark::Sprite(_) => sprites += 1,
                })
            });
            (quads, sprites)
        };
        assert_eq!(count(1.0), (3, 1));
        assert_eq!(count(0.2), (2, 0));
        assert_eq!(doc.wires().count(), 3);
    }

    #[test]
    fn d4_k9_far_zoom_wire_stub_threshold_and_near_zoom_unchanged() {
        let count = |doc: &Doc, zoom: f64| {
            let cache = DocumentCache::default();
            let (view, _) = sheet(zoom, (0.0, 0.0));
            let mut quads = 0;
            let mut sprites = 0;
            cache.draw(doc, view.camera(), 1.25, &[], |_, emit| {
                emit(&mut |mark| match mark {
                    Mark::Quad(_) => quads += 1,
                    Mark::Sprite(_) => sprites += 1,
                })
            });
            (quads, sprites)
        };

        let pin = omnia_doc::PIN_LATTICE_NM;
        // At zoom = 0.25 (far zoom: junction diameter 2.0 < 2.5 px), `pin` is exactly 2.5 logical px.
        let mut at_threshold = Doc::new();
        at_threshold
            .apply(Command::AddWires(vec![
                Wire {
                    a: Pt { x: 0, y: 0 },
                    b: Pt { x: pin, y: 0 },
                },
                Wire {
                    a: Pt { x: 0, y: GRID_NM },
                    b: Pt {
                        x: 0,
                        y: GRID_NM + pin,
                    },
                },
            ]))
            .unwrap();
        assert_eq!(count(&at_threshold, 0.25), (2, 0));
        // Below 2.5 px (zoom = 0.2 -> 2.0 px): both runs are hidden at far zoom.
        assert_eq!(count(&at_threshold, 0.2), (0, 0));

        // Strictly below the 2.5 px boundary at zoom = 0.25: hidden at far zoom, drawn at near zoom.
        let mut just_below = Doc::new();
        just_below
            .apply(Command::AddWires(vec![
                Wire {
                    a: Pt { x: 0, y: 0 },
                    b: Pt { x: pin - 1, y: 0 },
                },
                Wire {
                    a: Pt { x: 0, y: GRID_NM },
                    b: Pt {
                        x: 0,
                        y: GRID_NM + pin - 1,
                    },
                },
            ]))
            .unwrap();
        assert_eq!(count(&just_below, 0.25), (0, 0));
        assert_eq!(count(&just_below, 1.0), (2, 0));

        // Near-zoom path draws even very short wires (< 2.5 px at 100% zoom).
        let mut tiny = Doc::new();
        tiny.apply(Command::AddWires(vec![Wire {
            a: Pt::ZERO,
            b: Pt { x: 100_000, y: 0 },
        }]))
        .unwrap();
        assert_eq!(count(&tiny, 1.0), (1, 0));
    }

    #[test]
    fn d4_k9_far_zoom_keeps_long_wire_made_of_short_collinear_segments() {
        let pin = omnia_doc::PIN_LATTICE_NM;
        let pt = |x, y| Pt {
            x: x * pin,
            y: y * pin,
        };
        // Two T-junction stubs keep the three collinear horizontal segments separate in Doc.
        let mut doc = Doc::new();
        doc.apply(Command::AddWires(vec![
            Wire {
                a: pt(0, 0),
                b: pt(3, 0),
            },
            Wire {
                a: pt(1, 0),
                b: pt(1, 1),
            },
            Wire {
                a: pt(2, 0),
                b: pt(2, 1),
            },
        ]))
        .unwrap();
        assert_eq!(doc.wires().count(), 5);
        for (_, wire) in doc.wires() {
            assert_eq!(wire.a.manhattan_distance(wire.b), i64::from(pin));
        }

        let cache = DocumentCache::default();
        let draw_count = |zoom: f64| {
            let (view, _) = sheet(zoom, (0.0, 0.0));
            let mut quads = 0;
            let mut sprites = 0;
            cache.draw(&doc, view.camera(), 1.25, &[], |_, emit| {
                emit(&mut |mark| match mark {
                    Mark::Quad(_) => quads += 1,
                    Mark::Sprite(_) => sprites += 1,
                })
            });
            (quads, sprites)
        };

        // Near zoom: all 5 wires + 2 junction dots.
        assert_eq!(draw_count(1.0), (5, 2));
        // Far zoom 25%: merged horizontal run is 7.5 px and both vertical stubs are 2.5 px -> 3 quads.
        assert_eq!(draw_count(0.25), (3, 0));
        // Far zoom 10%: each segment is 1.0 px (< 2.5), so the two stubs drop, while the
        // merged 3-segment horizontal run is 3.0 px (>= 2.5) and still draws.
        assert_eq!(draw_count(0.1), (1, 0));
        // Far zoom 5%: even the merged 3-segment run is 1.5 px (< 2.5), so all drop.
        assert_eq!(draw_count(0.05), (0, 0));

        // Re-filtering cached merged runs across far-zoom camera changes allocates nothing (B10).
        ALLOCS.set(Some(0));
        for zoom in [0.25, 0.1, 0.05, 0.1, 0.25] {
            std::hint::black_box(draw_count(zoom));
        }
        assert_eq!(ALLOCS.replace(None), Some(0));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn d4_wire_readback_covers_constant_whole_device_pixels() {
        use gpui::{AppContext, Context, IntoElement, Render, WindowBounds, WindowOptions};
        use std::time::Duration;

        if std::env::var_os("OMNIA_HEADLESS_CI").is_some() {
            eprintln!("wire readback skipped: OMNIA_HEADLESS_CI");
            return;
        }
        let (finished, watchdog) = std::sync::mpsc::channel();
        let guard = std::thread::spawn(move || {
            if matches!(
                watchdog.recv_timeout(Duration::from_secs(60)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ) {
                std::process::exit(3);
            }
        });
        struct Probe {
            camera: Camera,
            document: SheetDocument,
        }
        impl Render for Probe {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                SheetView::with_document(
                    self.camera.clone(),
                    &omnia_theme::GRAPHITE,
                    self.document.clone(),
                )
            }
        }

        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        gpui_platform::application().run(move |cx: &mut App| {
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(640.0), px(400.0)));
            let mut doc = Doc::new();
            let library = omnia_symbols::SymbolLibrary::builtin();
            let ground = library.get(library.by_name("Ground-COM-General").unwrap());
            let pt = |x, y| Pt { x: x * GRID_NM, y: y * GRID_NM };
            doc.apply(Command::AddWires(vec![
                Wire { a: pt(-5, -2), b: pt(1, -2) },
                Wire { a: pt(2, -2), b: pt(2, 3) },
            ])).unwrap();
            doc.apply(Command::PlacePart {
                part: Part {
                    symbol: ground.id,
                    at: pt(-2, 1),
                    orient: Orient::ROT_90,
                    designator: "".into(),
                    value: "".into(),
                },
                local_pins: ground.pins.iter().map(|pin| pin.at).collect(),
                local_bounds: ground.bounds,
            }).unwrap();
            let document = SheetDocument::new(cx.new(|_| doc));
            let handle = cx.open_window(
                WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), show: false, focus: false, ..Default::default() },
                |window, cx| {
                    window.resize(bounds.size);
                    cx.new(|_| Probe { camera: Camera::new(Pt::ZERO, 1.0, bounds), document })
                }
            ).unwrap();
            cx.spawn(async move |cx| {
                let result = cx.update(|cx| -> Result<(), String> {
                    for sf in [1.0, 1.25, 1.5] {
                        for pan in [(0.3, 0.7), (-2.6, 1.1)] {
                            handle.update(cx, |probe, window, cx| {
                                let viewport = Bounds::new(point(px(0.0), px(0.0)), window.viewport_size());
                                let mut camera = Camera::new(Pt::ZERO, 1.0, viewport);
                                camera.pan_by(point(pan.0, pan.1));
                                probe.camera = camera;
                                cx.notify();
                            }).map_err(|e| e.to_string())?;
                            let camera = handle.read_with(cx, |probe, _| probe.camera.clone()).map_err(|e| e.to_string())?;
                            let image = cx.update_window(handle.into(), |_, window, cx| {
                                window.set_scale_factor(sf);
                                window.draw(cx).clear(cx);
                                assert_eq!(window.scale_factor(), sf);
                                window.render_to_image()
                            }).map_err(|e| e.to_string())?.map_err(|e| e.to_string())?;
                            let ink = omnia_theme::GRAPHITE.stage_ink;
                            let rgb = [ink.r, ink.g, ink.b].map(|c| (c * 255.0).round() as u8);
                            let is_ink = |x: i32, y: i32| {
                                image.get_pixel(x as u32, y as u32).0[..3].iter().zip(rgb)
                                    .all(|(&a, b)| a.abs_diff(b) <= 1)
                            };
                            let origin = camera.world_to_screen(Pt::ZERO);
                            let device = |p: f64| (p * f64::from(sf)).round() as i32;
                            let width = (1.8f32 * sf).round() as usize;
                            // A clockwise turn puts the local downward stem to the left of the pin.
                            let pin = camera.world_to_screen(pt(-2, 1));
                            let stem_x = device(pin.x - 12.0);
                            let stem_y = device(pin.y);
                            if !(stem_y - 1..=stem_y + 1).any(|y| is_ink(stem_x, y))
                                || (stem_y - 1..=stem_y + 1).any(|y| is_ink(device(pin.x + 12.0), y))
                            {
                                return Err(format!("rotated ground stem at scale {sf}, pan {pan:?}"));
                            }
                            for (vertical, at) in [(false, pt(0, -2)), (true, pt(2, 0))] {
                                let center = camera.world_to_screen(at);
                                let (along, across) = if vertical {
                                    (device(origin.y), center.x)
                                } else {
                                    (device(origin.x) - 25, center.y)
                                };
                                let edge = (across * f64::from(sf) - width as f64 / 2.0 + 0.5).floor() as i32;
                                for x in along - 20..along + 20 {
                                    for y in edge - 1..=edge + width as i32 {
                                        let covered = if vertical { is_ink(y, x) } else { is_ink(x, y) };
                                        if covered != (edge..edge + width as i32).contains(&y) {
                                            return Err(format!("wire readback at scale {sf}, pan {pan:?}, vertical {vertical}: pixel ({x}, {y})"));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(())
                });
                *slot.borrow_mut() = Some(result);
                cx.update(|cx| cx.quit());
            }).detach();
        });
        finished.send(()).unwrap();
        guard.join().unwrap();
        result
            .borrow_mut()
            .take()
            .expect("render task completed")
            .unwrap();
    }

    fn check(level: GridLevel, scale: f64) {
        assert!(
            (MIN_PITCH..MAX_PITCH).contains(&level.pitch),
            "pitch {} at scale {scale}",
            level.pitch
        );
        assert_eq!(level.pitch, f64::from(level.step_nm) * scale);
        assert!((MIN_LEVEL..=MAX_LEVEL).contains(&level.level));
        assert!((0.0..=1.0).contains(&level.fade));
        assert_eq!(level.fade > 0.0, level.pitch > FADE_START);
        if level.fade > 0.0 {
            assert_eq!(level.step_nm % 2, 0, "the finer level is whole nm");
        }
    }

    #[test]
    fn pitch_stays_in_range_across_k4() {
        // Dense log sweep, both ends, and every wheel notch (K4: 2^(1/4)).
        let steps = 100_000;
        let ratio = (MAX_ZOOM / MIN_ZOOM).ln();
        for n in 0..=steps {
            let zoom = (MIN_ZOOM * (ratio * f64::from(n) / f64::from(steps)).exp())
                .clamp(MIN_ZOOM, MAX_ZOOM);
            let scale = BASE_SCALE * zoom;
            check(grid_level(scale), scale);
        }
        let mut zoom = 1.0f64;
        while zoom > MIN_ZOOM {
            zoom = (zoom / 2f64.powf(0.25)).max(MIN_ZOOM);
            check(grid_level(BASE_SCALE * zoom), BASE_SCALE * zoom);
        }
        let mut zoom = 1.0f64;
        while zoom < MAX_ZOOM {
            zoom = (zoom * 2f64.powf(0.25)).min(MAX_ZOOM);
            check(grid_level(BASE_SCALE * zoom), BASE_SCALE * zoom);
        }
        for zoom in [MIN_ZOOM, MAX_ZOOM] {
            check(grid_level(BASE_SCALE * zoom), BASE_SCALE * zoom);
        }
    }

    #[test]
    fn known_levels() {
        let at = |zoom: f64| grid_level(BASE_SCALE * zoom);
        assert_eq!(
            (at(1.0).level, at(1.0).pitch, at(1.0).step_nm),
            (0, 20.0, GRID_NM)
        );
        assert_eq!((at(0.5).level, at(0.5).pitch), (1, 20.0));
        assert_eq!((at(2.0).level, at(2.0).pitch), (-1, 20.0));
        assert_eq!(at(2.0).step_nm, omnia_doc::PIN_LATTICE_NM);
        // BASE_SCALE × zoom is not exact in f64; the ends of K4 land within an ulp or so.
        assert_eq!(at(MIN_ZOOM).level, 5);
        assert!((at(MIN_ZOOM).pitch - 32.0).abs() < 1e-9);
        assert_eq!(at(MAX_ZOOM).level, -5);
        assert!((at(MAX_ZOOM).pitch - 20.0).abs() < 1e-9);
        assert_eq!(at(MAX_ZOOM).step_nm, GRID_NM / 32);
        assert_eq!(at(1.75).fade, 0.0);
        assert_eq!(at(1.875).fade, 0.5);
    }

    #[test]
    fn levels_nest_and_fade_is_continuous() {
        // Just below a level switch the faded dots are nearly opaque; just above it the
        // finer level is shown at full alpha with no fade: no dot pops.
        for k in MIN_LEVEL + 1..=MAX_LEVEL {
            let switch = MAX_PITCH / (f64::from(GRID_NM) * 2f64.powi(k));
            let below = grid_level(switch * (1.0 - 1e-9));
            let above = grid_level(switch * (1.0 + 1e-9));
            assert_eq!(below.level, k);
            assert!(below.fade > 0.999);
            assert_eq!(above.level, k - 1);
            assert_eq!(above.fade, 0.0);
            assert_eq!(below.step_nm / 2, above.step_nm);
        }
    }

    fn sheet(zoom: f64, pan: (f64, f64)) -> (SheetView, Bounds<Pixels>) {
        let bounds = Bounds {
            origin: point(px(10.0), px(20.0)),
            size: size(px(800.0), px(600.0)),
        };
        let mut camera = Camera::new(Pt::ZERO, zoom, bounds);
        camera.pan_by(point(pan.0, pan.1));
        (SheetView::new(camera, &omnia_theme::GRAPHITE), bounds)
    }

    #[test]
    fn paints_paper_then_every_visible_dot() {
        let (view, bounds) = sheet(1.0, (0.25, -0.5));
        let mut quads = Vec::new();
        view.paint_quads(bounds, 1.0, |q| quads.push(q));
        assert_eq!(quads[0].bounds, bounds);
        assert!(
            quads[0].background == Background::from(gpui_color(omnia_theme::GRAPHITE.stage_paper))
        );
        // 800 x 600 px at a 20 px pitch: 40 or 41 columns, 30 or 31 rows.
        let dots = &quads[1..];
        assert!((40 * 30..=41 * 31).contains(&dots.len()), "{}", dots.len());
        let grid = Background::from(gpui_color(omnia_theme::GRAPHITE.stage_grid));
        assert!(dots.iter().all(|q| q.background == grid));
        for q in dots {
            assert_eq!(q.bounds.size, size(px(1.0), px(1.0)));
        }
    }

    #[test]
    fn fade_draws_the_finer_level_faded() {
        let (view, bounds) = sheet(1.875, (0.0, 0.0));
        let mut quads = Vec::new();
        view.paint_quads(bounds, 1.5, |q| quads.push(q));
        let grid = gpui_color(omnia_theme::GRAPHITE.stage_grid);
        let full = quads[1..]
            .iter()
            .filter(|q| q.background == Background::from(grid))
            .count();
        let faded = quads[1..]
            .iter()
            .filter(|q| q.background == Background::from(grid.opacity(0.5)))
            .count();
        assert_eq!(full + faded, quads.len() - 1);
        // The faded dots are three of every four lattice points.
        assert!(faded > 2 * full && faded < 4 * full, "{full} {faded}");
    }

    #[test]
    fn t5_3_hidden_items_skip_draw_with_zero_warm_allocations() {
        let mut doc = Doc::new();
        let library = omnia_symbols::SymbolLibrary::builtin();
        let def = library.get(library.by_name("Resistor-IEEE-Standard").unwrap());
        let p0 = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: def.id,
                    at: Pt {
                        x: -4 * GRID_NM,
                        y: 0,
                    },
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                local_pins: def.pins.iter().map(|pin| pin.at).collect(),
                local_bounds: def.bounds,
            })
            .unwrap()
            .added[0];
        let p1 = doc
            .apply(Command::PlacePart {
                part: Part {
                    symbol: def.id,
                    at: Pt {
                        x: 4 * GRID_NM,
                        y: 0,
                    },
                    orient: Orient::IDENTITY,
                    designator: "R2".into(),
                    value: "22k".into(),
                },
                local_pins: def.pins.iter().map(|pin| pin.at).collect(),
                local_bounds: def.bounds,
            })
            .unwrap()
            .added[0];
        let w0 = doc
            .apply(Command::AddWires(vec![Wire {
                a: Pt {
                    x: -2 * GRID_NM,
                    y: 0,
                },
                b: Pt {
                    x: 2 * GRID_NM,
                    y: 0,
                },
            }]))
            .unwrap()
            .added[0];

        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(800.0), px(600.0)));
        let camera = Camera::new(Pt::ZERO, 1.0, bounds);
        let cache = DocumentCache::default();
        let count_marks = |hidden: &[ItemId]| {
            let mut count = 0usize;
            cache.draw(&doc, &camera, 1.0, hidden, |_, emit| {
                emit(&mut |_| count += 1);
            });
            count
        };

        // 2 parts + 1 wire = 3 marks when nothing is hidden.
        assert_eq!(count_marks(&[]), 3);
        let hidden = [p0, w0];
        assert_eq!(count_marks(&hidden), 1);
        assert_eq!(count_marks(&[p0, p1, w0]), 0);

        // Warm-path allocation check (B10 / INV7): filtering hidden items allocates nothing.
        ALLOCS.set(Some(0));
        for _ in 0..40 {
            assert_eq!(count_marks(&hidden), 1);
        }
        assert_eq!(ALLOCS.replace(None), Some(0));
    }
}
