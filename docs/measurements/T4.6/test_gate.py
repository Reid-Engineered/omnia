#!/usr/bin/env python3
"""Unit tests for the T4.6 Milestone 4 Orion gate script."""

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gate


def make_conditions(scale):
    return {
        "mains_online": {"AC": "1"},
        "panel": {
            "name": "eDP-1",
            "enabled": True,
            "scale": scale,
            "size": {"width": 1920, "height": 1080},
            "refreshRate": 60.008,
        },
        "governor": {f"cpu{i}": "performance" for i in range(4)},
        "power_profile": None,
        "platform_profile": {},
        "processes": ["omnia-bench"],
        "time_utc": "2026-11-23T00:00:00+00:00",
        "dpms": "DPMS mode for screen eDP-1: On",
    }


def make_start(sheet, viewport, ms):
    return {
        "schema": 1,
        "kind": "start",
        "config": {"sheet": sheet, "runs": gate.START_SAMPLES, "window_logical": viewport},
        "environment": {"os": "linux"},
        "samples_ms": [ms] * gate.START_SAMPLES,
    }


def make_hit(p99=0.05):
    return {
        "schema": 1,
        "kind": "hit",
        "config": {
            "sheet": "10k",
            "seed": gate.HIT_SEED,
            "samples": gate.HIT_SAMPLES,
            "tolerance_nm": gate.HIT_TOLERANCE_NM,
            "grid_nm": gate.GRID_NM,
        },
        "environment": {"os": "linux", "arch": "x86_64", "window": None},
        "hits": 18500,
        "parts": 10000,
        "wires": 20000,
        "metrics": {"hit_ms": {"p50": p99 * 0.5, "p95": p99 * 0.8, "p99": p99, "max": p99 * 1.5}},
    }


def make_trace():
    refresh = 16_664_444
    events = []
    for i in range(gate.FRAMES * 2):
        base = i * refresh
        events.append(
            {
                "event": "draw",
                "window": 1,
                "signal_ns": base,
                "signal_source": "native",
                "start_ns": base + 100_000,
                "end_ns": base + 1_100_000,
            }
        )
        events.append(
            {
                "event": "present",
                "window": 1,
                "start_ns": base + 1_200_000,
                "end_ns": base + 2_200_000,
                "animation_interval_ns": refresh if i > 0 else None,
            }
        )
    return {
        "units": "nanoseconds since trace creation",
        "events": events,
        "foreground": [],
        "journal_lost": 0,
    }


TRACE_TEMPLATE = make_trace()


def make_motion(
    sheet,
    script,
    scale,
    viewport,
    *,
    p99=2.0,
    maximum=4.0,
    ours=0,
    memory=90.0,
    sheet_paints=0,
    transitions=None,
    mismatches=0,
):
    parts = 1000 if sheet == "1k" else 10000
    metrics = {
        "paint_sampling": "overlay-every-frame" if script == "hover" else "every-frame",
        "paint_samples": gate.FRAMES,
        "frame_ms": {"p99": p99, "max": maximum},
        "allocs_per_frame": {"ours": {"max": ours}},
        "missed_refreshes": 0,
        "present_intervals_over_2_5_refreshes": 0,
        "memory_mb": memory,
    }
    report = {
        "schema": 1,
        "kind": "run",
        "config": {
            "sheet": sheet,
            "script": script,
            "frames": gate.FRAMES,
            "warmup": gate.FRAMES,
            "window_logical": viewport,
            "scene_counts": False,
            "diagnostic_trace": True,
        },
        "scene_details": {"seed": 42, "parts": parts, "wires": 2 * parts},
        "environment": {
            "os": "linux",
            "fullscreen": True,
            "scale_factor": scale,
            "viewport_logical": viewport,
            "viewport_device": [1920, 1080],
            "refresh_hz": 60.008,
            "gpu": {
                "is_software_emulated": False,
                "device_name": "Intel(R) HD Graphics 620 (KBL GT2)",
                "driver_name": "Intel open-source Mesa driver",
            },
        },
        "frames": {"drawn": gate.FRAMES, "on_time": gate.FRAMES, "late": 0, "unmeasured": 0},
        "warnings": [],
        "metrics": metrics,
        "diagnostic_trace": TRACE_TEMPLATE,
    }
    if script == "hover":
        metrics["sheet_paints"] = sheet_paints
        report["hover"] = {
            "overlay_paints": gate.FRAMES,
            "sheet_paints": sheet_paints,
            "transitions": gate.FRAMES if transitions is None else transitions,
            "mismatches": mismatches,
            "net_highlight_frames": gate.FRAMES // 3,
            "sweep_targets": 72,
        }
    return report


def write_campaign(root: Path, overrides=None):
    overrides = overrides or {}
    order = gate.schedule()
    for tag in gate.SCALES:
        (root / tag).mkdir(parents=True)
    (root / "order.json").write_text(json.dumps(order), encoding="utf-8")
    (root / "hardware.json").write_text(
        json.dumps(
            {
                "cpu": "Intel(R) Core(TM) i5-7300U CPU @ 2.60GHz",
                "os": "Linux orion 7.2.7-arch1-1",
                "session": "wayland",
                "panel": "eDP-1",
            }
        ),
        encoding="utf-8",
    )
    fp = {"sha256": "deadbeef", "commit": "370170b", "tracked_status": ""}
    (root / "binary-before.json").write_text(json.dumps(fp), encoding="utf-8")
    (root / "binary-after.json").write_text(json.dumps(fp), encoding="utf-8")
    with (root / "runs.jsonl").open("w", encoding="utf-8") as journal:
        for tag, rep, case in order:
            scale, viewport = gate.SCALES[tag]
            cond = make_conditions(scale)
            journal.write(
                json.dumps(
                    {"case": [tag, rep, case], "phase": "before", "conditions": cond, "exit": None}
                )
                + "\n"
            )
            journal.write(
                json.dumps(
                    {"case": [tag, rep, case], "phase": "after", "conditions": cond, "exit": 0}
                )
                + "\n"
            )
            stem = root / tag / f"{case}-{rep}"
            if case in gate.STARTS:
                sheet = case.split("-")[1]
                ms = overrides.get((tag, case), 200.0 if sheet == "empty" else 210.0)
                report = make_start(sheet, viewport, ms)
            elif case in gate.CPU_CASES:
                kw = overrides.get((tag, case), {})
                report = make_hit(**kw)
            else:
                sheet, script = case.split("-")
                kw = overrides.get((tag, case), {})
                report = make_motion(sheet, script, scale, viewport, **kw)
            stem.with_suffix(".json").write_text(json.dumps(report), encoding="utf-8")


class GateTest(unittest.TestCase):
    def test_schedule_covers_all_cases_and_rotates(self):
        order = gate.schedule()
        # s100: 10 * (2 starts + 5 motion + 1 cpu hit) = 80
        # s125: 10 * (2 starts + 5 motion) = 70
        self.assertEqual(len(order), 150)
        for tag in gate.SCALES:
            expected_cases = [*gate.STARTS, *gate.MOTION]
            if tag == "s100":
                expected_cases.extend(gate.CPU_CASES)
            first_cases = []
            for rep in range(1, gate.REPETITIONS + 1):
                cases = [c for t, r, c in order if t == tag and r == rep]
                self.assertEqual(sorted(cases), sorted(expected_cases))
                first_cases.append(cases[0])
                start_order = [c for c in cases if c in gate.STARTS]
                expected_starts = list(gate.STARTS[::-1] if (rep - 1) % 2 else gate.STARTS)
                self.assertEqual(start_order, expected_starts)
            distinct_first = 7 if tag == "s100" else 6
            self.assertEqual(len(set(first_cases[:distinct_first])), distinct_first)

    def test_passing_campaign(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp))
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertTrue(ok, buf.getvalue())
            self.assertIn("PASS B7 hit-10k p99", buf.getvalue())
            self.assertIn("PASS T4.6 10k-hover frame p99", buf.getvalue())
            self.assertIn("PASS INV11 10k-hover max SheetView paints", buf.getvalue())

    def test_hit_over_b7_fails_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "hit-10k"): {"p99": 0.25}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL B7 hit-10k p99", buf.getvalue())

    def test_hover_p99_over_16ms_fails_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "10k-hover"): {"p99": 16.1, "maximum": 20.0}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL T4.6 10k-hover frame p99", buf.getvalue())

    def test_hover_sheet_paint_over_zero_fails_inv11(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s125", "10k-hover"): {"sheet_paints": 1}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL INV11 10k-hover max SheetView paints", buf.getvalue())

    def test_hover_mismatch_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "10k-hover"): {"mismatches": 1}})
            with self.assertRaisesRegex(ValueError, "incomplete hover exercise"):
                gate.summarize(Path(tmp))

    def test_hover_short_transitions_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s125", "10k-hover"): {"transitions": gate.FRAMES - 1}})
            with self.assertRaisesRegex(ValueError, "incomplete hover exercise"):
                gate.summarize(Path(tmp))


if __name__ == "__main__":
    unittest.main()
