//! T6.4 wire-tool benchmark script: mounts `OverlayView` over a real generated sheet (notably
//! `1k` and `dense`), enters `Tool::Wire`, clicks an in-viewport start anchor to begin a live
//! wire chain without modifying the document, and sweeps the cursor across deterministic
//! in-viewport WD5 snap targets (`Pin`, `WireEnd`, `Junction`, `WireBody`, `Grid`) while
//! rendering the live WD6 L-route preview and snap markers. Records `dirty_to_present_ms` (B6),
//! `frame_ms` (B3/B5), `allocs_per_frame.ours` (B10), `sheet_paints` (expected `0` during press,
//! warm-up, and measured frames, INV11), and document revision stability (INV5).

use crate::alloc::AllocCounts;
use crate::drag_part::mouse_left_down;
use crate::driver::Script;
use crate::harness::{
    RunConfig, RunReportInput, build_run_report, fullscreen_requested, register_sheet_fonts,
    run_app, watchdog, window_options,
};
use crate::hover::{BenchOverlayScope, mouse_move};
use crate::scene::{STRESS_SEED, Sheet};
use crate::trace::FrameTrace;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FrameDurationSnapshot, IntoElement, Modifiers,
    MouseButton, MouseUpEvent, Pixels, PlatformInput, Point, Render, Window, point, px, size,
};
use omnia_canvas::{
    Camera as SheetCamera, OverlayView,
    tool::{Tool, ToolState, wire::snap_radius_nm},
};
use omnia_doc::{Doc, GRID_NM, ItemId, Pt, Rect, SHEET_LIMIT_NM, Snap, snap};
use omnia_theme::GRAPHITE;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const VIEWPORT_MARGIN_PX: f64 = 16.0;
const MAX_WAYPOINTS_PER_CLASS: usize = 6;
/// Post-ready frames drawn before `MouseDown` so initial compositor configure/activation settles
/// before the wire tool starts. Mirrors `drag_part.rs` on Linux fullscreen (`OMNIA_BENCH_FULLSCREEN`).
const SETTLE_FRAMES: u32 = 8;
/// Cap on frames waited for Linux fullscreen + pointer entry before failing fast.
const FULLSCREEN_WAIT_CAP: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireWaypoint {
    pub screen: Point<Pixels>,
    pub world: Pt,
    pub expected_snap: Snap,
    pub two_leg: bool,
}

#[derive(Clone, Debug)]
pub struct WirePlan {
    pub start_screen: Point<Pixels>,
    pub start_world: Pt,
    pub start_snap: Snap,
    pub waypoints: Vec<WireWaypoint>,
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

fn snap_class_index(target: Snap) -> usize {
    match target {
        Snap::Pin { .. } => 0,
        Snap::WireEnd { .. } => 1,
        Snap::Junction { .. } => 2,
        Snap::WireBody { .. } => 3,
        Snap::Grid(_) => 4,
    }
}

fn probe_point(
    doc: &Doc,
    camera: &SheetCamera,
    bounds: Bounds<Pixels>,
    margin: f64,
    snap_radius: i32,
    candidate: Pt,
    scratch: &mut Vec<ItemId>,
) -> Option<(Point<Pixels>, Pt, Snap)> {
    let screen = camera.world_to_pixels(candidate);
    if !contains_with_margin(bounds, screen, margin) {
        return None;
    }
    let raw_world = camera.screen_to_world(point(f64::from(screen.x), f64::from(screen.y)));
    let world = clamp_pt(i64::from(raw_world.x), i64::from(raw_world.y));
    let target = snap(doc, world, GRID_NM, snap_radius, scratch);
    Some((screen, world, target))
}

/// Selects a visible start anchor (preferring a part pin near the viewport centre) and builds a
/// deterministic closed loop of in-viewport wire-preview waypoints covering every WD5 snap class
/// present in the viewport such that every consecutive step (including wrap-around) transitions
/// `WireGesture::snap` and keeps a non-degenerate L-route preview active.
pub fn build_wire_plan(
    doc: &Doc,
    camera: &SheetCamera,
    bounds: Bounds<Pixels>,
) -> Result<WirePlan, String> {
    let margin = (f64::from(bounds.size.width).min(f64::from(bounds.size.height)) * 0.05)
        .clamp(4.0, VIEWPORT_MARGIN_PX);
    if f64::from(bounds.size.width) <= 2.0 * margin || f64::from(bounds.size.height) <= 2.0 * margin
    {
        return Err("drag-wire viewport too small for target trajectory".into());
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
        .map_err(|e| format!("invalid visible drag-wire rect: {e:?}"))?;

    let mut visible = Vec::new();
    doc.query(area, &mut visible);
    visible.sort_by_key(|item| match *item {
        ItemId::Part(id) => (0u8, id.0),
        ItemId::Wire(id) => (1u8, id.0),
        ItemId::Label(id) => (2u8, id.0),
    });

    let snap_radius = snap_radius_nm(camera);
    let mut scratch = Vec::new();
    let cx = f64::from(bounds.left()) + f64::from(bounds.size.width) * 0.5;
    let cy = f64::from(bounds.top()) + f64::from(bounds.size.height) * 0.5;

    let dist_sq_center = |screen: Point<Pixels>| -> f64 {
        let dx = f64::from(screen.x) - cx;
        let dy = f64::from(screen.y) - cy;
        dx * dx + dy * dy
    };

    struct StartCandidate {
        rank: u8,
        dist_sq: f64,
        at: Pt,
        screen: Point<Pixels>,
        world: Pt,
        target: Snap,
    }

    // 1. Choose a deterministic start anchor near the viewport centre (preferring Snap::Pin).
    let mut start_candidates: Vec<StartCandidate> = Vec::new();
    for &item in &visible {
        match item {
            ItemId::Part(id) => {
                if let Some(pins) = doc.pins(id) {
                    for &pin in pins {
                        if let Some((screen, world, target)) =
                            probe_point(doc, camera, bounds, margin, snap_radius, pin, &mut scratch)
                        {
                            let rank = match target {
                                Snap::Pin { .. } => 0,
                                Snap::Junction { .. } | Snap::WireEnd { .. } => 1,
                                Snap::WireBody { .. } => 2,
                                Snap::Grid(_) => 3,
                            };
                            start_candidates.push(StartCandidate {
                                rank,
                                dist_sq: dist_sq_center(screen),
                                at: target.at(),
                                screen,
                                world,
                                target,
                            });
                        }
                    }
                }
            }
            ItemId::Wire(id) => {
                if let Some(&wire) = doc.wire(id) {
                    for end in [wire.a, wire.b] {
                        if let Some((screen, world, target)) =
                            probe_point(doc, camera, bounds, margin, snap_radius, end, &mut scratch)
                        {
                            let rank = match target {
                                Snap::Pin { .. } => 0,
                                Snap::Junction { .. } | Snap::WireEnd { .. } => 1,
                                Snap::WireBody { .. } => 2,
                                Snap::Grid(_) => 3,
                            };
                            start_candidates.push(StartCandidate {
                                rank,
                                dist_sq: dist_sq_center(screen),
                                at: target.at(),
                                screen,
                                world,
                                target,
                            });
                        }
                    }
                }
            }
            ItemId::Label(_) => {}
        }
    }

    if start_candidates.is_empty() {
        let center_world = camera.screen_to_world(point(cx, cy));
        let clamped = clamp_pt(i64::from(center_world.x), i64::from(center_world.y));
        if let Some((screen, world, target)) = probe_point(
            doc,
            camera,
            bounds,
            margin,
            snap_radius,
            clamped,
            &mut scratch,
        ) {
            start_candidates.push(StartCandidate {
                rank: 3,
                dist_sq: 0.0,
                at: target.at(),
                screen,
                world,
                target,
            });
        }
    }

    if start_candidates.is_empty() {
        return Err("drag-wire found no in-viewport start anchor".into());
    }

    start_candidates.sort_by(|a, b| {
        a.rank
            .cmp(&b.rank)
            .then_with(|| a.dist_sq.total_cmp(&b.dist_sq))
            .then_with(|| a.at.x.cmp(&b.at.x))
            .then_with(|| a.at.y.cmp(&b.at.y))
    });
    let StartCandidate {
        screen: start_screen,
        world: start_world,
        target: start_snap,
        ..
    } = start_candidates[0];
    let start_pt = start_snap.at();

    struct WaypointCandidate {
        collinear: bool,
        dist_sq: f64,
        at: Pt,
        waypoint: WireWaypoint,
    }

    // 2. Gather verified waypoint candidates per WD5 snap class.
    let mut buckets: [Vec<WaypointCandidate>; 5] = Default::default();
    let mut consider = |candidate: Pt, scratch: &mut Vec<ItemId>| {
        let Some((screen, world, target)) =
            probe_point(doc, camera, bounds, margin, snap_radius, candidate, scratch)
        else {
            return;
        };
        let at = target.at();
        if at == start_pt {
            return;
        }
        let two_leg = at.x != start_pt.x && at.y != start_pt.y;
        let waypoint = WireWaypoint {
            screen,
            world,
            expected_snap: target,
            two_leg,
        };
        let class = snap_class_index(target);
        buckets[class].push(WaypointCandidate {
            collinear: !two_leg,
            dist_sq: dist_sq_center(screen),
            at,
            waypoint,
        });
    };

    for &item in &visible {
        match item {
            ItemId::Part(id) => {
                if let Some(pins) = doc.pins(id) {
                    for &pin in pins {
                        consider(pin, &mut scratch);
                    }
                }
            }
            ItemId::Wire(id) => {
                if let Some(&wire) = doc.wire(id) {
                    consider(wire.a, &mut scratch);
                    consider(wire.b, &mut scratch);
                    let mid = Pt {
                        x: ((i64::from(wire.a.x) + i64::from(wire.b.x)) / 2) as i32,
                        y: ((i64::from(wire.a.y) + i64::from(wire.b.y)) / 2) as i32,
                    };
                    consider(mid, &mut scratch);
                }
            }
            ItemId::Label(_) => {}
        }
    }

    // Add a grid field around `start_pt` so `Snap::Grid` (and non-collinear L-routes) are always
    // represented even on dense sheets.
    for gx in (-10..=10).step_by(2) {
        for gy in (-8..=8).step_by(2) {
            if gx == 0 && gy == 0 {
                continue;
            }
            if let Some(pt) = pt_in_k1(
                i64::from(start_pt.x) + i64::from(gx) * i64::from(GRID_NM),
                i64::from(start_pt.y) + i64::from(gy) * i64::from(GRID_NM),
            ) {
                consider(pt, &mut scratch);
            }
        }
    }

    let mut trimmed: [Vec<WireWaypoint>; 5] = Default::default();
    for (bucket, out) in buckets.iter_mut().zip(trimmed.iter_mut()) {
        bucket.sort_by(|a, b| {
            a.collinear
                .cmp(&b.collinear)
                .then_with(|| a.dist_sq.total_cmp(&b.dist_sq))
                .then_with(|| a.at.x.cmp(&b.at.x))
                .then_with(|| a.at.y.cmp(&b.at.y))
        });
        for candidate in bucket.iter() {
            let wp = candidate.waypoint;
            if out
                .iter()
                .any(|existing| existing.expected_snap == wp.expected_snap)
            {
                continue;
            }
            out.push(wp);
            if out.len() >= MAX_WAYPOINTS_PER_CLASS {
                break;
            }
        }
    }

    // 3. Round-robin interleave across snap classes so adjacent frames switch classes and snaps.
    let mut waypoints: Vec<WireWaypoint> = Vec::with_capacity(5 * MAX_WAYPOINTS_PER_CLASS);
    let max_len = trimmed.iter().map(Vec::len).max().unwrap_or(0);
    for idx in 0..max_len {
        for bucket in &trimmed {
            if let Some(&wp) = bucket.get(idx)
                && waypoints
                    .last()
                    .is_none_or(|prev| prev.expected_snap != wp.expected_snap)
            {
                waypoints.push(wp);
            }
        }
    }
    while waypoints.len() > 1
        && waypoints.last().unwrap().expected_snap == waypoints.first().unwrap().expected_snap
    {
        waypoints.pop();
    }

    if waypoints.len() < 4 {
        return Err(format!(
            "drag-wire found only {} distinct in-viewport waypoints (need at least 4)",
            waypoints.len()
        ));
    }
    if !waypoints.iter().any(|wp| wp.two_leg) {
        return Err("drag-wire found no non-collinear two-leg L-route waypoints".into());
    }

    Ok(WirePlan {
        start_screen,
        start_world,
        start_snap,
        waypoints,
    })
}

pub(crate) fn mouse_left_up(position: Point<Pixels>) -> PlatformInput {
    PlatformInput::MouseUp(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WirePhase {
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
                "drag-wire window did not settle into hovered fullscreen before wire start (fullscreen: {is_fullscreen}, hovered: {is_hovered})"
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

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WireSnapCounters {
    pub pin: u64,
    pub wire_end: u64,
    pub junction: u64,
    pub wire_body: u64,
    pub grid: u64,
    pub two_leg: u64,
}

impl WireSnapCounters {
    pub(crate) fn record(&mut self, target: Snap, has_second_leg: bool) {
        match target {
            Snap::Pin { .. } => self.pin += 1,
            Snap::WireEnd { .. } => self.wire_end += 1,
            Snap::Junction { .. } => self.junction += 1,
            Snap::WireBody { .. } => self.wire_body += 1,
            Snap::Grid(_) => self.grid += 1,
        }
        if has_second_leg {
            self.two_leg += 1;
        }
    }

    pub(crate) fn to_json(self) -> Value {
        json!({
            "pin": self.pin,
            "wire_end": self.wire_end,
            "junction": self.junction,
            "wire_body": self.wire_body,
            "grid": self.grid,
        })
    }
}

struct DragWireDriver {
    config: RunConfig,
    expect_fullscreen: bool,
    doc: Entity<Doc>,
    overlay: Entity<OverlayView>,
    scene_details: Value,
    phase: WirePhase,
    plan: Option<WirePlan>,
    step: u32,
    draws_before_warmup: u64,
    baseline: Option<FrameDurationSnapshot>,
    doc_revision_start: u64,
    sheet_paints_before_press: u64,
    press_sheet_paints: u64,
    sheet_paints_after_press: u64,
    warmup_sheet_paints: u64,
    sheet_paints_start: u64,
    overlay_paints_start: u64,
    previous_counts: AllocCounts,
    allocs: Vec<AllocCounts>,
    wire_transitions: u64,
    wire_mismatches: u64,
    revision_changes: u64,
    snap_counters: WireSnapCounters,
    last_snap: Option<Snap>,
    expected_snap: Option<Snap>,
    started: Instant,
    trace: Option<FrameTrace>,
    outcome: Rc<RefCell<Option<Result<Value, String>>>>,
}

fn active_wire_state(view: &OverlayView, expected_start: Pt) -> Option<(Option<Snap>, bool)> {
    match view.tool_state() {
        ToolState::Wiring(wire) if wire.start == Some(expected_start) => {
            Some((wire.snap, wire.preview_wires()[1].is_some()))
        }
        _ => None,
    }
}

impl DragWireDriver {
    fn fail(&mut self, error: String, window: &mut Window) -> bool {
        *self.outcome.borrow_mut() = Some(Err(error));
        window.on_next_frame(|_, cx| cx.quit());
        false
    }

    fn step(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if let WirePhase::Settle { waited, ready } = self.phase {
            match advance_settle(
                waited,
                ready,
                self.expect_fullscreen,
                window.is_fullscreen(),
                window.is_window_hovered(),
            ) {
                Ok(SettleStep::Wait { waited, ready }) => {
                    self.phase = WirePhase::Settle { waited, ready };
                    window.refresh();
                    return true;
                }
                Ok(SettleStep::Press) => {
                    self.phase = WirePhase::Press;
                }
                Err(error) => return self.fail(error, window),
            }
        }

        if self.phase == WirePhase::Press {
            let camera = self.overlay.read(cx).camera().clone();
            let viewport = self.overlay.read(cx).viewport();
            let plan = match build_wire_plan(self.doc.read(cx), &camera, viewport) {
                Ok(plan) => plan,
                Err(error) => return self.fail(error, window),
            };
            self.doc_revision_start = self.doc.read(cx).revision();
            self.sheet_paints_before_press = self.overlay.read(cx).sheet_paint_count(cx);
            self.overlay.update(cx, |view, cx| {
                view.set_tool(Tool::Wire, cx);
            });
            window.dispatch_event(mouse_left_down(plan.start_screen), cx);
            window.dispatch_event(mouse_left_up(plan.start_screen), cx);
            let started_ok = matches!(
                self.overlay.read(cx).tool_state(),
                ToolState::Wiring(wire) if wire.start == Some(plan.start_snap.at())
            );
            if !started_ok {
                return self.fail(
                    format!(
                        "drag-wire mouse-down at {:?} did not anchor wire start at {:?}",
                        plan.start_screen,
                        plan.start_snap.at()
                    ),
                    window,
                );
            }
            self.plan = Some(plan);
            self.phase = WirePhase::Move;
            // Wait one frame so GPUI draws the Press frame before warm-up begins.
            return true;
        }

        let plan = self.plan.as_ref().expect("wire plan initialized in Press");
        let start_pt = plan.start_snap.at();
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
            self.sheet_paints_after_press = sheet_paints_now;
        }

        if step == warmup {
            self.baseline = Some(window.frame_duration_snapshot());
            let view = self.overlay.read(cx);
            let sheet_paints_now = view.sheet_paint_count(cx);
            self.warmup_sheet_paints =
                sheet_paints_now.saturating_sub(self.sheet_paints_after_press);
            self.sheet_paints_start = sheet_paints_now;
            self.overlay_paints_start = view.overlay_paint_count();
            let Some((current_snap, _)) = active_wire_state(view, start_pt) else {
                return self.fail(
                    format!(
                        "drag-wire left active Wiring state before measured frames began (state: {:?}, fullscreen: {}, hovered: {}, active: {})",
                        view.tool_state(),
                        window.is_fullscreen(),
                        window.is_window_hovered(),
                        window.is_window_active(),
                    ),
                    window,
                );
            };
            self.last_snap = current_snap;
            if self.doc.read(cx).revision() != self.doc_revision_start {
                return self.fail(
                    "INV5: document revision changed during drag-wire warm-up".into(),
                    window,
                );
            }
            self.previous_counts = AllocCounts::now();
        } else if step > warmup && step <= last {
            let counts = AllocCounts::now();
            self.allocs.push(counts.since(self.previous_counts));
            self.previous_counts = counts;
            let view = self.overlay.read(cx);
            match active_wire_state(view, start_pt) {
                Some((Some(current_snap), has_second_leg)) => {
                    if Some(current_snap) != self.expected_snap {
                        self.wire_mismatches += 1;
                    }
                    if Some(current_snap) != self.last_snap {
                        self.wire_transitions += 1;
                        self.last_snap = Some(current_snap);
                    }
                    self.snap_counters.record(current_snap, has_second_leg);
                }
                _ => {
                    self.wire_mismatches += 1;
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
        self.expected_snap = Some(wp.expected_snap);
        window.dispatch_event(mouse_move(wp.screen), cx);
        self.step += 1;
        true
    }

    fn report(&mut self, window: &mut Window, cx: &App) -> Result<Value, String> {
        let base = self.baseline.as_ref().ok_or("no baseline snapshot")?;
        let plan = self.plan.as_ref().ok_or("no wire plan")?;
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
                "INV11: SheetView painted {} times during wire press, expected 0",
                self.press_sheet_paints
            ));
        }
        if self.warmup_sheet_paints != 0 {
            extra_warnings.push(format!(
                "INV11: SheetView painted {} times during wire warm-up, expected 0",
                self.warmup_sheet_paints
            ));
        }
        if self.wire_mismatches > 0 {
            extra_warnings.push(format!(
                "wire snap mismatch on {} of {} frames",
                self.wire_mismatches, self.config.frames
            ));
        }
        if self.wire_transitions != u64::from(self.config.frames) {
            extra_warnings.push(format!(
                "wire snap transitioned {} times, expected {}",
                self.wire_transitions, self.config.frames
            ));
        }
        if self.revision_changes > 0 {
            extra_warnings.push(format!(
                "INV5: document revision changed on {} measured frames during drag-wire",
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
        let start = plan.start_snap.at();
        report["wire"] = json!({
            "start": [start.x, start.y],
            "overlay_paints": overlay_paints,
            "sheet_paints": sheet_paints,
            "press_sheet_paints": self.press_sheet_paints,
            "warmup_sheet_paints": self.warmup_sheet_paints,
            "transitions": self.wire_transitions,
            "mismatches": self.wire_mismatches,
            "revision_changes": self.revision_changes,
            "waypoints": plan.waypoints.len(),
            "two_leg_frames": self.snap_counters.two_leg,
            "snap_classes": self.snap_counters.to_json(),
        });
        Ok(report)
    }
}

fn schedule_wire_step(window: &Window, driver: Rc<RefCell<DragWireDriver>>) {
    window.on_next_frame(move |window, cx| {
        let keep_going = driver.borrow_mut().step(window, cx);
        if keep_going {
            schedule_wire_step(window, driver);
        }
    });
}

struct DragWireRootView {
    overlay: Entity<OverlayView>,
    driver: Rc<RefCell<DragWireDriver>>,
    started: bool,
}

impl Render for DragWireRootView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if !self.started {
            self.started = true;
            let driver = self.driver.clone();
            window.on_next_frame(move |window, _| {
                // Let the platform draw the configured window before starting the wire tool.
                window.refresh();
                schedule_wire_step(window, driver);
            });
        }
        self.overlay.clone()
    }
}

pub fn run_drag_wire(config: RunConfig) -> Result<Value, String> {
    let Sheet::Stress(stress) = config.sheet else {
        return Err(
            "--script drag-wire needs a generated document (100, 1k, 10k, dense, or text)".into(),
        );
    };
    debug_assert_eq!(config.script, Script::DragWire);
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
            // T4.3/T5.2: focus OverlayView at open so wire input never triggers a focus-transfer refresh.
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            let trace = tracing.then(|| FrameTrace::new(cx));
            let expect_fullscreen = fullscreen_requested();
            let driver = Rc::new(RefCell::new(DragWireDriver {
                config,
                expect_fullscreen,
                doc,
                overlay: overlay.clone(),
                scene_details,
                phase: WirePhase::Settle {
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
                sheet_paints_after_press: 0,
                warmup_sheet_paints: 0,
                sheet_paints_start: 0,
                overlay_paints_start: 0,
                previous_counts: AllocCounts::now(),
                allocs: Vec::with_capacity(frames),
                wire_transitions: 0,
                wire_mismatches: 0,
                revision_changes: 0,
                snap_counters: WireSnapCounters::default(),
                last_snap: None,
                expected_snap: None,
                started: Instant::now(),
                trace,
                outcome,
            }));
            cx.new(|_| DragWireRootView {
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
    use omnia_canvas::{
        selection::hit_tolerance_nm,
        tool::{PointerRadii, ToolMachine},
    };

    #[test]
    fn build_wire_plan_succeeds_for_every_stress_sheet() {
        for sheet in StressSheet::ALL {
            let doc = generate(sheet, STRESS_SEED);
            for (w, h) in [(1920.0_f32, 1080.0_f32), (1536.0, 864.0), (1280.0, 800.0)] {
                let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(w), px(h)));
                let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
                let plan = build_wire_plan(&doc, &camera, bounds)
                    .unwrap_or_else(|e| panic!("{sheet:?} at {w}x{h}: {e}"));
                assert!(plan.waypoints.len() >= 4);
                let start = plan.start_snap.at();
                assert!(plan.waypoints.iter().any(|wp| wp.two_leg));
                for wp in &plan.waypoints {
                    assert_ne!(wp.expected_snap.at(), start);
                }
                for pair in plan.waypoints.windows(2) {
                    assert_ne!(pair[0].expected_snap, pair[1].expected_snap);
                }
                assert_ne!(
                    plan.waypoints.last().unwrap().expected_snap,
                    plan.waypoints.first().unwrap().expected_snap
                );
                if sheet == StressSheet::DenseWiring {
                    assert!(
                        plan.waypoints
                            .iter()
                            .any(|wp| matches!(wp.expected_snap, Snap::Junction { .. })),
                        "dense sheet should exercise Snap::Junction at {w}x{h}"
                    );
                }
                assert!(
                    plan.waypoints
                        .iter()
                        .any(|wp| matches!(wp.expected_snap, Snap::Pin { .. })),
                    "{sheet:?} should exercise Snap::Pin at {w}x{h}"
                );
                assert!(
                    plan.waypoints
                        .iter()
                        .any(|wp| matches!(wp.expected_snap, Snap::Grid(_))),
                    "{sheet:?} should exercise Snap::Grid at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn wire_pointer_move_makes_zero_heap_allocations() {
        for sheet in [StressSheet::Parts1k, StressSheet::DenseWiring] {
            let doc = generate(sheet, STRESS_SEED);
            let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1536.0), px(864.0)));
            let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
            let plan = build_wire_plan(&doc, &camera, bounds).expect("wire plan");
            let mut machine = ToolMachine::new();
            machine.set_tool(Tool::Wire);
            let radii = PointerRadii {
                hit: hit_tolerance_nm(&camera),
                snap: snap_radius_nm(&camera),
            };
            let down =
                machine.pointer_down_with_radii(&doc, 1, plan.start_world, radii, &[], false);
            assert!(down.handled);
            assert!(down.commit.is_none());
            // Warm one full pass so `ToolMachine`'s internal query scratch buffer reaches peak
            // capacity across the trajectory (matching the benchmark's warm-up phase, B10).
            for wp in &plan.waypoints {
                let out = machine.pointer_move_with_snap(&doc, wp.world, radii.snap, false);
                assert!(out.preview_changed);
            }
            let (_, allocs) = crate::count_allocs(|| {
                for wp in &plan.waypoints {
                    let out = machine.pointer_move_with_snap(&doc, wp.world, radii.snap, false);
                    assert!(out.preview_changed);
                }
            });
            assert_eq!(
                allocs, 0,
                "B10: ToolMachine::pointer_move_with_snap must not allocate during drag-wire on {sheet:?}"
            );
        }
    }

    #[test]
    fn settle_waits_for_hovered_fullscreen_then_counts_post_ready_frames() {
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
        assert_eq!(
            advance_settle(2, 3, true, true, false).unwrap(),
            SettleStep::Wait {
                waited: 3,
                ready: 0,
            }
        );
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
    fn headless_drag_wire_inv5_inv11_b10_on_100_and_dense() {
        for sheet in [StressSheet::Parts100, StressSheet::DenseWiring] {
            let config = RunConfig {
                sheet: Sheet::Stress(sheet),
                script: Script::DragWire,
                frames: 30,
                warmup: 30,
                size: (1280.0, 800.0),
                instrument: None,
            };
            let report = crate::scripts::run_headless(config)
                .unwrap_or_else(|e| panic!("headless drag-wire on {sheet:?}: {e}"));
            assert_eq!(report["config"]["script"], "drag-wire");
            assert_eq!(report["metrics"]["sheet_paints"], 0);
            assert_eq!(report["metrics"]["allocs_per_frame"]["ours"]["max"], 0.0);
            assert_eq!(report["wire"]["press_sheet_paints"], 0);
            assert_eq!(report["wire"]["warmup_sheet_paints"], 0);
            assert_eq!(report["wire"]["sheet_paints"], 0);
            assert_eq!(report["wire"]["overlay_paints"], 30);
            assert_eq!(report["wire"]["transitions"], 30);
            assert_eq!(report["wire"]["mismatches"], 0);
            assert_eq!(report["wire"]["revision_changes"], 0);
            assert!(report["wire"]["two_leg_frames"].as_u64().unwrap() > 0);
        }
    }
}
