//! T5.4 part-drag benchmark script: mounts `OverlayView` over a real generated sheet,
//! presses on a verified visible part near the viewport centre, lifts it off `SheetView` during
//! warm-up, and drags its overlay ghost across deterministic grid-step waypoints while recording
//! `dirty_to_present_ms` (B6), `frame_ms` (B3/B4/B5), `allocs_per_frame.ours` (B10),
//! `sheet_paints` (expected `0` during measured drag frames, INV11), and document revision
//! stability (INV5).

use crate::alloc::AllocCounts;
use crate::driver::Script;
use crate::harness::{
    RunConfig, RunReportInput, build_run_report, fullscreen_requested, register_sheet_fonts,
    run_app, watchdog, window_options,
};
use crate::hover::BenchOverlayScope;
use crate::scene::{STRESS_SEED, Sheet};
use crate::trace::FrameTrace;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FrameDurationSnapshot, IntoElement, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, PlatformInput, Point, Render, Window,
    point, px, size,
};
use omnia_canvas::{
    Camera as SheetCamera, OverlayView,
    selection::hit_tolerance_nm,
    tool::{
        ToolState,
        move_::{drag_anchor_pin, drag_delta, selection_pivot},
    },
};
use omnia_doc::{Doc, GRID_NM, Hit, ItemId, Orient, PartId, Pt, Rect, SHEET_LIMIT_NM, hit_test};
use omnia_theme::GRAPHITE;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const VIEWPORT_MARGIN_PX: f64 = 16.0;
/// Post-ready frames drawn before `MouseDown` so initial compositor configure/activation settles
/// before the drag begins. On Linux fullscreen (`OMNIA_BENCH_FULLSCREEN`), `Settle` first waits
/// for both `is_fullscreen()` and `is_window_hovered()` (which GPUI's Wayland `wl_pointer::Enter`
/// handler sets right before synthesizing `MouseMove { pressed_button: None }`). Non-fullscreen
/// Linux runs are best-effort (they still draw `SETTLE_FRAMES` frames, since the cursor may lie
/// outside a windowed surface).
const SETTLE_FRAMES: u32 = 8;
/// Cap on frames waited for Linux fullscreen + pointer entry before failing fast.
const FULLSCREEN_WAIT_CAP: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DragWaypoint {
    pub screen: Point<Pixels>,
    pub world: Pt,
    pub expected_delta: Pt,
}

#[derive(Clone, Debug)]
pub struct DragPlan {
    pub part_id: PartId,
    pub anchor_screen: Point<Pixels>,
    pub anchor_world: Pt,
    pub pivot: Pt,
    pub anchor_pin: Option<Pt>,
    pub waypoints: Vec<DragWaypoint>,
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

fn clamp_pt(x: i64, y: i64) -> Pt {
    let limit = i64::from(SHEET_LIMIT_NM);
    Pt {
        x: x.clamp(-limit, limit) as i32,
        y: y.clamp(-limit, limit) as i32,
    }
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

fn part_stays_in_k1_and_visible(doc: &Doc, id: PartId, delta: Pt, visible: Rect) -> bool {
    let Some(part) = doc.part(id) else {
        return false;
    };
    if pt_in_k1(
        i64::from(part.at.x) + i64::from(delta.x),
        i64::from(part.at.y) + i64::from(delta.y),
    )
    .is_none()
    {
        return false;
    }
    if let Some(pins) = doc.pins(id)
        && !pins.iter().all(|pin| {
            pt_in_k1(
                i64::from(pin.x) + i64::from(delta.x),
                i64::from(pin.y) + i64::from(delta.y),
            )
            .is_some()
        })
    {
        return false;
    }
    let Some(body) = doc.part_body(id) else {
        return false;
    };
    let moved_min = clamp_pt(
        i64::from(body.min.x) + i64::from(delta.x),
        i64::from(body.min.y) + i64::from(delta.y),
    );
    let moved_max = clamp_pt(
        i64::from(body.max.x) + i64::from(delta.x),
        i64::from(body.max.y) + i64::from(delta.y),
    );
    let moved = Rect {
        min: moved_min,
        max: moved_max,
    };
    moved.intersects(visible)
}

/// Builds a closed rectangular perimeter of grid-step offsets `(gx, gy)` with `gx in 1..=rx`
/// and `gy in y_lo..=y_hi` (never containing `(0, 0)`), where every adjacent pair (including
/// wrap-around) differs by exactly one grid step on one axis.
fn perimeter_offsets(rx: i32, ry: i32, sx: i32, sy: i32) -> Vec<(i32, i32)> {
    debug_assert!(rx >= 2 && ry >= 1);
    let y_lo = -(ry / 2);
    let y_hi = ry - (ry / 2);
    let mut offsets = Vec::with_capacity((2 * (rx + ry) - 2) as usize);
    for gx in 1..=rx {
        offsets.push((sx * gx, sy * y_lo));
    }
    for gy in (y_lo + 1)..=y_hi {
        offsets.push((sx * rx, sy * gy));
    }
    for gx in (1..rx).rev() {
        offsets.push((sx * gx, sy * y_hi));
    }
    for gy in ((y_lo + 1)..y_hi).rev() {
        offsets.push((sx, sy * gy));
    }
    offsets
}

/// Selects a visible part near the viewport centre (verified via `hit_test`) and builds a
/// deterministic closed loop of grid-step drag waypoints inside `bounds` and K1 such that every
/// consecutive step (including wrap-around) changes `drag_delta` and keeps the ghost visible.
pub fn build_drag_plan(
    doc: &Doc,
    camera: &SheetCamera,
    bounds: Bounds<Pixels>,
) -> Result<DragPlan, String> {
    let margin = (f64::from(bounds.size.width).min(f64::from(bounds.size.height)) * 0.05)
        .clamp(4.0, VIEWPORT_MARGIN_PX);
    if f64::from(bounds.size.width) <= 2.0 * margin || f64::from(bounds.size.height) <= 2.0 * margin
    {
        return Err("drag-part viewport too small for target trajectory".into());
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
        .map_err(|e| format!("invalid visible drag-part rect: {e:?}"))?;

    let mut visible = Vec::new();
    doc.query(area, &mut visible);

    let tol = hit_tolerance_nm(camera);
    let mut scratch = Vec::new();
    let cx = f64::from(bounds.left()) + f64::from(bounds.size.width) * 0.5;
    let cy = f64::from(bounds.top()) + f64::from(bounds.size.height) * 0.5;

    let mut candidates: Vec<(PartId, Point<Pixels>, Pt, f64)> = Vec::new();
    for item in visible {
        let ItemId::Part(id) = item else {
            continue;
        };
        let Some(body) = doc.part_body(id) else {
            continue;
        };
        let center = clamp_pt(
            (i64::from(body.min.x) + i64::from(body.max.x)) / 2,
            (i64::from(body.min.y) + i64::from(body.max.y)) / 2,
        );
        let screen = camera.world_to_pixels(center);
        if !contains_with_margin(bounds, screen, margin) {
            continue;
        }
        let world = camera.screen_to_world(point(f64::from(screen.x), f64::from(screen.y)));
        if hit_test(doc, world, tol, &mut scratch).map(|Hit { item, .. }| item) != Some(item) {
            continue;
        }
        let dx = f64::from(screen.x) - cx;
        let dy = f64::from(screen.y) - cy;
        candidates.push((id, screen, world, dx * dx + dy * dy));
    }

    if candidates.is_empty() {
        return Err("drag-part found no hit-testable visible part".into());
    }

    candidates.sort_by(|a, b| a.3.total_cmp(&b.3).then_with(|| a.0.0.cmp(&b.0.0)));

    let visible_world = camera.visible();
    let sizes = [(8, 4), (6, 4), (4, 3), (3, 2), (2, 1)];
    let signs = [(1, 1), (-1, 1), (1, -1), (-1, -1)];

    for (part_id, anchor_screen, anchor_world, _) in candidates {
        let item = ItemId::Part(part_id);
        let pivot = selection_pivot(doc, &[item]).unwrap_or(anchor_world);
        let anchor_pin = drag_anchor_pin(doc, item, &[item]);

        for &(rx, ry) in &sizes {
            for &(sx, sy) in &signs {
                let offsets = perimeter_offsets(rx, ry, sx, sy);
                let mut waypoints = Vec::with_capacity(offsets.len());
                let mut ok = true;
                for (gx, gy) in offsets {
                    let Some(target) = pt_in_k1(
                        i64::from(anchor_world.x) + i64::from(gx) * i64::from(GRID_NM),
                        i64::from(anchor_world.y) + i64::from(gy) * i64::from(GRID_NM),
                    ) else {
                        ok = false;
                        break;
                    };
                    let screen = camera.world_to_pixels(target);
                    if !contains_with_margin(bounds, screen, margin) {
                        ok = false;
                        break;
                    }
                    let world =
                        camera.screen_to_world(point(f64::from(screen.x), f64::from(screen.y)));
                    let expected_delta =
                        drag_delta(anchor_world, world, pivot, Orient::IDENTITY, anchor_pin);
                    if expected_delta == Pt::ZERO
                        || !part_stays_in_k1_and_visible(
                            doc,
                            part_id,
                            expected_delta,
                            visible_world,
                        )
                    {
                        ok = false;
                        break;
                    }
                    waypoints.push(DragWaypoint {
                        screen,
                        world,
                        expected_delta,
                    });
                }
                if !ok || waypoints.len() < 4 {
                    continue;
                }
                let distinct_steps = waypoints
                    .windows(2)
                    .all(|pair| pair[0].expected_delta != pair[1].expected_delta)
                    && waypoints.last().unwrap().expected_delta
                        != waypoints.first().unwrap().expected_delta;
                if distinct_steps {
                    return Ok(DragPlan {
                        part_id,
                        anchor_screen,
                        anchor_world,
                        pivot,
                        anchor_pin,
                        waypoints,
                    });
                }
            }
        }
    }

    Err("drag-part found no visible part with an in-viewport grid-step drag trajectory".into())
}

pub(crate) fn mouse_left_down(position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseDown(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    })
}

pub(crate) fn mouse_left_drag(position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseMove(MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    })
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
                "drag-part window did not settle into hovered fullscreen before drag start (fullscreen: {is_fullscreen}, hovered: {is_hovered})"
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

struct DragDriver {
    config: RunConfig,
    expect_fullscreen: bool,
    doc: Entity<Doc>,
    overlay: Entity<OverlayView>,
    scene_details: Value,
    phase: DragPhase,
    plan: Option<DragPlan>,
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
    revision_changes: u64,
    last_delta: Option<Pt>,
    expected_delta: Option<Pt>,
    started: Instant,
    trace: Option<FrameTrace>,
    outcome: Rc<RefCell<Option<Result<Value, String>>>>,
}

fn active_drag_delta(view: &OverlayView, expected_part: PartId) -> Option<Pt> {
    match view.tool_state() {
        ToolState::Dragging(drag)
            if drag.is_lifted() && drag.pressed == ItemId::Part(expected_part) =>
        {
            Some(drag.delta())
        }
        _ => None,
    }
}

impl DragDriver {
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
            let plan = match build_drag_plan(self.doc.read(cx), &camera, viewport) {
                Ok(plan) => plan,
                Err(error) => return self.fail(error, window),
            };
            self.doc_revision_start = self.doc.read(cx).revision();
            self.sheet_paints_before_press = self.overlay.read(cx).sheet_paint_count(cx);
            window.dispatch_event(mouse_left_down(plan.anchor_screen), cx);
            let started_ok = matches!(
                self.overlay.read(cx).tool_state(),
                ToolState::Dragging(drag) if drag.pressed == ItemId::Part(plan.part_id)
            );
            if !started_ok {
                return self.fail(
                    format!(
                        "drag-part mouse-down at {:?} did not start dragging Part({})",
                        plan.anchor_screen, plan.part_id.0
                    ),
                    window,
                );
            }
            self.plan = Some(plan);
            self.phase = DragPhase::Move;
            // Wait one frame so GPUI draws the MouseDown frame and OverlayLayer::paint rebinds
            // pointer capture to the new frame's hitbox before the first warm-up drag move.
            return true;
        }

        let plan = self.plan.as_ref().expect("drag plan initialized in Press");
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
            self.last_delta = active_drag_delta(view, plan.part_id);
            if self.last_delta.is_none() {
                return self.fail(
                    format!(
                        "drag-part left lifted Dragging state before measured frames began (state: {:?}, fullscreen: {}, hovered: {}, active: {})",
                        view.tool_state(),
                        window.is_fullscreen(),
                        window.is_window_hovered(),
                        window.is_window_active(),
                    ),
                    window,
                );
            }
            if self.doc.read(cx).revision() != self.doc_revision_start {
                return self.fail(
                    "INV5: document revision changed during drag-part warm-up".into(),
                    window,
                );
            }
            self.previous_counts = AllocCounts::now();
        } else if step > warmup && step <= last {
            let counts = AllocCounts::now();
            self.allocs.push(counts.since(self.previous_counts));
            self.previous_counts = counts;
            let view = self.overlay.read(cx);
            let current = active_drag_delta(view, plan.part_id);
            if current != self.expected_delta {
                self.drag_mismatches += 1;
            }
            if current.is_some() && current != self.last_delta {
                self.drag_transitions += 1;
                self.last_delta = current;
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
        self.expected_delta = Some(wp.expected_delta);
        window.dispatch_event(mouse_left_drag(wp.screen), cx);
        self.step += 1;
        true
    }

    fn report(&mut self, window: &mut Window, cx: &App) -> Result<Value, String> {
        let base = self.baseline.as_ref().ok_or("no baseline snapshot")?;
        let plan = self.plan.as_ref().ok_or("no drag plan")?;
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
                "INV11: SheetView painted {} times during drag press, expected 0",
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
                "drag delta mismatch on {} of {} frames",
                self.drag_mismatches, self.config.frames
            ));
        }
        if self.drag_transitions != u64::from(self.config.frames) {
            extra_warnings.push(format!(
                "drag transitioned {} times, expected {}",
                self.drag_transitions, self.config.frames
            ));
        }
        if self.revision_changes > 0 {
            extra_warnings.push(format!(
                "INV5: document revision changed on {} measured frames during drag",
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
            "part_id": plan.part_id.0,
            "overlay_paints": overlay_paints,
            "sheet_paints": sheet_paints,
            "press_sheet_paints": self.press_sheet_paints,
            "lift_sheet_paints": self.lift_sheet_paints,
            "transitions": self.drag_transitions,
            "mismatches": self.drag_mismatches,
            "revision_changes": self.revision_changes,
            "waypoints": plan.waypoints.len(),
        });
        Ok(report)
    }
}

fn schedule_drag_step(window: &Window, driver: Rc<RefCell<DragDriver>>) {
    window.on_next_frame(move |window, cx| {
        let keep_going = driver.borrow_mut().step(window, cx);
        if keep_going {
            schedule_drag_step(window, driver);
        }
    });
}

struct DragRootView {
    overlay: Entity<OverlayView>,
    driver: Rc<RefCell<DragDriver>>,
    started: bool,
}

impl Render for DragRootView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !self.started {
            self.started = true;
            let driver = self.driver.clone();
            window.on_next_frame(move |window, _| {
                // Let the platform draw the configured window before starting the drag gesture.
                window.refresh();
                schedule_drag_step(window, driver);
            });
        }
        self.overlay.clone()
    }
}

pub fn run_drag_part(config: RunConfig) -> Result<Value, String> {
    let Sheet::Stress(stress) = config.sheet else {
        return Err(
            "--script drag-part needs a generated document (100, 1k, 10k, dense, or text)".into(),
        );
    };
    debug_assert_eq!(config.script, Script::DragPart);
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
            // T4.3/T5.2: focus OverlayView at open so drag input never triggers a focus-transfer refresh.
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            let trace = tracing.then(|| FrameTrace::new(cx));
            let expect_fullscreen = fullscreen_requested();
            let driver = Rc::new(RefCell::new(DragDriver {
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
                revision_changes: 0,
                last_delta: None,
                expected_delta: None,
                started: Instant::now(),
                trace,
                outcome,
            }));
            cx.new(|_| DragRootView {
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
    fn build_drag_plan_succeeds_for_every_stress_sheet() {
        for sheet in StressSheet::ALL {
            let doc = generate(sheet, STRESS_SEED);
            for (w, h) in [(1920.0_f32, 1080.0_f32), (1536.0, 864.0), (1280.0, 800.0)] {
                let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(w), px(h)));
                let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
                let plan = build_drag_plan(&doc, &camera, bounds)
                    .unwrap_or_else(|e| panic!("{sheet:?} at {w}x{h}: {e}"));
                assert!(plan.waypoints.len() >= 4);
                assert!(doc.part(plan.part_id).is_some());
                for wp in &plan.waypoints {
                    assert_ne!(wp.expected_delta, Pt::ZERO);
                }
                for pair in plan.waypoints.windows(2) {
                    assert_ne!(pair[0].expected_delta, pair[1].expected_delta);
                }
                assert_ne!(
                    plan.waypoints.last().unwrap().expected_delta,
                    plan.waypoints.first().unwrap().expected_delta
                );
            }
        }
    }

    #[test]
    fn drag_pointer_move_makes_zero_heap_allocations() {
        let doc = generate(StressSheet::Parts1k, STRESS_SEED);
        let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1536.0), px(864.0)));
        let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
        let plan = build_drag_plan(&doc, &camera, bounds).expect("drag plan");
        let mut machine = omnia_canvas::tool::ToolMachine::new();
        let down = machine.pointer_down(
            &doc,
            1,
            plan.anchor_world,
            hit_tolerance_nm(&camera),
            &[],
            false,
        );
        assert!(down.capture);
        let first = machine.pointer_move(plan.waypoints[0].world, true);
        assert!(first.preview_changed);
        let (_, allocs) = crate::count_allocs(|| {
            for wp in plan.waypoints.iter().skip(1) {
                let out = machine.pointer_move(wp.world, true);
                assert!(out.preview_changed);
            }
        });
        assert_eq!(
            allocs, 0,
            "ToolMachine::pointer_move must not allocate during drag"
        );
    }

    #[test]
    fn settle_waits_for_hovered_fullscreen_then_counts_post_ready_frames() {
        // Fullscreen mode waits for both fullscreen configure and pointer Enter (`hovered`).
        assert_eq!(
            advance_settle(0, 0, true, false, false).unwrap(),
            SettleStep::Wait {
                waited: 1,
                ready: 0,
            }
        );
        assert_eq!(
            advance_settle(1, 0, true, true, false).unwrap(),
            SettleStep::Wait {
                waited: 2,
                ready: 0,
            }
        );
        // Dropping fullscreen or hover resets the post-ready counter without losing waited count.
        assert_eq!(
            advance_settle(2, 3, true, true, false).unwrap(),
            SettleStep::Wait {
                waited: 3,
                ready: 0,
            }
        );
        // Once both hold, `ready` counts SETTLE_FRAMES without incrementing `waited`.
        let mut step = SettleStep::Wait {
            waited: 3,
            ready: 0,
        };
        for expected_ready in 1..=SETTLE_FRAMES {
            let SettleStep::Wait { waited, ready } = step else {
                panic!("expected Wait");
            };
            step = advance_settle(waited, ready, true, true, true).unwrap();
            assert_eq!(
                step,
                SettleStep::Wait {
                    waited: 3,
                    ready: expected_ready,
                }
            );
        }
        let SettleStep::Wait { waited, ready } = step else {
            panic!("expected Wait before Press");
        };
        assert_eq!(
            advance_settle(waited, ready, true, true, true).unwrap(),
            SettleStep::Press
        );
        assert!(advance_settle(FULLSCREEN_WAIT_CAP, 0, true, true, false).is_err());

        // Non-fullscreen path (Windows / windowed Linux) counts SETTLE_FRAMES and presses.
        let mut non_fs = SettleStep::Wait {
            waited: 0,
            ready: 0,
        };
        for expected_ready in 1..=SETTLE_FRAMES {
            let SettleStep::Wait { waited, ready } = non_fs else {
                panic!("expected Wait");
            };
            non_fs = advance_settle(waited, ready, false, false, false).unwrap();
            assert_eq!(
                non_fs,
                SettleStep::Wait {
                    waited: 0,
                    ready: expected_ready,
                }
            );
        }
        let SettleStep::Wait { waited, ready } = non_fs else {
            panic!("expected Wait before Press");
        };
        assert_eq!(
            advance_settle(waited, ready, false, false, false).unwrap(),
            SettleStep::Press
        );
    }

    #[cfg(feature = "headless")]
    #[test]
    fn headless_drag_part_inv5_inv11_b10_on_100() {
        let config = RunConfig {
            sheet: Sheet::Stress(StressSheet::Parts100),
            script: Script::DragPart,
            frames: 30,
            warmup: 10,
            size: (1280.0, 800.0),
            instrument: None,
        };
        let report = crate::scripts::run_headless(config).expect("headless drag-part");
        assert_eq!(report["config"]["script"], "drag-part");
        assert_eq!(report["metrics"]["sheet_paints"], 0);
        assert_eq!(report["metrics"]["allocs_per_frame"]["ours"]["max"], 0.0);
        assert_eq!(report["drag"]["press_sheet_paints"], 0);
        assert_eq!(report["drag"]["lift_sheet_paints"], 1);
        assert_eq!(report["drag"]["sheet_paints"], 0);
        assert_eq!(report["drag"]["overlay_paints"], 30);
        assert_eq!(report["drag"]["transitions"], 30);
        assert_eq!(report["drag"]["mismatches"], 0);
        assert_eq!(report["drag"]["revision_changes"], 0);
    }
}
