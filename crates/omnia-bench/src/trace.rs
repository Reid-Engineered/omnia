//! Opt-in frame trace from the pinned GPUI profiler (`OMNIA_BENCH_TRACE=1`). Gate metrics never
//! come from it; the B3/B4 gates use it only to attribute present intervals over their limit
//! (canvas-plan.md 2.1, owner decision 2026-11-17).
//!
//! Two GPUI sources are read once, after the last measured frame, so tracing adds no work of
//! ours to any frame: the frame-event ring (draws and presents, recorded only while tracing)
//! and the foreground journal (always recorded with the `profiler` feature). The journal gives
//! each draw's frame-signal source and the main thread's recorded work.
use gpui::profiler::journal::{
    ForegroundEvent, ForegroundJournalCollector, ForegroundJournalEntry, IntervalBoundary,
    PlatformSignal,
};
use gpui::{App, FrameEvent, FrameRequestSource, FrameTimingCollector};
use serde_json::{Value, json};
use std::time::Instant;

pub(crate) struct FrameTrace {
    anchor: Instant,
    collector: FrameTimingCollector,
    journal: ForegroundJournalCollector,
}

impl FrameTrace {
    pub(crate) fn new(cx: &App) -> Self {
        Self {
            anchor: Instant::now(),
            collector: FrameTimingCollector::new(),
            journal: cx.foreground_journal().collector(),
        }
    }

    pub(crate) fn collect(&mut self) -> Value {
        let journal = self.journal.collect_unseen();
        let signals: Vec<PlatformSignal> = journal
            .entries
            .iter()
            .filter_map(|entry| match entry {
                ForegroundJournalEntry::PlatformSignal(signal) => Some(*signal),
                _ => None,
            })
            .collect();
        let anchor = self.anchor;
        let foreground: Vec<_> = journal
            .entries
            .iter()
            .filter_map(|entry| work_json(anchor, entry))
            .collect();
        let events: Vec<_> = self
            .collector
            .collect_unseen()
            .into_iter()
            .map(|event| event_json(anchor, event, &signals))
            .collect();
        json!({
            "units": "nanoseconds since trace creation",
            "scope": "includes warmup; ends before reporting draw; diagnostic overhead enabled",
            "events": events,
            "foreground": foreground,
            "journal_lost": journal.lost,
        })
    }
}

fn ns(anchor: Instant, at: Instant) -> u64 {
    at.saturating_duration_since(anchor).as_nanos() as u64
}

/// Main-thread work the GPUI journal recorded: task polls of at least 100 µs, spans of
/// folded shorter polls, actions, inputs, draws, presents and callbacks that drew nothing.
/// View phases lie inside their draw and are left out.
fn work_json(anchor: Instant, entry: &ForegroundJournalEntry) -> Option<Value> {
    let span = |kind: &str, start: Instant, end: Instant| {
        Some(json!({ "kind": kind, "start_ns": ns(anchor, start), "end_ns": ns(anchor, end) }))
    };
    match entry {
        ForegroundJournalEntry::Event(event) => {
            let kind = match event {
                ForegroundEvent::TaskPoll(_) => "task_poll",
                ForegroundEvent::Action(_) => "action",
                ForegroundEvent::Input(_) => "input",
                ForegroundEvent::Draw(_) => "draw",
                ForegroundEvent::View(_) => return None,
                ForegroundEvent::Present(_) => "present",
                ForegroundEvent::SmallPolls(_) => "small_polls",
            };
            span(kind, event.start_time(), event.end_time())
        }
        ForegroundJournalEntry::Boundary(IntervalBoundary::Presented(presented)) => span(
            "present",
            presented.presentation.present_start,
            presented.presentation.present_end,
        ),
        ForegroundJournalEntry::Boundary(IntervalBoundary::FrameSkipped(skipped)) => {
            span("frame_skipped", skipped.at, skipped.at)
        }
        ForegroundJournalEntry::ReentrantFrameSkipped { at, .. } => span("frame_skipped", *at, *at),
        _ => None,
    }
}

/// The source of a draw's frame signal: the last platform signal its window handled before the
/// draw began, if that signal carries the draw's signal time.
fn signal_source(signals: &[PlatformSignal], draw: &gpui::FrameTiming) -> Value {
    let Some(signal_at) = draw.signal_at else {
        return Value::Null;
    };
    let matched = signals
        .iter()
        .rev()
        .find(|s| s.window_id == draw.window_id && s.handled_at <= draw.draw_start)
        .filter(|s| s.signal_at == Some(signal_at));
    match matched.map(|s| s.source) {
        Some(FrameRequestSource::NativeCallback) => json!("native"),
        Some(FrameRequestSource::LocalSchedule) => json!("local"),
        None => json!("unmatched"),
    }
}

fn event_json(anchor: Instant, event: FrameEvent, signals: &[PlatformSignal]) -> Value {
    let ns = |at: Instant| ns(anchor, at);
    match event {
        FrameEvent::Draw(f) => json!({
            "event": "draw", "window": format!("{:?}", f.window_id),
            "signal_ns": f.signal_at.map(ns), "dirty_ns": f.dirty_at.map(ns),
            "signal_source": signal_source(signals, &f),
            "start_ns": ns(f.draw_start), "end_ns": ns(f.draw_end),
        }),
        FrameEvent::Present(p) => json!({
            "event": "present", "window": format!("{:?}", p.window_id),
            "start_ns": ns(p.present_start), "end_ns": ns(p.present_end),
            "animation_interval_ns": p.animation_interval.map(|d| d.as_nanos() as u64),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn preserves_long_intervals_and_present_call_separately() {
        let anchor = Instant::now();
        let event = FrameEvent::Present(gpui::PresentTiming {
            window_id: gpui::WindowId::from(0),
            present_start: anchor + Duration::from_millis(49),
            present_end: anchor + Duration::from_millis(50),
            animation_interval: Some(Duration::from_millis(50)),
        });
        let value = event_json(anchor, event, &[]);
        assert_eq!(value["animation_interval_ns"], 50_000_000);
        assert_eq!(value["start_ns"], 49_000_000);
        assert_eq!(value["end_ns"], 50_000_000);
    }

    fn draw(window: u64, signal_at: Option<Instant>, draw_start: Instant) -> gpui::FrameTiming {
        gpui::FrameTiming {
            window_id: gpui::WindowId::from(window),
            dirty_at: None,
            invalidations: 0,
            draw_start,
            draw_end: draw_start,
            refresh_interval: None,
            signal_at,
            phases: Default::default(),
        }
    }

    fn signal(
        window: u64,
        signal_at: Instant,
        handled_at: Instant,
        source: FrameRequestSource,
    ) -> PlatformSignal {
        PlatformSignal {
            window_id: gpui::WindowId::from(window),
            signal_at: Some(signal_at),
            handled_at,
            source,
        }
    }

    #[test]
    fn signal_source_comes_from_the_matching_platform_signal() {
        let t = Instant::now();
        let ms = Duration::from_millis;
        let signals = [
            signal(0, t, t + ms(1), FrameRequestSource::NativeCallback),
            signal(0, t + ms(16), t + ms(17), FrameRequestSource::LocalSchedule),
            signal(
                1,
                t + ms(30),
                t + ms(30),
                FrameRequestSource::NativeCallback,
            ),
        ];
        let source = |d| signal_source(&signals, &d);
        assert_eq!(source(draw(0, Some(t), t + ms(2))), "native");
        assert_eq!(source(draw(0, Some(t + ms(16)), t + ms(18))), "local");
        // The last signal handled before the draw must carry the draw's signal time.
        assert_eq!(source(draw(0, Some(t), t + ms(18))), "unmatched");
        assert_eq!(source(draw(0, None, t + ms(2))), Value::Null);
        assert_eq!(source(draw(2, Some(t), t + ms(40))), "unmatched");
    }
}
