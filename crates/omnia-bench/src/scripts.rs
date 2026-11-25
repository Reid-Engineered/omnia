//! T2.6 hosted check: drive the real sheet through GPUI's test platform.
//! This reports deterministic counts only. Reference timing uses the native window.

use crate::alloc::{AllocCounts, ours};
use crate::compare::SCHEMA;
use crate::drag_part::{build_drag_plan, mouse_left_down, mouse_left_drag};
use crate::driver::Script;
use crate::generate::generate;
use crate::harness::{RunConfig, primitive_counts};
use crate::hover::{BenchOverlayScope, build_hover_targets, mouse_move};
use crate::scene::{BenchScene, STRESS_SEED, SceneElement, Sheet};
use crate::stats::Summary;
use gpui::{
    AppContext, Bounds, Context, IntoElement, Render, TestApp, Window, WindowBounds, WindowOptions,
    point, px, size,
};
use omnia_canvas::{Camera as SheetCamera, OverlayView, tool::ToolState};
use omnia_doc::{ItemId, Pt};
use omnia_theme::GRAPHITE;
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

struct ScriptView {
    script: Script,
    frame: Rc<Cell<u32>>,
    scene: Rc<RefCell<dyn BenchScene>>,
    paint_time: Rc<Cell<Option<Duration>>>,
}

impl Render for ScriptView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        SceneElement {
            scene: self.scene.clone(),
            camera: ours(|| self.script.camera(self.frame.get())),
            paint_time: self.paint_time.clone(),
        }
    }
}

/// Draw a fixed script and report peak and sweep-wide primitive counts and per-frame allocations.
pub fn run_headless(config: RunConfig) -> Result<Value, String> {
    let Sheet::Stress(stress) = config.sheet else {
        return Err("--headless needs a generated document (100, 1k, 10k, dense, or text)".into());
    };
    if config.script == Script::Hover {
        return run_headless_hover(config, stress);
    }
    if config.script == Script::DragPart {
        return run_headless_drag_part(config, stress);
    }
    let end = config
        .warmup
        .checked_add(config.frames)
        .filter(|_| config.warmup > 0 && config.frames > 0)
        .ok_or("headless warmup and frames must be positive and fit in u32")?;
    let scene = config.sheet.scene().expect("generated sheet has a scene");
    let scene_details = scene.borrow().details();
    let frame = Rc::new(Cell::new(0));
    let paint_time = Rc::new(Cell::new(None));
    let mut app = TestApp::new();
    let mut window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(0.), px(0.)),
                size(px(config.size.0), px(config.size.1)),
            ))),
            ..Default::default()
        },
        |_, _| ScriptView {
            script: config.script,
            frame: frame.clone(),
            scene,
            paint_time: paint_time.clone(),
        },
    );
    let scale_factor = window.update(|_, window, _| window.scale_factor());
    let mut ours_per_frame = Vec::with_capacity(config.frames as usize);
    let mut peak_primitives = BTreeMap::<String, u64>::new();
    let mut sweep_primitives = BTreeMap::<String, u64>::new();
    for frame_index in 0..end {
        frame.set(frame_index);
        paint_time.set(None);
        let before = AllocCounts::now();
        window.draw();
        if paint_time.get().is_none() {
            return Err(format!("headless frame {frame_index} did not paint"));
        }
        if frame_index >= config.warmup {
            ours_per_frame.push(AllocCounts::now().since(before).ours_allocs as f64);
            let counts = window.update(|_, window, _| primitive_counts(window));
            for (name, count) in counts.as_object().expect("primitive counts are an object") {
                let count = count.as_u64().expect("primitive count is an integer");
                if name != "batches" && name != "paint_operations" {
                    *sweep_primitives.entry(name.clone()).or_default() += count;
                }
                let peak = peak_primitives.entry(name.clone()).or_default();
                *peak = (*peak).max(count);
            }
        }
    }
    let sweep_total: u64 = sweep_primitives.values().sum();
    Ok(json!({
        "schema": SCHEMA,
        "kind": "headless-run",
        "config": {
            "sheet": config.sheet.name(),
            "script": config.script.name(),
            "frames": config.frames,
            "warmup": config.warmup,
            "window_logical": [config.size.0, config.size.1],
            "scale_factor": scale_factor,
        },
        "environment": { "platform": "gpui-test", "os": std::env::consts::OS },
        "scene_details": scene_details,
        "metrics": {
            "allocs_per_frame": { "ours": Summary::from_samples(&mut ours_per_frame).unwrap().to_json() },
            "primitives": {
                "peak": peak_primitives,
                "sweep": sweep_primitives,
                "sweep_total": sweep_total,
            },
        },
    }))
}

struct HeadlessHoverView {
    overlay: gpui::Entity<OverlayView>,
}

impl Render for HeadlessHoverView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.overlay.clone()
    }
}

fn run_headless_hover(
    config: RunConfig,
    stress: crate::generate::StressSheet,
) -> Result<Value, String> {
    let end = config
        .warmup
        .checked_add(config.frames)
        .filter(|_| config.warmup > 0 && config.frames > 0)
        .ok_or("headless warmup and frames must be positive and fit in u32")?;
    let (parts, wires) = stress.counts();
    let scene_details = json!({ "seed": STRESS_SEED, "parts": parts, "wires": wires });
    let mut app = TestApp::new();
    let bounds = Bounds::new(
        point(px(0.), px(0.)),
        size(px(config.size.0), px(config.size.1)),
    );
    let mut doc_entity = None;
    let mut overlay_entity = None;
    let mut window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |window, cx| {
            let doc = cx.new(|_| generate(stress, STRESS_SEED));
            doc_entity = Some(doc.clone());
            let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
            let overlay = cx.new(|cx| {
                OverlayView::new(doc, camera, &GRAPHITE, cx)
                    .with_paint_scope(Rc::new(BenchOverlayScope))
            });
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            overlay_entity = Some(overlay.clone());
            HeadlessHoverView { overlay }
        },
    );
    window.draw();
    let doc_entity = doc_entity.expect("doc initialized");
    let overlay_entity = overlay_entity.expect("overlay initialized");
    let targets = window.update(|_, _, cx| {
        let overlay = overlay_entity.read(cx);
        let camera = overlay.camera().clone();
        let viewport = overlay.viewport();
        build_hover_targets(doc_entity.read(cx), &camera, viewport)
    })?;
    let scale_factor = window.update(|_, window, _| window.scale_factor());
    let mut ours_per_frame = Vec::with_capacity(config.frames as usize);
    let mut peak_primitives = BTreeMap::<String, u64>::new();
    let mut sweep_primitives = BTreeMap::<String, u64>::new();
    let mut sheet_paints_start = 0u64;
    let mut overlay_paints_start = 0u64;
    let mut hover_transitions = 0u64;
    let mut hover_mismatches = 0u64;
    let mut net_highlight_frames = 0u64;
    let mut last_hovered = None;
    for frame_index in 0..end {
        if frame_index == config.warmup {
            (sheet_paints_start, overlay_paints_start, last_hovered) = window.update(|_, _, cx| {
                let overlay = overlay_entity.read(cx);
                (
                    overlay.sheet_paint_count(cx),
                    overlay.overlay_paint_count(),
                    overlay.hovered(),
                )
            });
        }
        let target = targets[(frame_index as usize) % targets.len()];
        let before = AllocCounts::now();
        window.update(|_, window, cx| {
            window.dispatch_event(mouse_move(target.screen), cx);
        });
        window.draw();
        if frame_index >= config.warmup {
            ours_per_frame.push(AllocCounts::now().since(before).ours_allocs as f64);
            let (hovered, has_net) = window.update(|_, _, cx| {
                let overlay = overlay_entity.read(cx);
                (overlay.hovered(), !overlay.highlighted_wires().is_empty())
            });
            if hovered != target.expected {
                hover_mismatches += 1;
            }
            if hovered != last_hovered {
                hover_transitions += 1;
                last_hovered = hovered;
            }
            if has_net {
                net_highlight_frames += 1;
            }
            let counts = window.update(|_, window, _| primitive_counts(window));
            for (name, count) in counts.as_object().expect("primitive counts are an object") {
                let count = count.as_u64().expect("primitive count is an integer");
                if name != "batches" && name != "paint_operations" {
                    *sweep_primitives.entry(name.clone()).or_default() += count;
                }
                let peak = peak_primitives.entry(name.clone()).or_default();
                *peak = (*peak).max(count);
            }
        }
    }
    let (sheet_paints_end, overlay_paints_end) = window.update(|_, _, cx| {
        let overlay = overlay_entity.read(cx);
        (overlay.sheet_paint_count(cx), overlay.overlay_paint_count())
    });
    let sheet_paints = sheet_paints_end.saturating_sub(sheet_paints_start);
    let overlay_paints = overlay_paints_end.saturating_sub(overlay_paints_start);
    let sweep_total: u64 = sweep_primitives.values().sum();
    Ok(json!({
        "schema": SCHEMA,
        "kind": "headless-run",
        "config": {
            "sheet": config.sheet.name(),
            "script": config.script.name(),
            "frames": config.frames,
            "warmup": config.warmup,
            "window_logical": [config.size.0, config.size.1],
            "scale_factor": scale_factor,
        },
        "environment": { "platform": "gpui-test", "os": std::env::consts::OS },
        "scene_details": scene_details,
        "hover": {
            "overlay_paints": overlay_paints,
            "sheet_paints": sheet_paints,
            "transitions": hover_transitions,
            "mismatches": hover_mismatches,
            "net_highlight_frames": net_highlight_frames,
            "sweep_targets": targets.len(),
        },
        "metrics": {
            "sheet_paints": sheet_paints,
            "allocs_per_frame": { "ours": Summary::from_samples(&mut ours_per_frame).unwrap().to_json() },
            "primitives": {
                "peak": peak_primitives,
                "sweep": sweep_primitives,
                "sweep_total": sweep_total,
            },
        },
    }))
}

fn run_headless_drag_part(
    config: RunConfig,
    stress: crate::generate::StressSheet,
) -> Result<Value, String> {
    let end = config
        .warmup
        .checked_add(config.frames)
        .filter(|_| config.warmup > 0 && config.frames > 0)
        .ok_or("headless warmup and frames must be positive and fit in u32")?;
    let (parts, wires) = stress.counts();
    let scene_details = json!({ "seed": STRESS_SEED, "parts": parts, "wires": wires });
    let mut app = TestApp::new();
    let bounds = Bounds::new(
        point(px(0.), px(0.)),
        size(px(config.size.0), px(config.size.1)),
    );
    let mut doc_entity = None;
    let mut overlay_entity = None;
    let mut window = app.open_window_with_options(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |window, cx| {
            let doc = cx.new(|_| generate(stress, STRESS_SEED));
            doc_entity = Some(doc.clone());
            let camera = SheetCamera::new(Pt::ZERO, 1.0, bounds);
            let overlay = cx.new(|cx| {
                OverlayView::new(doc, camera, &GRAPHITE, cx)
                    .with_paint_scope(Rc::new(BenchOverlayScope))
            });
            let focus = overlay.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
            overlay_entity = Some(overlay.clone());
            HeadlessHoverView { overlay }
        },
    );
    window.draw();
    let doc_entity = doc_entity.expect("doc initialized");
    let overlay_entity = overlay_entity.expect("overlay initialized");
    let plan = window.update(|_, _, cx| {
        let overlay = overlay_entity.read(cx);
        let camera = overlay.camera().clone();
        let viewport = overlay.viewport();
        build_drag_plan(doc_entity.read(cx), &camera, viewport)
    })?;
    let doc_revision_start = window.update(|_, _, cx| doc_entity.read(cx).revision());
    let sheet_paints_before_press =
        window.update(|_, _, cx| overlay_entity.read(cx).sheet_paint_count(cx));
    window.update(|_, window, cx| {
        window.dispatch_event(mouse_left_down(plan.anchor_screen), cx);
    });
    let started_ok = window.update(|_, _, cx| {
        matches!(
            overlay_entity.read(cx).tool_state(),
            ToolState::Dragging(drag) if drag.pressed == ItemId::Part(plan.part_id)
        )
    });
    if !started_ok {
        return Err(format!(
            "headless drag-part mouse-down did not start dragging Part({})",
            plan.part_id.0
        ));
    }
    let sheet_paints_before_lift =
        window.update(|_, _, cx| overlay_entity.read(cx).sheet_paint_count(cx));
    let press_sheet_paints = sheet_paints_before_lift.saturating_sub(sheet_paints_before_press);
    let scale_factor = window.update(|_, window, _| window.scale_factor());
    let mut ours_per_frame = Vec::with_capacity(config.frames as usize);
    let mut peak_primitives = BTreeMap::<String, u64>::new();
    let mut sweep_primitives = BTreeMap::<String, u64>::new();
    let mut sheet_paints_start = 0u64;
    let mut lift_sheet_paints = 0u64;
    let mut overlay_paints_start = 0u64;
    let mut drag_transitions = 0u64;
    let mut drag_mismatches = 0u64;
    let mut revision_changes = 0u64;
    let mut last_delta = None;
    for frame_index in 0..end {
        if frame_index == config.warmup {
            let (sp, op, delta, rev) = window.update(|_, _, cx| {
                let overlay = overlay_entity.read(cx);
                let delta = match overlay.tool_state() {
                    ToolState::Dragging(drag)
                        if drag.is_lifted() && drag.pressed == ItemId::Part(plan.part_id) =>
                    {
                        Some(drag.delta())
                    }
                    _ => None,
                };
                (
                    overlay.sheet_paint_count(cx),
                    overlay.overlay_paint_count(),
                    delta,
                    doc_entity.read(cx).revision(),
                )
            });
            lift_sheet_paints = sp.saturating_sub(sheet_paints_before_lift);
            sheet_paints_start = sp;
            overlay_paints_start = op;
            last_delta = delta;
            if last_delta.is_none() {
                return Err(
                    "headless drag-part left lifted Dragging state before measured frames".into(),
                );
            }
            if rev != doc_revision_start {
                return Err(
                    "INV5: document revision changed during headless drag-part warm-up".into(),
                );
            }
        }
        let wp = plan.waypoints[(frame_index as usize) % plan.waypoints.len()];
        let before = AllocCounts::now();
        window.update(|_, window, cx| {
            window.dispatch_event(mouse_left_drag(wp.screen), cx);
        });
        if frame_index >= config.warmup {
            ours_per_frame.push(AllocCounts::now().since(before).ours_allocs as f64);
            let (delta, rev) = window.update(|_, _, cx| {
                let overlay = overlay_entity.read(cx);
                let delta = match overlay.tool_state() {
                    ToolState::Dragging(drag)
                        if drag.is_lifted() && drag.pressed == ItemId::Part(plan.part_id) =>
                    {
                        Some(drag.delta())
                    }
                    _ => None,
                };
                (delta, doc_entity.read(cx).revision())
            });
            if delta != Some(wp.expected_delta) {
                drag_mismatches += 1;
            }
            if delta.is_some() && delta != last_delta {
                drag_transitions += 1;
                last_delta = delta;
            }
            if rev != doc_revision_start {
                revision_changes += 1;
            }
            let counts = window.update(|_, window, _| primitive_counts(window));
            for (name, count) in counts.as_object().expect("primitive counts are an object") {
                let count = count.as_u64().expect("primitive count is an integer");
                if name != "batches" && name != "paint_operations" {
                    *sweep_primitives.entry(name.clone()).or_default() += count;
                }
                let peak = peak_primitives.entry(name.clone()).or_default();
                *peak = (*peak).max(count);
            }
        }
    }
    let (sheet_paints_end, overlay_paints_end) = window.update(|_, _, cx| {
        let overlay = overlay_entity.read(cx);
        (overlay.sheet_paint_count(cx), overlay.overlay_paint_count())
    });
    let sheet_paints = sheet_paints_end.saturating_sub(sheet_paints_start);
    let overlay_paints = overlay_paints_end.saturating_sub(overlay_paints_start);
    let sweep_total: u64 = sweep_primitives.values().sum();
    Ok(json!({
        "schema": SCHEMA,
        "kind": "headless-run",
        "config": {
            "sheet": config.sheet.name(),
            "script": config.script.name(),
            "frames": config.frames,
            "warmup": config.warmup,
            "window_logical": [config.size.0, config.size.1],
            "scale_factor": scale_factor,
        },
        "environment": { "platform": "gpui-test", "os": std::env::consts::OS },
        "scene_details": scene_details,
        "drag": {
            "part_id": plan.part_id.0,
            "overlay_paints": overlay_paints,
            "sheet_paints": sheet_paints,
            "press_sheet_paints": press_sheet_paints,
            "lift_sheet_paints": lift_sheet_paints,
            "transitions": drag_transitions,
            "mismatches": drag_mismatches,
            "revision_changes": revision_changes,
            "waypoints": plan.waypoints.len(),
        },
        "metrics": {
            "sheet_paints": sheet_paints,
            "allocs_per_frame": {
                "ours": Summary::from_samples(&mut ours_per_frame)
                    .ok_or("no allocation samples recorded")?
                    .to_json()
            },
            "primitives": {
                "peak": peak_primitives,
                "sweep": sweep_primitives,
                "sweep_total": sweep_total,
            },
        },
    }))
}
