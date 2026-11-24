//! T4.6 hover benchmark script: mounts `OverlayView` over a real generated sheet and
//! sweeps pointer hover across visible parts, wires (triggering `NetHighlight`), labels (when
//! present), and empty space while recording `sheet_paints` (expected `0`, INV11) and frame costs.

use crate::alloc::{AllocCounts, not_ours, ours};
use crate::driver::Script;
use crate::harness::{
    RunConfig, RunReportInput, build_run_report, register_sheet_fonts, run_app, watchdog,
    window_options,
};
use crate::scene::{STRESS_SEED, Sheet};
use crate::trace::FrameTrace;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FrameDurationSnapshot, IntoElement, Modifiers,
    MouseMoveEvent, Pixels, PlatformInput, Point, Render, Window, point, px, size,
};
use omnia_canvas::{
    Camera as SheetCamera, OverlayPaintScope, OverlayView, selection::hit_tolerance_nm,
};
use omnia_doc::{Doc, Hit, ItemId, Pt, Rect, SHEET_LIMIT_NM, hit_test};
use omnia_theme::GRAPHITE;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

const MAX_TARGETS_PER_CLASS: usize = 24;
const VIEWPORT_MARGIN_PX: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverKind {
    Part,
    Wire,
    Label,
    Empty,
}

#[derive(Clone, Copy, Debug)]
pub struct HoverTarget {
    pub kind: HoverKind,
    pub world: Pt,
    pub screen: Point<Pixels>,
    pub expected: Option<ItemId>,
}

fn item_order_key(item: ItemId) -> (u8, u32) {
    match item {
        ItemId::Part(id) => (0, id.0),
        ItemId::Wire(id) => (1, id.0),
        ItemId::Label(id) => (2, id.0),
    }
}

fn contains_with_margin(bounds: Bounds<Pixels>, pos: Point<Pixels>, margin: f64) -> bool {
    let x = f64::from(pos.x);
    let y = f64::from(pos.y);
    let left = f64::from(bounds.left()) + margin;
    let right = f64::from(bounds.right()) - margin;
    let top = f64::from(bounds.top()) + margin;
    let bottom = f64::from(bounds.bottom()) - margin;
    x.is_finite() && y.is_finite() && x >= left && x <= right && y >= top && y <= bottom
}

struct TargetContext<'a> {
    doc: &'a Doc,
    camera: &'a SheetCamera,
    bounds: Bounds<Pixels>,
    margin: f64,
    tol: i32,
}

impl TargetContext<'_> {
    /// Verifies that a world point maps into `bounds` (with margin) and round-trips through
    /// `world_to_pixels` -> `screen_to_world` -> `hit_test` with the exact `expected` outcome.
    fn verified(
        &self,
        candidate: Pt,
        expected: Option<ItemId>,
        kind: HoverKind,
        scratch: &mut Vec<ItemId>,
    ) -> Option<HoverTarget> {
        let screen = self.camera.world_to_pixels(candidate);
        if !contains_with_margin(self.bounds, screen, self.margin) {
            return None;
        }
        let world = self
            .camera
            .screen_to_world(point(f64::from(screen.x), f64::from(screen.y)));
        let actual = hit_test(self.doc, world, self.tol, scratch).map(|Hit { item, .. }| item);
        if actual != expected {
            return None;
        }
        Some(HoverTarget {
            kind,
            world,
            screen,
            expected,
        })
    }
}

fn clamp_pt(x: i64, y: i64) -> Pt {
    let limit = i64::from(SHEET_LIMIT_NM);
    Pt {
        x: x.clamp(-limit, limit) as i32,
        y: y.clamp(-limit, limit) as i32,
    }
}

/// Builds a deterministic, interleaved hover sweep over visible parts, wires (with resolved
/// `NetId`s for `NetHighlight`), labels (if any exist in `doc`), and empty space inside `bounds`.
/// Every consecutive pair (including wrap-around) transitions `hovered()`.
pub fn build_hover_targets(
    doc: &Doc,
    camera: &SheetCamera,
    bounds: Bounds<Pixels>,
) -> Result<Vec<HoverTarget>, String> {
    let margin = (f64::from(bounds.size.width).min(f64::from(bounds.size.height)) * 0.05)
        .clamp(4.0, VIEWPORT_MARGIN_PX);
    if f64::from(bounds.size.width) <= 2.0 * margin || f64::from(bounds.size.height) <= 2.0 * margin
    {
        return Err("hover viewport too small for target sweep".into());
    }

    let min_world = camera.screen_to_world(point(
        f64::from(bounds.left()) + margin,
        f64::from(bounds.top()) + margin,
    ));
    let max_world = camera.screen_to_world(point(
        f64::from(bounds.right()) - margin,
        f64::from(bounds.bottom()) - margin,
    ));
    let area = Rect::from_points(min_world, max_world)
        .map_err(|e| format!("invalid visible hover rect: {e:?}"))?;

    let mut visible = Vec::new();
    doc.query(area, &mut visible);
    visible.sort_unstable_by_key(|&item| item_order_key(item));

    let ctx = TargetContext {
        doc,
        camera,
        bounds,
        margin,
        tol: hit_tolerance_nm(camera),
    };
    let mut hit_scratch = Vec::new();

    let mut parts = Vec::new();
    let mut wires = Vec::new();
    let mut seen_nets = HashSet::new();
    let mut labels = Vec::new();

    for &item in &visible {
        match item {
            ItemId::Part(id) if parts.len() < MAX_TARGETS_PER_CLASS => {
                if let Some(body) = doc.part_body(id) {
                    let center = clamp_pt(
                        (i64::from(body.min.x) + i64::from(body.max.x)) / 2,
                        (i64::from(body.min.y) + i64::from(body.max.y)) / 2,
                    );
                    if let Some(t) =
                        ctx.verified(center, Some(item), HoverKind::Part, &mut hit_scratch)
                    {
                        parts.push(t);
                    }
                }
            }
            ItemId::Wire(id) if wires.len() < MAX_TARGETS_PER_CLASS => {
                let Some(net) = doc.net_of(id) else {
                    continue;
                };
                if !seen_nets.insert(net) {
                    continue;
                }
                if let Some(&wire) = doc.wire(id) {
                    let mid = clamp_pt(
                        (i64::from(wire.a.x) + i64::from(wire.b.x)) / 2,
                        (i64::from(wire.a.y) + i64::from(wire.b.y)) / 2,
                    );
                    let three_quarters = clamp_pt(
                        (i64::from(wire.a.x) + 3 * i64::from(wire.b.x)) / 4,
                        (i64::from(wire.a.y) + 3 * i64::from(wire.b.y)) / 4,
                    );
                    let candidates = [wire.b, three_quarters, mid, wire.a];
                    if let Some(t) = candidates.into_iter().find_map(|pt| {
                        ctx.verified(pt, Some(item), HoverKind::Wire, &mut hit_scratch)
                    }) {
                        wires.push(t);
                    } else {
                        seen_nets.remove(&net);
                    }
                }
            }
            ItemId::Label(id) if labels.len() < MAX_TARGETS_PER_CLASS => {
                if let Some(label) = doc.label(id)
                    && let Some(t) =
                        ctx.verified(label.at, Some(item), HoverKind::Label, &mut hit_scratch)
                {
                    labels.push(t);
                }
            }
            _ => {}
        }
    }

    // Fall back to distinct wires even if they share a net, should fewer distinct nets be visible.
    if wires.len() < MAX_TARGETS_PER_CLASS {
        for &item in &visible {
            if wires.len() >= MAX_TARGETS_PER_CLASS {
                break;
            }
            if let ItemId::Wire(id) = item
                && doc.net_of(id).is_some()
                && !wires.iter().any(|t| t.expected == Some(item))
                && let Some(&wire) = doc.wire(id)
            {
                let mid = clamp_pt(
                    (i64::from(wire.a.x) + i64::from(wire.b.x)) / 2,
                    (i64::from(wire.a.y) + i64::from(wire.b.y)) / 2,
                );
                for pt in [wire.b, mid, wire.a] {
                    if let Some(t) = ctx.verified(pt, Some(item), HoverKind::Wire, &mut hit_scratch)
                    {
                        wires.push(t);
                        break;
                    }
                }
            }
        }
    }

    // Sample a deterministic grid across the inset viewport for empty-space hover points.
    let mut empties = Vec::new();
    let cols = 16usize;
    let rows = 12usize;
    let left = f64::from(bounds.left()) + margin;
    let top = f64::from(bounds.top()) + margin;
    let span_x = (f64::from(bounds.size.width) - 2.0 * margin).max(1.0);
    let span_y = (f64::from(bounds.size.height) - 2.0 * margin).max(1.0);
    'empty: for r in 0..rows {
        for c in 0..cols {
            let sx = left + span_x * ((c as f64 + 0.5) / cols as f64);
            let sy = top + span_y * ((r as f64 + 0.5) / rows as f64);
            let world = camera.screen_to_world(point(sx, sy));
            if let Some(t) = ctx.verified(world, None, HoverKind::Empty, &mut hit_scratch) {
                empties.push(t);
                if empties.len() >= MAX_TARGETS_PER_CLASS {
                    break 'empty;
                }
            }
        }
    }

    if parts.is_empty() {
        return Err("hover target sweep found no visible parts".into());
    }
    if wires.is_empty() {
        return Err("hover target sweep found no visible net-backed wires".into());
    }
    if empties.is_empty() {
        return Err("hover target sweep found no visible empty-space points".into());
    }

    let rounds = parts
        .len()
        .max(wires.len())
        .max(labels.len())
        .max(empties.len());
    let mut sweep = Vec::with_capacity(rounds * if labels.is_empty() { 3 } else { 4 });
    for i in 0..rounds {
        sweep.push(parts[i % parts.len()]);
        sweep.push(wires[i % wires.len()]);
        if !labels.is_empty() {
            sweep.push(labels[i % labels.len()]);
        }
        sweep.push(empties[i % empties.len()]);
    }
    debug_assert!(
        sweep
            .windows(2)
            .all(|pair| pair[0].expected != pair[1].expected)
            && sweep.last().unwrap().expected != sweep.first().unwrap().expected,
        "consecutive hover targets must always change hovered item"
    );
    Ok(sweep)
}

pub(crate) struct BenchOverlayScope;

impl OverlayPaintScope for BenchOverlayScope {
    fn omnia(&self, work: &mut dyn FnMut()) {
        ours(work);
    }

    fn gpui(&self, work: &mut dyn FnMut()) {
        not_ours(work);
    }
}

pub(crate) fn mouse_move(position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: None,
        modifiers: Modifiers::none(),
    })
}

struct HoverDriver {
    config: RunConfig,
    doc: Entity<Doc>,
    overlay: Entity<OverlayView>,
    scene_details: Value,
    targets: Vec<HoverTarget>,
    step: u32,
    draws_before_warmup: u64,
    baseline: Option<FrameDurationSnapshot>,
    sheet_paints_start: u64,
    overlay_paints_start: u64,
    previous_counts: AllocCounts,
    allocs: Vec<AllocCounts>,
    hover_transitions: u64,
    hover_mismatches: u64,
    net_highlight_frames: u64,
    last_hovered: Option<ItemId>,
    expected_hovered: Option<ItemId>,
    started: Instant,
    trace: Option<FrameTrace>,
    outcome: Rc<RefCell<Option<Result<Value, String>>>>,
}

impl HoverDriver {
    fn step(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if self.targets.is_empty() {
            let camera = self.overlay.read(cx).camera().clone();
            let viewport = self.overlay.read(cx).viewport();
            let targets = match build_hover_targets(self.doc.read(cx), &camera, viewport) {
                Ok(t) => t,
                Err(error) => {
                    *self.outcome.borrow_mut() = Some(Err(error));
                    window.on_next_frame(|_, cx| cx.quit());
                    return false;
                }
            };
            self.targets = targets;
        }

        let warmup = self.config.warmup;
        let last = warmup + self.config.frames;
        let step = self.step;

        if step == 0 {
            self.draws_before_warmup = window
                .frame_duration_snapshot()
                .draw_duration_histogram
                .len();
        }

        if step == warmup {
            self.baseline = Some(window.frame_duration_snapshot());
            let view = self.overlay.read(cx);
            self.sheet_paints_start = view.sheet_paint_count(cx);
            self.overlay_paints_start = view.overlay_paint_count();
            self.last_hovered = view.hovered();
            self.previous_counts = AllocCounts::now();
        } else if step > warmup && step <= last {
            let counts = AllocCounts::now();
            self.allocs.push(counts.since(self.previous_counts));
            self.previous_counts = counts;
            let view = self.overlay.read(cx);
            let hovered = view.hovered();
            if hovered != self.expected_hovered {
                self.hover_mismatches += 1;
            }
            if hovered != self.last_hovered {
                self.hover_transitions += 1;
                self.last_hovered = hovered;
            }
            if !view.highlighted_wires().is_empty() {
                self.net_highlight_frames += 1;
            }
        }

        if step == last {
            let report = self.report(window, cx);
            *self.outcome.borrow_mut() = Some(report);
            window.on_next_frame(|_, cx| cx.quit());
            return false;
        }

        let target = self.targets[(step as usize) % self.targets.len()];
        self.expected_hovered = target.expected;
        window.dispatch_event(mouse_move(target.screen), cx);
        self.step += 1;
        true
    }

    fn report(&mut self, window: &mut Window, cx: &App) -> Result<Value, String> {
        let base = self.baseline.as_ref().ok_or("no baseline snapshot")?;
        let view = self.overlay.read(cx);
        let sheet_paints = view
            .sheet_paint_count(cx)
            .saturating_sub(self.sheet_paints_start);
        let overlay_paints = view
            .overlay_paint_count()
            .saturating_sub(self.overlay_paints_start);

        let mut extra_metrics = Map::new();
        extra_metrics.insert("sheet_paints".into(), json!(sheet_paints));

        let mut extra_warnings = Vec::new();
        if overlay_paints != u64::from(self.config.frames) {
            extra_warnings.push(format!(
                "overlay painted {overlay_paints} times, expected {}",
                self.config.frames
            ));
        }
        if self.hover_mismatches > 0 {
            extra_warnings.push(format!(
                "hover target mismatch on {} of {} frames",
                self.hover_mismatches, self.config.frames
            ));
        }

        let mut report = build_run_report(RunReportInput {
            config: self.config,
            scene_details: Some(self.scene_details.clone()),
            window,
            cx,
            base,
            draws_before_warmup: self.draws_before_warmup,
            allocs: &self.allocs,
            paint_ms: &mut [],
            paint_sampling: "overlay-every-frame",
            paint_samples: overlay_paints as usize,
            started: self.started,
            trace: self.trace.as_mut(),
            extra_metrics,
            extra_warnings,
        })?;
        report["hover"] = json!({
            "overlay_paints": overlay_paints,
            "sheet_paints": sheet_paints,
            "transitions": self.hover_transitions,
            "mismatches": self.hover_mismatches,
            "net_highlight_frames": self.net_highlight_frames,
            "sweep_targets": self.targets.len(),
        });
        Ok(report)
    }
}

fn schedule_hover_step(window: &Window, driver: Rc<RefCell<HoverDriver>>) {
    window.on_next_frame(move |window, cx| {
        let keep_going = driver.borrow_mut().step(window, cx);
        if keep_going {
            schedule_hover_step(window, driver);
        }
    });
}

struct HoverRootView {
    overlay: Entity<OverlayView>,
    driver: Rc<RefCell<HoverDriver>>,
    started: bool,
}

impl Render for HoverRootView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !self.started {
            self.started = true;
            let driver = self.driver.clone();
            window.on_next_frame(move |window, _| {
                // Let the platform draw the configured window before sending hover input.
                window.refresh();
                schedule_hover_step(window, driver);
            });
        }
        self.overlay.clone()
    }
}

pub fn run_hover(config: RunConfig) -> Result<Value, String> {
    let Sheet::Stress(stress) = config.sheet else {
        return Err(
            "--script hover needs a generated document (100, 1k, 10k, dense, or text)".into(),
        );
    };
    debug_assert_eq!(config.script, Script::Hover);
    let tracing = std::env::var_os("OMNIA_BENCH_TRACE").is_some();
    let enabled_trace = tracing && gpui::set_trace_enabled(true);
    watchdog(
        "run",
        Duration::from_secs(60) + Duration::from_millis(100) * (config.frames + config.warmup),
    );
    let result = run_app("run", move |cx, outcome| {
        register_sheet_fonts(config.sheet, cx)?;
        let options = window_options(config.size, cx, true);
        let frames = config.frames as usize;
        let (parts, wires) = stress.counts();
        let scene_details = json!({ "seed": STRESS_SEED, "parts": parts, "wires": wires });
        cx.open_window(options, move |window, cx| {
            #[cfg(not(target_os = "linux"))]
            window.resize(size(px(config.size.0), px(config.size.1)));
            let doc = cx.new(|_| crate::generate::generate(stress, STRESS_SEED));
            let viewport = Bounds::new(
                point(px(0.0), px(0.0)),
                size(px(config.size.0), px(config.size.1)),
            );
            let camera = SheetCamera::new(Pt::ZERO, 1.0, viewport);
            let overlay = cx.new(|cx| {
                OverlayView::new(doc.clone(), camera, &GRAPHITE, cx)
                    .with_paint_scope(Rc::new(BenchOverlayScope))
            });
            // T4.3: focus OverlayView at open so clicks/hover never trigger a focus-transfer refresh.
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            let trace = tracing.then(|| FrameTrace::new(cx));
            let driver = Rc::new(RefCell::new(HoverDriver {
                config,
                doc,
                overlay: overlay.clone(),
                scene_details,
                targets: Vec::new(),
                step: 0,
                draws_before_warmup: 0,
                baseline: None,
                sheet_paints_start: 0,
                overlay_paints_start: 0,
                previous_counts: AllocCounts::now(),
                allocs: Vec::with_capacity(frames),
                hover_transitions: 0,
                hover_mismatches: 0,
                net_highlight_frames: 0,
                last_hovered: None,
                expected_hovered: None,
                started: Instant::now(),
                trace,
                outcome,
            }));
            cx.new(|_| HoverRootView {
                overlay,
                driver,
                started: false,
            })
        })
        .map_err(|e| format!("open window: {e}"))?;
        cx.activate(true);
        Ok(())
    });
    if enabled_trace {
        gpui::set_trace_enabled(false);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::{StressSheet, generate};
    use omnia_doc::{GRID_NM, Label, Orient, Part, SymbolId, Wire};

    #[test]
    fn build_hover_targets_covers_parts_wires_and_empty_on_10k() {
        let doc = generate(StressSheet::Parts10k, STRESS_SEED);
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1536.0), px(864.0)));
        let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
        let targets = build_hover_targets(&doc, &camera, bounds).unwrap();
        assert!(!targets.is_empty());
        assert!(targets.iter().any(|t| t.kind == HoverKind::Part));
        assert!(targets.iter().any(|t| t.kind == HoverKind::Wire));
        assert!(targets.iter().any(|t| t.kind == HoverKind::Empty));
        for pair in targets.windows(2) {
            assert_ne!(pair[0].expected, pair[1].expected);
        }
        assert_ne!(
            targets.last().unwrap().expected,
            targets.first().unwrap().expected
        );
    }

    #[test]
    fn build_hover_targets_includes_labels_when_present() {
        let g = GRID_NM;
        let doc = Doc::from_items(
            [(
                Part {
                    symbol: SymbolId(0),
                    at: Pt { x: -6 * g, y: 0 },
                    orient: Orient::IDENTITY,
                    designator: "R1".into(),
                    value: "10k".into(),
                },
                vec![Pt { x: 2 * g, y: 0 }],
                Rect::from_points(Pt { x: -2 * g, y: -g }, Pt { x: 2 * g, y: g }).unwrap(),
            )],
            [Wire {
                a: Pt { x: -4 * g, y: 0 },
                b: Pt { x: 4 * g, y: 0 },
            }],
            [Label {
                at: Pt { x: 0, y: 4 * g },
                text: "OUT".into(),
            }],
        )
        .unwrap();
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1280.0), px(800.0)));
        let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
        let targets = build_hover_targets(&doc, &camera, bounds).unwrap();
        assert!(targets.iter().any(|t| t.kind == HoverKind::Part));
        assert!(targets.iter().any(|t| t.kind == HoverKind::Wire));
        assert!(targets.iter().any(|t| t.kind == HoverKind::Label));
        assert!(targets.iter().any(|t| t.kind == HoverKind::Empty));
    }
}
