//! U0.3 / US1 and US5: isolate window chrome from the workspace shell.

use super::SpikeError;
use crate::harness::{environment, watchdog};
use gpui::{
    App, AppContext as _, Bounds, Context, Decorations, Div, IntoElement, MouseButton, Pixels,
    Render, Rgba, TitlebarOptions, Window, WindowBounds, WindowControlArea, WindowDecorations,
    WindowOptions, anchored, deferred, div, point, prelude::*, px, size,
};
use gpui::{Point, ResizeEdge, Size};
use omnia_theme::GRAPHITE;
use serde_json::json;
use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

const BAR_HEIGHT: f32 = 36.;
const WINDOW_TITLE: &str = "Omnia — Window chrome spike";
const RESIZE_INSET: f32 = 6.;

pub(super) fn main(args: Vec<String>) -> Result<(), SpikeError> {
    let mut args = args.into_iter();
    let mut seconds = None;
    let mut png = None;
    while let Some(flag) = args.next() {
        let usage = |message: &str| SpikeError::Usage(format!("spike chrome: {message}"));
        match flag.as_str() {
            "--seconds" if seconds.is_none() => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("--seconds needs a value"))?;
                let number = value
                    .parse::<u64>()
                    .ok()
                    .filter(|number| (1..=3600).contains(number))
                    .ok_or_else(|| usage("--seconds must be an integer in 1..=3600"))?;
                seconds = Some(number);
            }
            "--png" if png.is_none() => {
                png = Some(PathBuf::from(
                    args.next().ok_or_else(|| usage("--png needs a path"))?,
                ));
            }
            _ => return Err(usage(&format!("unexpected argument {flag}"))),
        }
    }
    if png.is_some() && !cfg!(all(windows, feature = "readback")) {
        return Err(SpikeError::Usage(
            "spike chrome: --png requires Windows and --features readback".into(),
        ));
    }

    let duration = Duration::from_secs(seconds.unwrap_or(60));
    watchdog("spike chrome", duration + Duration::from_secs(30));
    let result = Rc::new(RefCell::new(None));
    let completed = result.clone();
    gpui_platform::application().run(move |cx: &mut App| {
        let capture = png.is_some();
        let requested = size(px(1440.), px(900.));
        let bounds = Bounds::centered(None, requested, cx);
        let options = WindowOptions {
            show: !capture,
            focus: !capture,
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                bounds.origin,
                requested,
            ))),
            window_min_size: capture.then_some(requested),
            titlebar: Some(TitlebarOptions {
                title: Some(WINDOW_TITLE.into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        let handle = match cx.open_window(options, |window, cx| {
            window.set_window_title(WINDOW_TITLE);
            if cfg!(windows) {
                window.resize(requested);
            }
            cx.new(|cx| Chrome::new(window, cx, capture))
        }) {
            Ok(handle) => handle,
            Err(error) => {
                *completed.borrow_mut() = Some(Err(format!("open window: {error}")));
                cx.quit();
                return;
            }
        };

        if !capture {
            cx.activate(true);
        }
        let closed = completed.clone();
        cx.on_window_closed(move |cx, _| {
            let mut result = closed.borrow_mut();
            if result.is_none() {
                println!("{}", json!({"event": "closed", "reason": "window control"}));
                *result = Some(Ok(()));
                cx.quit();
            }
        })
        .detach();

        cx.spawn(async move |cx| {
            let setup = cx.update(|cx| {
                cx.update_window(handle.into(), |_, window, cx| {
                    println!(
                        "{}",
                        json!({"event": "environment", "details": environment(window, cx)})
                    );
                    if let Some(path) = png {
                        capture_png(window, cx, path)?;
                    }
                    Ok(())
                })
            });
            let setup = setup
                .map_err(|error| error.to_string())
                .and_then(|outcome| outcome);
            if setup.is_ok() && !capture {
                cx.background_executor().timer(duration).await;
                println!("{}", json!({"event": "end", "reason": "timeout"}));
            }
            if completed.borrow().is_none() {
                *completed.borrow_mut() = Some(setup);
                cx.update(|cx| cx.quit());
            }
        })
        .detach();
    });
    result
        .take()
        .unwrap_or_else(|| Err("application quit without a completed spike".into()))
        .map_err(SpikeError::Run)
}

#[cfg(all(windows, feature = "readback"))]
fn capture_png(window: &mut Window, cx: &mut App, path: PathBuf) -> Result<(), String> {
    window.draw(cx).clear(cx);
    let image = window
        .render_to_image()
        .map_err(|error| error.to_string())?;
    image
        .save(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    println!(
        "{}",
        json!({"event": "readback", "path": path, "width": image.width(), "height": image.height()})
    );
    Ok(())
}

#[cfg(not(all(windows, feature = "readback")))]
fn capture_png(_: &mut Window, _: &mut App, _: PathBuf) -> Result<(), String> {
    Err("PNG capture requires Windows and --features readback".into())
}

struct Chrome {
    popover_open: bool,
    last_decorations: Option<Decorations>,
    last_bounds: Option<Bounds<Pixels>>,
}

impl Chrome {
    fn new(window: &mut Window, cx: &mut Context<Self>, popover_open: bool) -> Self {
        cx.observe_window_bounds(window, |chrome, window, cx| {
            chrome.report_geometry(window);
            cx.notify();
        })
        .detach();
        cx.observe_window_visibility(window, |_, visibility, _, _| {
            println!(
                "{}",
                json!({"event": "visibility", "state": format!("{visibility:?}")})
            );
        })
        .detach();
        window.on_window_should_close(cx, |_, _| {
            report_control("close", "native request");
            true
        });
        Self {
            popover_open,
            last_decorations: None,
            last_bounds: None,
        }
    }

    fn report_geometry(&mut self, window: &Window) {
        let decorations = window.window_decorations();
        if self.last_decorations != Some(decorations) {
            println!(
                "{}",
                json!({"event": "decorations", "granted": format!("{decorations:?}")})
            );
            self.last_decorations = Some(decorations);
        }
        let bounds = window.bounds();
        if self.last_bounds == Some(bounds) {
            return;
        }
        let moved = self
            .last_bounds
            .is_some_and(|old| old.origin != bounds.origin);
        let resized = self.last_bounds.is_some_and(|old| old.size != bounds.size);
        self.last_bounds = Some(bounds);
        println!(
            "{}",
            json!({
                "event": "bounds",
                "origin": [f32::from(bounds.origin.x), f32::from(bounds.origin.y)],
                "size": [f32::from(bounds.size.width), f32::from(bounds.size.height)],
                "moved": moved,
                "resized": resized,
                "maximized": window.is_maximized()
            })
        );
    }
}

impl Render for Chrome {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.report_geometry(window);
        let client_linux = cfg!(target_os = "linux")
            && matches!(window.window_decorations(), Decorations::Client { .. });
        let mut drag_area = div().h_full().flex_1();
        if cfg!(windows) {
            drag_area = drag_area.window_control_area(WindowControlArea::Drag);
        } else if client_linux {
            drag_area = drag_area.on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    report_control("maximize", "title double-click");
                    window.zoom_window();
                } else {
                    println!("{}", json!({"event": "move_requested"}));
                    window.start_window_move();
                }
            });
        }

        let bar = div()
            .h(px(BAR_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .bg(color(GRAPHITE.rail_shell))
            .text_color(color(GRAPHITE.text_primary))
            .child(div().px_3().child("Omnia"))
            .child(
                div()
                    .id("file-menu")
                    .h_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.popover_open = !this.popover_open;
                        cx.notify();
                    }))
                    .child("File"),
            )
            .child(drag_area)
            .when(cfg!(windows) || client_linux, |bar| {
                bar.child(window_control("−", "minimize", WindowControlArea::Min))
                    .child(window_control("□", "maximize", WindowControlArea::Max))
                    .child(window_control("×", "close", WindowControlArea::Close))
            });

        let root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(color(GRAPHITE.bg_backdrop))
            .child(bar)
            .child(
                div()
                    .p_4()
                    .text_color(color(GRAPHITE.text_secondary))
                    .child("Drag the title bar, resize the window, and try the window controls."),
            )
            .when(self.popover_open, |root| {
                root.child(deferred(
                    anchored().position(point(px(74.), px(BAR_HEIGHT))).child(
                        div()
                            .w(px(188.))
                            .flex()
                            .flex_col()
                            .p_2()
                            .gap_1()
                            .bg(color(GRAPHITE.bg_raised))
                            .text_color(color(GRAPHITE.text_primary))
                            .child(div().px_2().py_1().child("New schematic"))
                            .child(div().px_2().py_1().child("Open…")),
                    ),
                ))
            });

        if cfg!(target_os = "linux") {
            linux_resize(root, window, client_linux)
        } else {
            root
        }
    }
}

fn color(token: omnia_theme::Rgba) -> Rgba {
    Rgba {
        r: token.r,
        g: token.g,
        b: token.b,
        a: token.a,
    }
}

fn report_control(control: &str, source: &str) {
    println!(
        "{}",
        json!({"event": "control", "control": control, "source": source})
    );
}

fn window_control(label: &'static str, name: &'static str, area: WindowControlArea) -> Div {
    let control = div()
        .w(px(46.))
        .h_full()
        .flex()
        .items_center()
        .justify_center()
        .child(label);
    if cfg!(windows) {
        control.window_control_area(area)
    } else {
        control.on_mouse_down(MouseButton::Left, move |_, window, _| {
            report_control(name, "client request");
            match area {
                WindowControlArea::Min => window.minimize_window(),
                WindowControlArea::Max => window.zoom_window(),
                WindowControlArea::Close => window.remove_window(),
                WindowControlArea::Drag => unreachable!("drag has its own area"),
            }
        })
    }
}

fn linux_resize(root: Div, window: &mut Window, client: bool) -> Div {
    let inset = if client && !window.is_maximized() {
        px(RESIZE_INSET)
    } else {
        px(0.)
    };
    if window.client_inset().unwrap_or_default() != inset {
        window.set_client_inset(inset);
    }
    if inset == px(0.) {
        return root;
    }
    // A capture handler on the padded parent wins over children at corners.
    root.p(inset)
        .capture_any_mouse_down(move |event, window, cx| {
            if event.button == MouseButton::Left
                && let Some(edge) = resize_edge(event.position, window.viewport_size(), inset)
            {
                cx.stop_propagation();
                println!(
                    "{}",
                    json!({"event": "resize_requested", "edge": format!("{edge:?}")})
                );
                window.start_window_resize(edge);
            }
        })
}

fn resize_edge(
    position: Point<Pixels>,
    viewport: Size<Pixels>,
    inset: Pixels,
) -> Option<ResizeEdge> {
    use std::cmp::Ordering::{Equal, Greater, Less};

    let axis = |value: Pixels, limit: Pixels| {
        if value < inset {
            Less
        } else if value >= limit - inset {
            Greater
        } else {
            Equal
        }
    };
    match (
        axis(position.x, viewport.width),
        axis(position.y, viewport.height),
    ) {
        (Less, Less) => Some(ResizeEdge::TopLeft),
        (Equal, Less) => Some(ResizeEdge::Top),
        (Greater, Less) => Some(ResizeEdge::TopRight),
        (Less, Equal) => Some(ResizeEdge::Left),
        (Equal, Equal) => None,
        (Greater, Equal) => Some(ResizeEdge::Right),
        (Less, Greater) => Some(ResizeEdge::BottomLeft),
        (Equal, Greater) => Some(ResizeEdge::Bottom),
        (Greater, Greater) => Some(ResizeEdge::BottomRight),
    }
}
