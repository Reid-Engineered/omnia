//! W3.3 segment-drag benchmark script (`wiring-plan.md` WB6, WD7, WD11, INV5, INV11):
//! mounts `OverlayView` over a real generated sheet (notably `dense` for WB6), presses on a
//! verified visible wire segment near the viewport centre, lifts it off `SheetView` during
//! warm-up, and drags it across deterministic perpendicular grid-step waypoints while recording
//! `dirty_to_present_ms` (WB6/B6), `frame_ms` (B3/B4/B5), `allocs_per_frame.ours` (B10),
//! `sheet_paints` (expected `0` during measured drag frames, INV11), and document revision
//! stability (INV5).

use crate::alloc::AllocCounts;
use crate::drag_part::{mouse_left_down, mouse_left_drag};
use crate::driver::Script;
use crate::harness::{
    RunConfig, RunReportInput, build_run_report, fullscreen_requested, register_sheet_fonts,
    run_app, watchdog, window_options,
};
use crate::hover::BenchOverlayScope;
use crate::scene::{STRESS_SEED, Sheet};
use crate::trace::FrameTrace;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FrameDurationSnapshot, IntoElement, Pixels, Point,
    Render, Window, point, px, size,
};
use omnia_canvas::{
    Camera as SheetCamera, OverlayView,
    selection::hit_tolerance_nm,
    tool::{ToolState, segment::segment_offset},
};
use omnia_doc::{Doc, GRID_NM, Hit, ItemId, Pt, Rect, SHEET_LIMIT_NM, WireId, hit_test};
use omnia_theme::GRAPHITE;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const VIEWPORT_MARGIN_PX: f64 = 16.0;
const SETTLE_FRAMES: u32 = 8;
const FULLSCREEN_WAIT_CAP: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentDragWaypoint {
    pub screen: Point<Pixels>,
    pub world: Pt,
    pub expected_offset: i32,
}

#[derive(Clone, Debug)]
pub struct SegmentDragBenchPlan {
    pub wire_id: WireId,
    pub horizontal: bool,
    pub anchor_screen: Point<Pixels>,
    pub anchor_world: Pt,
    pub waypoints: Vec<SegmentDragWaypoint>,
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

fn pt_in_k1(x: i64, y: i64) -> Option<Pt> {
    let limit = i64::from(SHEET_LIMIT_NM);
    if (-limit..=limit).contains(&x) && (-limit..=limit).contains(&y) {
        Some(Pt {
            x: x as i32,
            y: y as i32,
        })
    } else {
        None
    }
}

/// Builds a closed oscillation of non-zero grid steps `[1..=r, r-1..=1, -1..=-r, -(r-1)..=-1]`
/// where every consecutive pair (including wrap-around) differs and `0` never occurs.
fn oscillation_steps(radius: i32) -> Vec<i32> {
    debug_assert!(radius >= 1);
    let mut steps = Vec::with_capacity((4 * radius - 2).max(2) as usize);
    for g in 1..=radius {
        steps.push(g);
    }
    for g in (1..radius).rev() {
        steps.push(g);
    }
    for g in 1..=radius {
        steps.push(-g);
    }
    for g in (1..radius).rev() {
        steps.push(-g);
    }
    steps
}

/// Selects a visible wire segment near the viewport centre (verified via `hit_test`) and builds a
/// deterministic closed loop of perpendicular grid-step waypoints inside `bounds` and K1 such that
/// every consecutive step (including wrap-around) changes `segment_offset` and stays lifted.
pub fn build_drag_segment_plan(
    doc: &Doc,
    camera: &SheetCamera,
    bounds: Bounds<Pixels>,
) -> Result<SegmentDragBenchPlan, String> {
    let margin = (f64::from(bounds.size.width).min(f64::from(bounds.size.height)) * 0.05)
        .clamp(4.0, VIEWPORT_MARGIN_PX);
    if f64::from(bounds.size.width) <= 2.0 * margin || f64::from(bounds.size.height) <= 2.0 * margin
    {
        return Err("drag-segment viewport too small for target trajectory".into());
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
        .map_err(|e| format!("invalid visible drag-segment rect: {e:?}"))?;

    let mut visible = Vec::new();
    doc.query(area, &mut visible);

    let tol = hit_tolerance_nm(camera);
    let mut scratch = Vec::new();
    let cx = f64::from(bounds.left()) + f64::from(bounds.size.width) * 0.5;
    let cy = f64::from(bounds.top()) + f64::from(bounds.size.height) * 0.5;

    let mut candidates: Vec<(WireId, bool, Point<Pixels>, Pt, f64)> = Vec::new();
    for item in visible {
        let ItemId::Wire(id) = item else {
            continue;
        };
        let Some(&wire) = doc.wire(id) else {
            continue;
        };
        let mid = Pt {
            x: ((i64::from(wire.a.x) + i64::from(wire.b.x)) / 2) as i32,
            y: ((i64::from(wire.a.y) + i64::from(wire.b.y)) / 2) as i32,
        };
        let screen = camera.world_to_pixels(mid);
        if !contains_with_margin(bounds, screen, margin) {
            continue;
        }
        let world = camera.screen_to_world(point(f64::from(screen.x), f64::from(screen.y)));
        if hit_test(doc, world, tol, &mut scratch).map(|Hit { item, .. }| item) != Some(item) {
            continue;
        }
        let dx = f64::from(screen.x) - cx;
        let dy = f64::from(screen.y) - cy;
        candidates.push((id, wire.a.y == wire.b.y, screen, world, dx * dx + dy * dy));
    }

    if candidates.is_empty() {
        return Err("drag-segment found no hit-testable visible wire segment".into());
    }

    candidates.sort_by(|a, b| a.4.total_cmp(&b.4).then_with(|| a.0.0.cmp(&b.0.0)));

    for (wire_id, horizontal, anchor_screen, anchor_world, _) in candidates {
        for radius in [4, 3, 2, 1] {
            let steps = oscillation_steps(radius);
            let mut waypoints = Vec::with_capacity(steps.len());
            let mut ok = true;
            for step in steps {
                let delta_nm = i64::from(step) * i64::from(GRID_NM);
                let target = if horizontal {
                    pt_in_k1(
                        i64::from(anchor_world.x),
                        i64::from(anchor_world.y) + delta_nm,
                    )
                } else {
                    pt_in_k1(
                        i64::from(anchor_world.x) + delta_nm,
                        i64::from(anchor_world.y),
                    )
                };
                let Some(target) = target else {
                    ok = false;
                    break;
                };
                let screen = camera.world_to_pixels(target);
                if !contains_with_margin(bounds, screen, margin) {
                    ok = false;
                    break;
                }
                let world = camera.screen_to_world(point(f64::from(screen.x), f64::from(screen.y)));
                let expected_offset = segment_offset(horizontal, anchor_world, world);
                if expected_offset == 0 {
                    ok = false;
                    break;
                }
                waypoints.push(SegmentDragWaypoint {
                    screen,
                    world,
                    expected_offset,
                });
            }
            if !ok || waypoints.len() < 2 {
                continue;
            }
            let distinct_steps = waypoints
                .windows(2)
                .all(|pair| pair[0].expected_offset != pair[1].expected_offset)
                && waypoints.last().unwrap().expected_offset
                    != waypoints.first().unwrap().expected_offset;
            if distinct_steps {
                return Ok(SegmentDragBenchPlan {
                    wire_id,
                    horizontal,
                    anchor_screen,
                    anchor_world,
                    waypoints,
                });
            }
        }
    }

    Err(
        "drag-segment found no visible wire segment with an in-viewport perpendicular trajectory"
            .into(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragPhase {
    Settle { waited: u32, ready: u32 },
    Press,
    Move,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettleStep {
    Wait { waited: u32, ready: u32 },
    Press,
}

fn advance_settle(
    waited: u32,
    ready: u32,
    expect_fullscreen: bool,
    is_fullscreen: bool,
    is_hovered: bool,
) -> Result<SettleStep, String> {
    if expect_fullscreen && !(is_fullscreen && is_hovered) {
        if waited >= FULLSCREEN_WAIT_CAP {
            return Err(format!(
                "drag-segment window did not settle into hovered fullscreen before drag start (fullscreen: {is_fullscreen}, hovered: {is_hovered})"
            ));
        }
        return Ok(SettleStep::Wait {
            waited: waited + 1,
            ready: 0,
        });
    }
    if ready < SETTLE_FRAMES {
        return Ok(SettleStep::Wait {
            waited,
            ready: ready + 1,
        });
    }
    Ok(SettleStep::Press)
}

struct DragSegmentDriver {
    config: RunConfig,
    expect_fullscreen: bool,
    doc: Entity<Doc>,
    overlay: Entity<OverlayView>,
    scene_details: Value,
    phase: DragPhase,
    plan: Option<SegmentDragBenchPlan>,
    step: u32,
    draws_before_warmup: u64,
    baseline: Option<FrameDurationSnapshot>,
    doc_revision_start: u64,
    sheet_paints_before_press: u64,
    press_sheet_paints: u64,
    sheet_paints_before_lift: u64,
    lift_sheet_paints: u64,
    sheet_paints_start: u64,
    overlay_paints_start: u64,
    previous_counts: AllocCounts,
    allocs: Vec<AllocCounts>,
    drag_transitions: u64,
    drag_mismatches: u64,
    refused_frames: u64,
    revision_changes: u64,
    last_offset: Option<i32>,
    expected_offset: Option<i32>,
    started: Instant,
    trace: Option<FrameTrace>,
    outcome: Rc<RefCell<Option<Result<Value, String>>>>,
}

fn active_segment_state(view: &OverlayView, expected_wire: WireId) -> Option<(i32, bool)> {
    match view.tool_state() {
        ToolState::DraggingSegment(seg) if seg.is_lifted() && seg.wire == expected_wire => {
            Some((seg.offset(), seg.is_refused()))
        }
        _ => None,
    }
}

impl DragSegmentDriver {
    fn fail(&mut self, error: String, window: &mut Window) -> bool {
        *self.outcome.borrow_mut() = Some(Err(error));
        window.on_next_frame(|_, cx| cx.quit());
        false
    }

    fn step(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if let DragPhase::Settle { waited, ready } = self.phase {
            match advance_settle(
                waited,
                ready,
                self.expect_fullscreen,
                window.is_fullscreen(),
                window.is_window_hovered(),
            ) {
                Ok(SettleStep::Wait { waited, ready }) => {
                    self.phase = DragPhase::Settle { waited, ready };
                    window.refresh();
                    return true;
                }
                Ok(SettleStep::Press) => {
                    self.phase = DragPhase::Press;
                }
                Err(error) => return self.fail(error, window),
            }
        }

        if self.phase == DragPhase::Press {
            let camera = self.overlay.read(cx).camera().clone();
            let viewport = self.overlay.read(cx).viewport();
            let plan = match build_drag_segment_plan(self.doc.read(cx), &camera, viewport) {
                Ok(plan) => plan,
                Err(error) => return self.fail(error, window),
            };
            self.doc_revision_start = self.doc.read(cx).revision();
            self.sheet_paints_before_press = self.overlay.read(cx).sheet_paint_count(cx);
            window.dispatch_event(mouse_left_down(plan.anchor_screen), cx);
            let started_ok = matches!(
                self.overlay.read(cx).tool_state(),
                ToolState::DraggingSegment(seg) if seg.wire == plan.wire_id
            );
            if !started_ok {
                return self.fail(
                    format!(
                        "drag-segment mouse-down at {:?} did not start DraggingSegment(Wire({}))",
                        plan.anchor_screen, plan.wire_id.0
                    ),
                    window,
                );
            }
            self.plan = Some(plan);
            self.phase = DragPhase::Move;
            return true;
        }

        let plan = self
            .plan
            .as_ref()
            .expect("segment drag plan initialized in Press");
        let warmup = self.config.warmup;
        let last = warmup + self.config.frames;
        let step = self.step;

        if step == 0 {
            self.draws_before_warmup = window
                .frame_duration_snapshot()
                .draw_duration_histogram
                .len();
            let sheet_paints_now = self.overlay.read(cx).sheet_paint_count(cx);
            self.press_sheet_paints =
                sheet_paints_now.saturating_sub(self.sheet_paints_before_press);
            self.sheet_paints_before_lift = sheet_paints_now;
        }

        if step == warmup {
            self.baseline = Some(window.frame_duration_snapshot());
            let view = self.overlay.read(cx);
            let sheet_paints_now = view.sheet_paint_count(cx);
            self.lift_sheet_paints = sheet_paints_now.saturating_sub(self.sheet_paints_before_lift);
            self.sheet_paints_start = sheet_paints_now;
            self.overlay_paints_start = view.overlay_paint_count();
            self.last_offset = active_segment_state(view, plan.wire_id).map(|(offset, _)| offset);
            if self.last_offset.is_none() {
                return self.fail(
                    format!(
                        "drag-segment left lifted DraggingSegment state before measured frames began (state: {:?})",
                        view.tool_state()
                    ),
                    window,
                );
            }
            if self.doc.read(cx).revision() != self.doc_revision_start {
                return self.fail(
                    "INV5: document revision changed during drag-segment warm-up".into(),
                    window,
                );
            }
            self.previous_counts = AllocCounts::now();
        } else if step > warmup && step <= last {
            let counts = AllocCounts::now();
            self.allocs.push(counts.since(self.previous_counts));
            self.previous_counts = counts;
            let view = self.overlay.read(cx);
            let current = active_segment_state(view, plan.wire_id);
            let offset = current.map(|(offset, _)| offset);
            if offset != self.expected_offset {
                self.drag_mismatches += 1;
            }
            if let Some((offset, refused)) = current {
                if Some(offset) != self.last_offset {
                    self.drag_transitions += 1;
                    self.last_offset = Some(offset);
                }
                if refused {
                    self.refused_frames += 1;
                }
            }
            if self.doc.read(cx).revision() != self.doc_revision_start {
                self.revision_changes += 1;
            }
        }

        if step == last {
            let report = self.report(window, cx);
            *self.outcome.borrow_mut() = Some(report);
            window.on_next_frame(|_, cx| cx.quit());
            return false;
        }

        let wp = plan.waypoints[(step as usize) % plan.waypoints.len()];
        self.expected_offset = Some(wp.expected_offset);
        window.dispatch_event(mouse_left_drag(wp.screen), cx);
        self.step += 1;
        true
    }

    fn report(&mut self, window: &mut Window, cx: &App) -> Result<Value, String> {
        let base = self.baseline.as_ref().ok_or("no baseline snapshot")?;
        let plan = self.plan.as_ref().ok_or("no segment drag plan")?;
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
        if self.press_sheet_paints != 0 {
            extra_warnings.push(format!(
                "INV11: SheetView painted {} times during segment drag press, expected 0",
                self.press_sheet_paints
            ));
        }
        if self.lift_sheet_paints != 1 {
            extra_warnings.push(format!(
                "D7: SheetView painted {} times during warm-up lift-off, expected 1",
                self.lift_sheet_paints
            ));
        }
        if self.drag_mismatches > 0 {
            extra_warnings.push(format!(
                "segment drag offset mismatch on {} of {} frames",
                self.drag_mismatches, self.config.frames
            ));
        }
        if self.drag_transitions != u64::from(self.config.frames) {
            extra_warnings.push(format!(
                "segment drag transitioned {} times, expected {}",
                self.drag_transitions, self.config.frames
            ));
        }
        if self.revision_changes > 0 {
            extra_warnings.push(format!(
                "INV5: document revision changed on {} measured frames during segment drag",
                self.revision_changes
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
        report["drag"] = json!({
            "wire_id": plan.wire_id.0,
            "horizontal": plan.horizontal,
            "overlay_paints": overlay_paints,
            "sheet_paints": sheet_paints,
            "press_sheet_paints": self.press_sheet_paints,
            "lift_sheet_paints": self.lift_sheet_paints,
            "transitions": self.drag_transitions,
            "mismatches": self.drag_mismatches,
            "refused_frames": self.refused_frames,
            "revision_changes": self.revision_changes,
            "waypoints": plan.waypoints.len(),
        });
        Ok(report)
    }
}

fn schedule_drag_step(window: &Window, driver: Rc<RefCell<DragSegmentDriver>>) {
    window.on_next_frame(move |window, cx| {
        let keep_going = driver.borrow_mut().step(window, cx);
        if keep_going {
            schedule_drag_step(window, driver);
        }
    });
}

struct DragSegmentRootView {
    overlay: Entity<OverlayView>,
    driver: Rc<RefCell<DragSegmentDriver>>,
    started: bool,
}

impl Render for DragSegmentRootView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !self.started {
            self.started = true;
            let driver = self.driver.clone();
            window.on_next_frame(move |window, _| {
                window.refresh();
                schedule_drag_step(window, driver);
            });
        }
        self.overlay.clone()
    }
}

pub fn run_drag_segment(config: RunConfig) -> Result<Value, String> {
    let Sheet::Stress(stress) = config.sheet else {
        return Err(
            "--script drag-segment needs a generated document (100, 1k, 10k, dense, or text)"
                .into(),
        );
    };
    debug_assert_eq!(config.script, Script::DragSegment);
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
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            let trace = tracing.then(|| FrameTrace::new(cx));
            let expect_fullscreen = fullscreen_requested();
            let driver = Rc::new(RefCell::new(DragSegmentDriver {
                config,
                expect_fullscreen,
                doc,
                overlay: overlay.clone(),
                scene_details,
                phase: DragPhase::Settle {
                    waited: 0,
                    ready: 0,
                },
                plan: None,
                step: 0,
                draws_before_warmup: 0,
                baseline: None,
                doc_revision_start: 0,
                sheet_paints_before_press: 0,
                press_sheet_paints: 0,
                sheet_paints_before_lift: 0,
                lift_sheet_paints: 0,
                sheet_paints_start: 0,
                overlay_paints_start: 0,
                previous_counts: AllocCounts::now(),
                allocs: Vec::with_capacity(frames),
                drag_transitions: 0,
                drag_mismatches: 0,
                refused_frames: 0,
                revision_changes: 0,
                last_offset: None,
                expected_offset: None,
                started: Instant::now(),
                trace,
                outcome,
            }));
            cx.new(|_| DragSegmentRootView {
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

    #[test]
    fn build_drag_segment_plan_succeeds_for_every_stress_sheet() {
        for sheet in StressSheet::ALL {
            let doc = generate(sheet, STRESS_SEED);
            for (w, h) in [(1920.0_f32, 1080.0_f32), (1536.0, 864.0), (1280.0, 800.0)] {
                let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(w), px(h)));
                let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
                let plan = build_drag_segment_plan(&doc, &camera, bounds)
                    .unwrap_or_else(|e| panic!("{sheet:?} at {w}x{h}: {e}"));
                assert!(plan.waypoints.len() >= 2);
                assert!(doc.wire(plan.wire_id).is_some());
                for wp in &plan.waypoints {
                    assert_ne!(wp.expected_offset, 0);
                    assert_eq!(wp.expected_offset % GRID_NM, 0);
                }
                for pair in plan.waypoints.windows(2) {
                    assert_ne!(pair[0].expected_offset, pair[1].expected_offset);
                }
                assert_ne!(
                    plan.waypoints.last().unwrap().expected_offset,
                    plan.waypoints.first().unwrap().expected_offset
                );
            }
        }
    }
}
