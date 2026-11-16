//! Opt-in diagnostic events from the pinned GPUI profiler. Never used for gate metrics.
use gpui::{FrameEvent, FrameTimingCollector};
use serde_json::{Value, json};
use std::time::Instant;

pub(crate) struct FrameTrace {
    anchor: Instant,
    collector: FrameTimingCollector,
}

impl FrameTrace {
    pub(crate) fn new() -> Self {
        Self {
            anchor: Instant::now(),
            collector: FrameTimingCollector::new(),
        }
    }

    pub(crate) fn collect(&mut self) -> Value {
        let events: Vec<_> = self
            .collector
            .collect_unseen()
            .into_iter()
            .map(|event| event_json(self.anchor, event))
            .collect();
        json!({
            "units": "nanoseconds since trace creation",
            "scope": "includes warmup; ends before reporting draw; diagnostic overhead enabled",
            "events": events,
        })
    }
}

fn event_json(anchor: Instant, event: FrameEvent) -> Value {
    let ns = |at: Instant| at.saturating_duration_since(anchor).as_nanos() as u64;
    match event {
        FrameEvent::Draw(f) => json!({
            "event": "draw", "window": format!("{:?}", f.window_id),
            "signal_ns": f.signal_at.map(ns), "dirty_ns": f.dirty_at.map(ns),
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
        let value = event_json(anchor, event);
        assert_eq!(value["animation_interval_ns"], 50_000_000);
        assert_eq!(value["start_ns"], 49_000_000);
        assert_eq!(value["end_ns"], 50_000_000);
    }
}
