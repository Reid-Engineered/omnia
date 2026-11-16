//! T1.5 "Done when": simulated input through a GPUI test window.
//!
//! - The world point under the cursor stays fixed across wheel and pinch zooms.
//! - A status update (cursor movement) notifies neither the sheet canvas (which renders
//!   `SheetView`) nor the window root; only the status bar view renders again.
//! - Middle-drag and space-drag pan, and a drag keeps panning outside the sheet.
//!
//! GPUI's test platform (feature `test-support`) has no GPU and opens no OS window, and it
//! draws a dirty window while flushing effects, so render counts below are real frames.

use gpui::{
    Entity, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, PinchEvent, Pixels, Point,
    ScrollDelta, ScrollEvent, TestAppContext, TouchPhase, VisualTestContext, point, px,
};
use omnia_app::Workspace;
use omnia_app::status_bar::StatusBar;
use omnia_canvas::SheetCanvas;
use omnia_doc::Pt;
use std::{cell::Cell, rc::Rc};

struct Fixture {
    root: Entity<Workspace>,
    canvas: Entity<SheetCanvas>,
    bar: Entity<StatusBar>,
    root_notifies: Rc<Cell<u32>>,
    canvas_notifies: Rc<Cell<u32>>,
    bar_notifies: Rc<Cell<u32>>,
}

fn counter<T: 'static>(cx: &mut VisualTestContext, entity: &Entity<T>) -> Rc<Cell<u32>> {
    let count = Rc::new(Cell::new(0));
    let c = count.clone();
    cx.update(|_, cx| cx.observe(entity, move |_, _| c.set(c.get() + 1)).detach());
    count
}

fn open(cx: &mut TestAppContext) -> (Fixture, &mut VisualTestContext) {
    let (root, cx) = cx.add_window_view(|_, cx| Workspace::new(None).with_sheet(cx));
    let (canvas, bar) = cx.update(|_, cx| {
        let workspace = root.read(cx);
        (
            workspace.canvas().unwrap().clone(),
            workspace.status_bar().unwrap().clone(),
        )
    });
    let fixture = Fixture {
        root_notifies: counter(cx, &root),
        canvas_notifies: counter(cx, &canvas),
        bar_notifies: counter(cx, &bar),
        root,
        canvas,
        bar,
    };
    (fixture, cx)
}

impl Fixture {
    fn renders(&self, cx: &mut VisualTestContext) -> (u64, u64) {
        cx.update(|_, cx| {
            (
                self.canvas.read(cx).render_count(),
                self.bar.read(cx).render_count(),
            )
        })
    }

    fn world_under(&self, cx: &mut VisualTestContext, p: Point<Pixels>) -> Pt {
        cx.update(|_, cx| {
            self.canvas
                .read(cx)
                .camera()
                .screen_to_world(point(f64::from(p.x), f64::from(p.y)))
        })
    }

    fn zoom(&self, cx: &mut VisualTestContext) -> f64 {
        cx.update(|_, cx| self.canvas.read(cx).camera().zoom())
    }
}

fn assert_fixed(before: Pt, after: Pt) {
    assert!(
        (i64::from(before.x) - i64::from(after.x)).abs() <= 1
            && (i64::from(before.y) - i64::from(after.y)).abs() <= 1,
        "world point under the cursor moved from {before:?} to {after:?}"
    );
}

/// A point inside the sheet: below the two header lines, above the status bar.
fn on_sheet() -> Point<Pixels> {
    point(px(431.25), px(300.5))
}

#[gpui::test]
fn status_update_notifies_only_the_status_bar(cx: &mut TestAppContext) {
    let (f, cx) = open(cx);
    let renders = f.renders(cx);
    assert_eq!(cx.update(|_, cx| f.bar.read(cx).status().cursor), None);

    for (i, x) in [431.25f32, 432.0, 500.75].into_iter().enumerate() {
        cx.simulate_mouse_move(point(px(x), px(300.5)), None, Modifiers::none());
        let status = cx.update(|_, cx| f.bar.read(cx).status());
        assert_eq!(
            status.cursor,
            Some(f.world_under(cx, point(px(x), px(300.5))))
        );
        assert_eq!(f.bar_notifies.get(), i as u32 + 1);
        assert_eq!(f.renders(cx), (renders.0, renders.1 + i as u64 + 1));
    }
    assert_eq!(
        f.canvas_notifies.get(),
        0,
        "a status update notified SheetCanvas"
    );
    assert_eq!(
        f.root_notifies.get(),
        0,
        "a status update notified the window root"
    );
}

#[gpui::test]
fn wheel_zoom_keeps_the_world_point_under_the_cursor(cx: &mut TestAppContext) {
    let (f, cx) = open(cx);
    let at = on_sheet();
    cx.simulate_mouse_move(at, None, Modifiers::none());
    let before = f.world_under(cx, at);
    let canvas_renders = f.renders(cx).0;
    let mut expected = 1.0f64;
    // T0.5: ±3 lines per notch, -6 when two notches merge.
    for lines in [3.0f32, 3.0, -6.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0, 3.0] {
        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Lines(point(0.0, lines)),
            ..Default::default()
        });
        expected *= (f64::from(lines) / 12.0).exp2();
        assert!((f.zoom(cx) / expected - 1.0).abs() < 1e-12);
        assert_fixed(before, f.world_under(cx, at));
        let status = cx.update(|_, cx| f.bar.read(cx).status());
        assert_eq!(status.zoom, expected as f32);
        assert_fixed(before, status.cursor.unwrap());
    }
    // Each zoom notified the canvas once and repainted it once; never the root.
    assert_eq!(f.canvas_notifies.get(), 10);
    assert_eq!(f.renders(cx).0, canvas_renders + 10);
    assert_eq!(f.root_notifies.get(), 0);
}

#[gpui::test]
fn pinch_zoom_keeps_the_world_point_under_the_pinch(cx: &mut TestAppContext) {
    let (f, cx) = open(cx);
    let at = point(px(612.5), px(250.25));
    let before = f.world_under(cx, at);
    let pinch = |delta, phase| PinchEvent {
        position: at,
        delta,
        modifiers: Modifiers::none(),
        phase,
    };
    cx.simulate_event(pinch(0.0, TouchPhase::Started));
    let mut expected = 1.0f64;
    // T0.5: Moved deltas are the scale ratio - 1, per event |delta| p50 0.014, p99 0.067.
    for delta in [0.014f32, 0.067, 0.2, -0.05, 0.5] {
        cx.simulate_event(pinch(delta, TouchPhase::Moved));
        expected *= 1.0 + f64::from(delta);
        assert_fixed(before, f.world_under(cx, at));
    }
    cx.simulate_event(pinch(0.0, TouchPhase::Ended));
    assert!((f.zoom(cx) / expected - 1.0).abs() < 1e-12);
    assert_eq!(f.root_notifies.get(), 0);
}

#[gpui::test]
fn touchpad_scroll_pans_by_the_delta(cx: &mut TestAppContext) {
    let (f, cx) = open(cx);
    let at = on_sheet();
    let p = Pt::ZERO;
    let screen = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| f.canvas.read(cx).camera().world_to_screen(p))
    };
    let before = screen(cx);
    for (dx, dy, phase) in [
        (0.0, 0.0, TouchPhase::Started),
        (1.5, -64.25, TouchPhase::Moved),
        (-0.5, 0.25, TouchPhase::Moved),
        (0.0, 0.0, TouchPhase::Ended),
    ] {
        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            touch_phase: phase,
            ..Default::default()
        });
    }
    assert_eq!(screen(cx), point(before.x + 1.0, before.y - 64.0));
    assert_eq!(f.zoom(cx), 1.0);
}

#[gpui::test]
fn middle_drag_pans_and_follows_the_pointer_off_the_sheet(cx: &mut TestAppContext) {
    let (f, cx) = open(cx);
    let p = Pt::ZERO;
    let screen = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| f.canvas.read(cx).camera().world_to_screen(p))
    };
    let before = screen(cx);
    let start = on_sheet();
    cx.simulate_mouse_down(start, MouseButton::Middle, Modifiers::none());
    cx.simulate_mouse_move(
        point(px(450.0), px(320.0)),
        MouseButton::Middle,
        Modifiers::none(),
    );
    // Off the top of the window, over the header: the pan holds the pointer capture.
    cx.simulate_mouse_move(
        point(px(460.0), px(-40.0)),
        MouseButton::Middle,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        point(px(460.0), px(-40.0)),
        MouseButton::Middle,
        Modifiers::none(),
    );
    let after = screen(cx);
    assert_eq!(
        (after.x - before.x, after.y - before.y),
        (460.0 - 431.25, -40.0 - 300.5)
    );
    // Released: further moves do not pan.
    cx.simulate_mouse_move(on_sheet(), None, Modifiers::none());
    assert_eq!(screen(cx), after);
    assert_eq!(f.root_notifies.get(), 0);
}

#[gpui::test]
fn space_drag_pans(cx: &mut TestAppContext) {
    let (f, cx) = open(cx);
    let p = Pt::ZERO;
    let screen = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| f.canvas.read(cx).camera().world_to_screen(p))
    };
    let before = screen(cx);
    let at = on_sheet();
    // A plain left drag does not pan, but it focuses the sheet for the Space key.
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(px(480.0), px(300.5)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        point(px(480.0), px(300.5)),
        MouseButton::Left,
        Modifiers::none(),
    );
    assert_eq!(screen(cx), before);

    let space = Keystroke::parse("space").unwrap();
    cx.simulate_event(KeyDownEvent {
        keystroke: space.clone(),
        is_held: false,
        prefer_character_input: false,
    });
    assert!(cx.update(|_, cx| f.canvas.read(cx).navigator().space_held()));
    cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        point(px(400.0), px(350.5)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_up(
        point(px(400.0), px(350.5)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_event(KeyUpEvent { keystroke: space });
    assert!(!cx.update(|_, cx| f.canvas.read(cx).navigator().space_held()));
    let after = screen(cx);
    assert_eq!(
        (after.x - before.x, after.y - before.y),
        (400.0 - 431.25, 50.0)
    );
    let _ = &f.root;
}
