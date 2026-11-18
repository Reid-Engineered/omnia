//! T2.6 hosted check: drive the real sheet through GPUI's test platform.
//! This reports deterministic counts only. Reference timing uses the native window.

use crate::alloc::{AllocCounts, ours};
use crate::compare::SCHEMA;
use crate::driver::Script;
use crate::harness::{RunConfig, primitive_counts};
use crate::scene::{BenchScene, SceneElement, Sheet};
use crate::stats::Summary;
use gpui::{
    Bounds, Context, IntoElement, Render, TestApp, Window, WindowBounds, WindowOptions, point, px,
    size,
};
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

/// Draw a fixed script and report the peak primitive count of each kind and per-frame allocations.
pub fn run_headless(config: RunConfig) -> Result<Value, String> {
    if !matches!(config.sheet, Sheet::Stress(_)) {
        return Err("--headless needs a generated document (100, 1k, 10k, dense, or text)".into());
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
                let peak = peak_primitives.entry(name.clone()).or_default();
                *peak = (*peak).max(count.as_u64().expect("primitive count is an integer"));
            }
        }
    }
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
            "primitives": peak_primitives,
        },
    }))
}
