//! Run the camera scripts against the production document sheet (T2.6).

use crate::alloc::{not_ours, ours};
use crate::driver::Camera;
use crate::generate::{StressSheet, generate};
use crate::scene::BenchScene;
use gpui::{App, AppContext, Bounds, Element, Pixels, Window, point};
use omnia_canvas::{Camera as SheetCamera, SheetView, sheet::SheetDocument};
use omnia_doc::Pt;

const SEED: u64 = 42;

pub(crate) struct DocumentScene {
    sheet: StressSheet,
    document: Option<SheetDocument>,
}

impl DocumentScene {
    pub(crate) fn new(sheet: StressSheet) -> Self {
        Self {
            sheet,
            document: None,
        }
    }
}

impl BenchScene for DocumentScene {
    fn paint(&mut self, bounds: Bounds<Pixels>, camera: Camera, window: &mut Window, cx: &mut App) {
        let document = self.document.get_or_insert_with(|| {
            // First-frame setup precedes measured paint. Keep the live doc and renderer caches
            // across frames, exactly as the application does.
            not_ours(|| SheetDocument::new(cx.new(|_| generate(self.sheet, SEED))))
        });
        let (dx, dy) = camera.logical(window.scale_factor());
        let mut camera_view = SheetCamera::new(Pt::ZERO, camera.zoom, bounds);
        camera_view.pan_by(point(-f64::from(dx), -f64::from(dy)));
        SheetView::with_document(camera_view, &omnia_theme::GRAPHITE, document.clone())
            .with_paint_scopes(|paint| ours(paint), |paint| not_ours(paint))
            .paint(None, None, bounds, &mut (), &mut (), window, cx);
    }
}

#[cfg(feature = "headless")]
mod headless {
    use super::*;
    use crate::alloc::AllocCounts;
    use crate::compare::SCHEMA;
    use crate::harness::{RunConfig, primitive_counts};
    use crate::scene::{SceneElement, Sheet};
    use crate::stats::Summary;
    use gpui::{Context, HeadlessAppContext, IntoElement, NoopTextSystem, Render, px, size};
    use serde_json::{Value, json};
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    const SCALE: f32 = 1.0;

    struct ScriptView {
        scene: Rc<RefCell<dyn BenchScene>>,
        camera: Rc<Cell<Camera>>,
        paint_time: Rc<Cell<Option<Duration>>>,
    }

    impl Render for ScriptView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            SceneElement {
                scene: self.scene.clone(),
                camera: self.camera.get(),
                paint_time: self.paint_time.clone(),
            }
        }
    }

    /// Hosted CI runs the same scene and camera driver against GPUI's in-memory platform.
    /// Without presentation, the report contains only deterministic counts.
    pub fn run_headless(config: RunConfig) -> Result<Value, String> {
        if !matches!(config.sheet, Sheet::Document(_)) || config.warmup == 0 || config.frames == 0 {
            return Err("headless runs require a generated sheet and positive frame counts".into());
        }
        let mut app = HeadlessAppContext::new(Arc::new(NoopTextSystem));
        let camera = Rc::new(Cell::new(Camera::default()));
        let paint_time = Rc::new(Cell::new(None));
        let window = app
            .open_window(size(px(config.size.0), px(config.size.1)), |window, cx| {
                window.set_scale_factor(SCALE);
                cx.new(|_| ScriptView {
                    scene: config.sheet.scene().expect("document scene"),
                    camera: camera.clone(),
                    paint_time: paint_time.clone(),
                })
            })
            .map_err(|e| e.to_string())?;

        let mut allocations = Vec::with_capacity(config.frames as usize);
        let mut counts = BTreeMap::<String, u64>::new();
        // Measured frames continue the camera path past warm-up, as the windowed harness does
        // (harness.rs), so an allocation made only at a camera position not yet drawn is
        // counted. The schedule stays fixed: independent of focus, display scale, refresh rate
        // and GPU.
        for (measuring, frames) in [(false, config.warmup), (true, config.frames)] {
            for frame in 0..frames {
                let step = if measuring {
                    config.warmup + frame
                } else {
                    frame
                };
                let before = AllocCounts::now();
                camera.set(ours(|| config.script.camera(step)));
                paint_time.set(None);
                app.update_window(window.into(), |_, window, cx| {
                    window.refresh();
                    window.draw(cx).clear(cx);
                    if measuring {
                        allocations.push(AllocCounts::now().since(before).ours_allocs as f64);
                        let prims = primitive_counts(window);
                        let mut total = 0;
                        for (kind, value) in prims.as_object().expect("primitive counts") {
                            if kind == "batches" || kind == "paint_operations" {
                                continue;
                            }
                            let n = value.as_u64().expect("integer primitive count");
                            *counts.entry(kind.clone()).or_default() += n;
                            total += n;
                        }
                        *counts.entry("total".into()).or_default() += total;
                    }
                })
                .map_err(|e| e.to_string())?;
                if paint_time.get().is_none() {
                    return Err(format!("headless frame {frame} did not paint the sheet"));
                }
            }
        }

        Ok(json!({
            "schema": SCHEMA,
            "kind": "run",
            "config": {
                "backend": "headless",
                "sheet": config.sheet.name(),
                "seed": SEED,
                "script": config.script.name(),
                "frames": config.frames,
                "warmup": config.warmup,
                "window_logical": [config.size.0, config.size.1],
                "scale_factor": SCALE,
            },
            "metrics": {
                "allocs_per_frame": {
                    "ours": Summary::from_samples(&mut allocations)
                        .expect("positive measured frame count")
                        .to_json(),
                },
                "primitives": counts,
            },
        }))
    }
}

#[cfg(feature = "headless")]
pub use headless::run_headless;
