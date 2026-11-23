//! T2.6 hosted check: drive the real sheet through GPUI's test platform.
//! This reports deterministic counts only. Reference timing uses the native window.

use crate::alloc::{AllocCounts, ours};
use crate::compare::SCHEMA;
use crate::driver::Script;
use crate::generate::generate;
use crate::harness::{RunConfig, primitive_counts};
use crate::hover::{build_hover_targets, mouse_move};
use crate::scene::{BenchScene, STRESS_SEED, SceneElement, Sheet};
use crate::stats::Summary;
use gpui::{
    AppContext, Bounds, Context, IntoElement, Render, TestApp, Window, WindowBounds, WindowOptions,
    point, px, size,
};
use omnia_canvas::{Camera as SheetCamera, OverlayView};
use omnia_doc::Pt;
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
            let overlay = cx.new(|cx| OverlayView::new(doc, camera, &GRAPHITE, cx));
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
    for frame_index in 0..end {
        if frame_index == config.warmup {
            (sheet_paints_start, overlay_paints_start) = window.update(|_, _, cx| {
                let overlay = overlay_entity.read(cx);
                (overlay.sheet_paint_count(cx), overlay.overlay_paint_count())
            });
        }
        let target = ours(|| targets[(frame_index as usize) % targets.len()]);
        let before = AllocCounts::now();
        window.update(|_, window, cx| {
            window.dispatch_event(mouse_move(target.screen), cx);
        });
        window.draw();
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
