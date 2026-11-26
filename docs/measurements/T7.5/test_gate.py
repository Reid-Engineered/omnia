#!/usr/bin/env python3
"""Unit tests for the T7.5 final canvas budget gate (B1 to B10)."""

from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gate


def clean_conditions(scale):
    return {
        "mains_online": {"AC": "1"},
        "governor": {f"cpu{i}": "performance" for i in range(4)},
        "power_profile": None,
        "platform_profile": {},
        "panel": {"scale": scale, "size": {"width": 1920, "height": 1080}, "refreshRate": 60.008},
        "processes": ["1 systemd", "802 kwin_wayland"],
        "time_utc": "2026-11-26T20:00:00+00:00",
        "dpms": "dpms mode for screen eDP-1: on",
    }


def gui_env(scale, viewport):
    return {
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
    }


def make_start(sheet, viewport, sample_ms):
    return {
        "schema": 1,
        "kind": "start",
        "config": {"sheet": sheet, "runs": gate.START_SAMPLES, "window_logical": viewport},
        "environment": {"os": "linux"},
        "samples_ms": [sample_ms] * gate.START_SAMPLES,
    }


def make_idle(scale, viewport, frames=0, renders=0, cpu_percent=0.0):
    return {
        "schema": 1,
        "kind": "idle",
        "config": {
            "sheet": "empty",
            "seconds": float(gate.IDLE_SECONDS),
            "settle_ms": gate.IDLE_SETTLE_MS,
            "window_logical": viewport,
        },
        "environment": gui_env(scale, viewport),
        "startup": {"frames": 2, "renders": 2},
        "measured_s": 10.001,
        "measured_renders": renders,
        "metrics": {"frames": frames, "renders": renders, "cpu_percent": cpu_percent},
    }


def make_hit(p99_ms=0.05):
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
        "parts": 10000,
        "wires": 20000,
        "hits": 12000,
        "metrics": {"hit_ms": {"p50": p99_ms / 2, "p99": p99_ms, "max": p99_ms * 2}},
    }


def make_trace():
    refresh = 16_664_444
    events = []
    t = 0
    prev_present_end = None
    for _ in range(gate.FRAMES * 2):
        signal_ns = t if prev_present_end is None else prev_present_end + (refresh - 2_200_000)
        draw_start = signal_ns + 100_000
        draw_end = signal_ns + 1_100_000
        present_start = signal_ns + 1_200_000
        present_end = signal_ns + 2_200_000
        interval = None if prev_present_end is None else present_end - prev_present_end
        events.append(
            {
                "event": "draw",
                "window": 1,
                "signal_ns": signal_ns,
                "signal_source": "native",
                "start_ns": draw_start,
                "end_ns": draw_end,
            }
        )
        events.append(
            {
                "event": "present",
                "window": 1,
                "start_ns": present_start,
                "end_ns": present_end,
                "animation_interval_ns": interval,
            }
        )
        prev_present_end = present_end
        t = present_end
    return {
        "units": "nanoseconds since trace creation",
        "events": events,
        "foreground": [],
        "journal_lost": 0,
    }


TRACE_TEMPLATE = make_trace()


def make_motion(sheet, script, scale, viewport, p99=2.0, maximum=4.0, d2p_p99=3.0, ours=0.0, mem=160.0):
    parts, wires = gate.SCENE_COUNTS[sheet]
    metrics = {
        "paint_sampling": "overlay-every-frame" if script in ("drag-part", "drag-wire") else "every-frame",
        "paint_samples": gate.FRAMES,
        "frame_ms": {"p50": p99 / 2, "p95": p99 * 0.9, "p99": p99, "max": maximum},
        "allocs_per_frame": {"ours": {"max": ours}},
        "memory_mb": mem,
        "missed_refreshes": 0,
        "present_intervals_over_2_5_refreshes": 0,
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
        "scene_details": {"seed": 42, "parts": parts, "wires": wires},
        "environment": gui_env(scale, viewport),
        "warnings": [],
        "frames": {
            "startup_draws": 0,
            "drawn": gate.FRAMES,
            "unmeasured": 0,
            "on_time": gate.FRAMES,
            "late": 0,
        },
        "metrics": metrics,
        "diagnostic_trace": TRACE_TEMPLATE,
    }
    if script == "drag-part":
        metrics["sheet_paints"] = 0
        metrics["dirty_to_present_ms"] = {"p99": d2p_p99, "max": d2p_p99 + 1.0}
        report["drag"] = {
            "overlay_paints": gate.FRAMES,
            "sheet_paints": 0,
            "press_sheet_paints": 0,
            "lift_sheet_paints": 1,
            "revision_changes": 0,
            "transitions": gate.FRAMES,
            "mismatches": 0,
            "waypoints": 8,
        }
    elif script == "drag-wire":
        metrics["sheet_paints"] = 0
        metrics["dirty_to_present_ms"] = {"p99": d2p_p99, "max": d2p_p99 + 1.0}
        report["wire"] = {
            "overlay_paints": gate.FRAMES,
            "sheet_paints": 0,
            "press_sheet_paints": 0,
            "warmup_sheet_paints": 0,
            "revision_changes": 0,
            "transitions": gate.FRAMES,
            "mismatches": 0,
            "waypoints": 14,
            "two_leg_frames": gate.FRAMES,
        }
    return report


def write_campaign(root, mutate=None):
    order = gate.schedule()
    (root / "order.json").write_text(json.dumps(order), encoding="utf-8")
    fp = {"sha256": "deadbeef", "commit": "3f8d257", "tracked_status": ""}
    (root / "binary-before.json").write_text(json.dumps(fp), encoding="utf-8")
    (root / "binary-after.json").write_text(json.dumps(fp), encoding="utf-8")
    (root / "hardware.json").write_text(
        json.dumps(
            {
                "cpu": "Intel(R) Core(TM) i5-7300U CPU @ 2.60GHz",
                "os": "Linux orion-laptop 7.2.7-arch1-1",
                "session": "wayland",
                "panel": "eDP-1",
            }
        ),
        encoding="utf-8",
    )
    for tag in gate.SCALES:
        (root / tag).mkdir()
    journal_lines = []
    for tag, rep, case in order:
        scale, viewport = gate.SCALES[tag]
        cond = clean_conditions(scale)
        journal_lines.append(
            json.dumps({"case": [tag, rep, case], "phase": "before", "conditions": cond, "exit": None})
        )
        journal_lines.append(
            json.dumps({"case": [tag, rep, case], "phase": "after", "conditions": cond, "exit": 0})
        )
        if case == "start-empty":
            rep_data = make_start("empty", viewport, 120.0)
        elif case == "start-1k":
            rep_data = make_start("1k", viewport, 165.0)
        elif case == "idle-empty":
            rep_data = make_idle(scale, viewport)
        elif case == "hit-10k":
            rep_data = make_hit(0.05)
        else:
            sheet, script = case.split("-", 1)
            rep_data = make_motion(sheet, script, scale, viewport)
        if mutate:
            mutate(tag, rep, case, rep_data)
        (root / tag / f"{case}-{rep}.json").write_text(json.dumps(rep_data), encoding="utf-8")
    (root / "runs.jsonl").write_text("\n".join(journal_lines) + "\n", encoding="utf-8")


class GateTest(unittest.TestCase):
    def test_schedule_counts_and_rotation(self):
        order = gate.schedule()
        self.assertEqual(len(order), 230)
        s100 = [entry for entry in order if entry[0] == "s100"]
        s125 = [entry for entry in order if entry[0] == "s125"]
        self.assertEqual(len(s100), 120)
        self.assertEqual(len(s125), 110)
        self.assertEqual(sum(1 for _, _, c in order if c == "hit-10k"), 10)
        self.assertEqual(sum(1 for _, _, c in order if c == "idle-empty"), 20)

    def test_args_for_all_case_kinds(self):
        args, traced = gate.args_for("start-empty", "1920x1080")
        self.assertEqual(args[:3], ["start", "--sheet", "empty"])
        self.assertFalse(traced)
        args, traced = gate.args_for("idle-empty", "1536x864")
        self.assertEqual(args, ["idle", "--sheet", "empty", "--seconds", "10", "--settle-ms", "1000", "--size", "1536x864"])
        self.assertFalse(traced)
        args, traced = gate.args_for("hit-10k", "1920x1080")
        self.assertEqual(args, ["hit", "--sheet", "10k", "--samples", "20000", "--seed", "42"])
        self.assertFalse(traced)
        args, traced = gate.args_for("10k-drag-part", "1920x1080")
        self.assertEqual(args[:5], ["run", "--sheet", "10k", "--script", "drag-part"])
        self.assertTrue(traced)

    def test_passing_campaign_covers_b1_through_b10(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            write_campaign(root)
            buf = io.StringIO()
            with redirect_stdout(buf):
                passed = gate.summarize(root)
            out = buf.getvalue()
            self.assertTrue(passed, out)
            for budget in ("B1 ", "B2 ", "B3 ", "B4 ", "B5 ", "B6 ", "B7 ", "B8 ", "B9 ", "B10 "):
                self.assertIn(f"PASS {budget}", out)

    def test_b1_b7_b8_failures_report_fail(self):
        def mutate(tag, _rep, case, data):
            if tag == "s100" and case == "start-empty":
                data["samples_ms"] = [450.0] * gate.START_SAMPLES
            if case == "hit-10k":
                data["metrics"]["hit_ms"]["p99"] = 0.25
            if tag == "s125" and case == "idle-empty":
                data["metrics"]["frames"] = 1

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            write_campaign(root, mutate=mutate)
            buf = io.StringIO()
            with redirect_stdout(buf):
                passed = gate.summarize(root)
            out = buf.getvalue()
            self.assertFalse(passed)
            self.assertIn("FAIL B1 start-empty", out)
            self.assertIn("FAIL B7 hit-10k", out)
            self.assertIn("FAIL B8 idle-empty", out)

    def test_drag_part_and_drag_wire_invariants_fail_explicitly(self):
        def mutate(tag, rep, case, data):
            if tag == "s100" and rep == 1 and case == "1k-drag-part":
                data["drag"]["lift_sheet_paints"] = 2
                data["warnings"].append(
                    "D7: SheetView painted 2 times during warm-up lift-off, expected 1"
                )
            if tag == "s125" and rep == 1 and case == "dense-drag-wire":
                data["wire"]["revision_changes"] = 1
                data["warnings"].append(
                    "INV5: document revision changed on 1 measured frames during drag-wire"
                )

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            write_campaign(root, mutate=mutate)
            buf = io.StringIO()
            with redirect_stdout(buf):
                passed = gate.summarize(root)
            out = buf.getvalue()
            self.assertFalse(passed)
            self.assertIn("FAIL D7 1k-drag-part", out)
            self.assertIn("FAIL INV5 dense-drag-wire", out)

    def test_unexpected_warning_fails_closed(self):
        def mutate(tag, rep, case, data):
            if tag == "s100" and rep == 1 and case == "10k-pan":
                data["warnings"].append("unexpected stall")

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            write_campaign(root, mutate=mutate)
            with self.assertRaisesRegex(ValueError, "incomplete frame sampling"):
                gate.summarize(root)


if __name__ == "__main__":
    unittest.main()
