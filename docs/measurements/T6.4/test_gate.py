#!/usr/bin/env python3
"""Unit tests for the T6.4 Milestone 6 Orion gate script."""

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
        "time_utc": "2026-11-26T00:00:00+00:00",
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


def make_trace(startup_draws=0, late_measured_index=None, late_interval_refreshes=None):
    refresh = 16_664_444
    events = []
    t = 0
    prev_present_end = None
    total = startup_draws + gate.FRAMES * 2
    for i in range(total):
        measured_idx = i - startup_draws - gate.FRAMES
        if late_measured_index is not None and measured_idx == late_measured_index:
            interval = int(round(late_interval_refreshes * refresh))
            signal_ns = prev_present_end + 100_000
            present_end = prev_present_end + interval
            draw_start = signal_ns + 100_000
            draw_end = present_end - 200_000
            present_start = present_end - 100_000
        else:
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


TRACE_TEMPLATE = make_trace(0)
WIRE_TRACE_TEMPLATE = make_trace(11)


def make_motion(
    sheet,
    script,
    scale,
    viewport,
    *,
    p99=2.0,
    maximum=4.0,
    d2p_p99=3.0,
    d2p_max=5.0,
    ours=0,
    memory=155.0,
    sheet_paints=0,
    press_sheet_paints=0,
    warmup_sheet_paints=0,
    transitions=None,
    mismatches=0,
    revision_changes=0,
    waypoints=20,
    two_leg_frames=500,
    omit_d2p=False,
    startup_draws=None,
    trace=None,
    missed_refreshes=0,
    over_2_5_refreshes=0,
    warnings=None,
):
    parts, wires = gate.SCENE_COUNTS[sheet]
    is_wire = script == "drag-wire"
    metrics = {
        "paint_sampling": "overlay-every-frame" if is_wire else "every-frame",
        "paint_samples": gate.FRAMES,
        "frame_ms": {"p99": p99, "max": maximum},
        "allocs_per_frame": {"ours": {"max": ours}},
        "missed_refreshes": missed_refreshes,
        "present_intervals_over_2_5_refreshes": over_2_5_refreshes,
        "memory_mb": memory,
    }
    if not omit_d2p:
        metrics["dirty_to_present_ms"] = {"p99": d2p_p99, "max": d2p_max}
    resolved_startup = (11 if is_wire else 0) if startup_draws is None else startup_draws
    resolved_trace = (WIRE_TRACE_TEMPLATE if is_wire else TRACE_TEMPLATE) if trace is None else trace
    resolved_warnings = list(warnings) if warnings is not None else []
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
        "frames": {
            "drawn": gate.FRAMES,
            "on_time": gate.FRAMES,
            "late": 0,
            "unmeasured": 0,
            "startup_draws": resolved_startup,
        },
        "warnings": resolved_warnings,
        "metrics": metrics,
        "diagnostic_trace": resolved_trace,
    }
    if is_wire:
        metrics["sheet_paints"] = sheet_paints
        report["wire"] = {
            "start": [0, 0],
            "overlay_paints": gate.FRAMES,
            "sheet_paints": sheet_paints,
            "press_sheet_paints": press_sheet_paints,
            "warmup_sheet_paints": warmup_sheet_paints,
            "transitions": gate.FRAMES if transitions is None else transitions,
            "mismatches": mismatches,
            "revision_changes": revision_changes,
            "waypoints": waypoints,
            "two_leg_frames": two_leg_frames,
            "snap_classes": {
                "pin": 200,
                "wire_end": 150,
                "junction": 100 if sheet == "dense" else 0,
                "wire_body": 0,
                "grid": 150 if sheet == "dense" else 250,
            },
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
    fp = {"sha256": "deadbeef", "commit": "7337b30", "tracked_status": ""}
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
                sheet = case.split("-", 1)[1]
                ms = overrides.get((tag, case), 200.0 if sheet == "empty" else 240.0)
                report = make_start(sheet, viewport, ms)
            else:
                sheet, script = case.split("-", 1)
                kw = overrides.get((tag, case), {})
                report = make_motion(sheet, script, scale, viewport, **kw)
            stem.with_suffix(".json").write_text(json.dumps(report), encoding="utf-8")


class GateTest(unittest.TestCase):
    def test_args_for_splits_hyphenated_scripts(self):
        self.assertEqual(
            gate.args_for("start-empty", "1920x1080"),
            (["start", "--sheet", "empty", "--runs", "10", "--size", "1920x1080"], False),
        )
        self.assertEqual(
            gate.args_for("1k-drag-wire", "1536x864"),
            (
                [
                    "run",
                    "--sheet",
                    "1k",
                    "--script",
                    "drag-wire",
                    "--warmup",
                    str(gate.FRAMES),
                    "--frames",
                    str(gate.FRAMES),
                    "--size",
                    "1536x864",
                ],
                True,
            ),
        )
        self.assertEqual(
            gate.args_for("dense-drag-wire", "1920x1080"),
            (
                [
                    "run",
                    "--sheet",
                    "dense",
                    "--script",
                    "drag-wire",
                    "--warmup",
                    str(gate.FRAMES),
                    "--frames",
                    str(gate.FRAMES),
                    "--size",
                    "1920x1080",
                ],
                True,
            ),
        )
        self.assertEqual(
            gate.args_for("10k-zoom", "1920x1080")[0][1:5],
            ["--sheet", "10k", "--script", "zoom"],
        )

    def test_schedule_covers_all_cases_and_rotates(self):
        order = gate.schedule()
        # Each scale: 10 * (2 starts + 6 motion) = 80; total = 160.
        self.assertEqual(len(order), 160)
        for tag in gate.SCALES:
            expected_cases = [*gate.STARTS, *gate.MOTION]
            first_cases = []
            for rep in range(1, gate.REPETITIONS + 1):
                cases = [c for t, r, c in order if t == tag and r == rep]
                self.assertEqual(sorted(cases), sorted(expected_cases))
                first_cases.append(cases[0])
                start_order = [c for c in cases if c in gate.STARTS]
                expected_starts = list(gate.STARTS[::-1] if (rep - 1) % 2 else gate.STARTS)
                self.assertEqual(start_order, expected_starts)
            # 7 rotating groups (start pair + 6 motion cases).
            self.assertEqual(len(set(first_cases[:7])), 7)

    def test_passing_campaign(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp))
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertTrue(ok, out)
            self.assertIn("PASS B6 1k-drag-wire dirty-to-present p99", out)
            self.assertIn("PASS B6 dense-drag-wire dirty-to-present p99", out)
            self.assertIn("PASS B3 1k-drag-wire frame p99", out)
            self.assertIn("PASS B3 dense-drag-wire frame p99", out)
            self.assertIn("PASS INV5 1k-drag-wire max revision changes", out)
            self.assertIn("PASS INV11 1k-drag-wire max SheetView paints", out)
            self.assertIn("PASS INV11 1k-drag-wire max press SheetView paints", out)
            self.assertIn("PASS INV11 1k-drag-wire max warm-up SheetView paints", out)
            self.assertIn("PASS INV11 dense-drag-wire max SheetView paints", out)

    def test_b6_at_exact_one_refresh_passes(self):
        exact_refresh = 1000.0 / 60.008
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(
                Path(tmp),
                {
                    ("s100", "1k-drag-wire"): {
                        "d2p_p99": exact_refresh,
                        "d2p_max": exact_refresh + 1.0,
                    },
                    ("s125", "dense-drag-wire"): {
                        "d2p_p99": exact_refresh,
                        "d2p_max": exact_refresh + 1.0,
                    },
                },
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertTrue(ok, buf.getvalue())
            self.assertIn("PASS B6 1k-drag-wire dirty-to-present p99", buf.getvalue())
            self.assertIn("PASS B6 dense-drag-wire dirty-to-present p99", buf.getvalue())

    def test_drag_wire_d2p_over_one_refresh_fails_b6(self):
        for case in ("1k-drag-wire", "dense-drag-wire"):
            with tempfile.TemporaryDirectory() as tmp:
                write_campaign(Path(tmp), {("s100", case): {"d2p_p99": 16.7, "d2p_max": 20.0}})
                buf = io.StringIO()
                with redirect_stdout(buf):
                    ok = gate.summarize(Path(tmp))
                self.assertFalse(ok)
                self.assertIn(f"FAIL B6 {case} dirty-to-present p99", buf.getvalue())

    def test_b2_startup_delta_over_300_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "start-1k"): 510.0})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL B2 startup delta", buf.getvalue())

    def test_drag_wire_frame_p99_over_b3_fails_gate(self):
        for tag, case in (("s100", "1k-drag-wire"), ("s125", "dense-drag-wire")):
            with tempfile.TemporaryDirectory() as tmp:
                write_campaign(Path(tmp), {(tag, case): {"p99": 16.8, "maximum": 20.0}})
                buf = io.StringIO()
                with redirect_stdout(buf):
                    ok = gate.summarize(Path(tmp))
                self.assertFalse(ok)
                self.assertIn(f"FAIL B3 {case} frame p99", buf.getvalue())

    def test_b5_worst_frame_over_limit_fails_on_drag_wire(self):
        for tag, case in (("s100", "1k-drag-wire"), ("s125", "dense-drag-wire")):
            with tempfile.TemporaryDirectory() as tmp:
                write_campaign(Path(tmp), {(tag, case): {"maximum": 33.4}})
                buf = io.StringIO()
                with redirect_stdout(buf):
                    ok = gate.summarize(Path(tmp))
                self.assertFalse(ok)
                self.assertIn(f"FAIL B5 {case} worst frame", buf.getvalue())

    def test_b9_memory_over_300_fails_on_10k_pan(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "10k-pan"): {"memory": 300.1}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL B9 10k-pan private memory", buf.getvalue())

    def test_traced_counted_interval_uses_1_5r_for_drag_wire_and_2_5r_for_10k(self):
        # A 2.0R counted interval fails 1k-drag-wire and dense-drag-wire (1.5R B3 limit)
        # while passing 10k-pan (2.5R B4 limit).
        trace_2r_wire = make_trace(11, late_measured_index=10, late_interval_refreshes=2.0)
        trace_2r_pan = make_trace(0, late_measured_index=10, late_interval_refreshes=2.0)
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(
                Path(tmp),
                {
                    ("s100", "1k-drag-wire"): {"trace": trace_2r_wire, "missed_refreshes": 1},
                    ("s100", "dense-drag-wire"): {"trace": trace_2r_wire, "missed_refreshes": 1},
                    ("s100", "10k-pan"): {"trace": trace_2r_pan, "missed_refreshes": 1},
                },
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertFalse(ok)
            self.assertIn("FAIL B3 1k-drag-wire attribution 0/10 passed", out)
            self.assertIn("FAIL B3 dense-drag-wire attribution 0/10 passed", out)
            self.assertIn("PASS B4 10k-pan attribution 10/10 passed", out)

        # A 2.6R counted interval fails 10k-pan under its 2.5R B4 limit.
        trace_2_6r_pan = make_trace(0, late_measured_index=10, late_interval_refreshes=2.6)
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(
                Path(tmp),
                {
                    ("s125", "10k-pan"): {
                        "trace": trace_2_6r_pan,
                        "missed_refreshes": 1,
                        "over_2_5_refreshes": 1,
                    },
                },
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL B4 10k-pan attribution 0/10 passed", buf.getvalue())

    def test_mismatched_startup_draws_fails_attribution(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "dense-drag-wire"): {"startup_draws": 10}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertFalse(ok)
            self.assertIn("FAIL B3 dense-drag-wire attribution 0/10 passed", out)
            self.assertIn("trace unusable", out)

    def test_inv5_and_inv11_fail_as_verdicts_not_incomplete(self):
        cases = [
            (
                {"sheet_paints": 1},
                "FAIL INV11 dense-drag-wire max SheetView paints",
            ),
            (
                {
                    "press_sheet_paints": 1,
                    "warnings": ["INV11: SheetView painted 1 times during wire press, expected 0"],
                },
                "FAIL INV11 dense-drag-wire max press SheetView paints",
            ),
            (
                {
                    "warmup_sheet_paints": 2,
                    "warnings": ["INV11: SheetView painted 2 times during wire warm-up, expected 0"],
                },
                "FAIL INV11 dense-drag-wire max warm-up SheetView paints",
            ),
            (
                {
                    "revision_changes": 1,
                    "warnings": [
                        "INV5: document revision changed on 1 measured frames during drag-wire"
                    ],
                },
                "FAIL INV5 dense-drag-wire max revision changes",
            ),
        ]
        for override, expected in cases:
            with self.subTest(expected=expected):
                with tempfile.TemporaryDirectory() as tmp:
                    write_campaign(Path(tmp), {("s125", "dense-drag-wire"): override})
                    buf = io.StringIO()
                    with redirect_stdout(buf):
                        ok = gate.summarize(Path(tmp))
                    self.assertFalse(ok)
                    self.assertIn(expected, buf.getvalue())

    def test_drag_wire_allocs_over_zero_fails_b10(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "1k-drag-wire"): {"ours": 1}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            self.assertFalse(ok)
            self.assertIn("FAIL B10 1k-drag-wire max Omnia allocations/frame", buf.getvalue())

    def test_drag_wire_incomplete_exercise_and_unexpected_warnings_rejected(self):
        cases = [
            ({"mismatches": 1}, "incomplete drag-wire exercise"),
            ({"transitions": gate.FRAMES - 1}, "incomplete drag-wire exercise"),
            ({"waypoints": 2}, "incomplete drag-wire exercise"),
            ({"two_leg_frames": 0}, "incomplete drag-wire exercise"),
            ({"sheet_paints": 0.5}, "invalid count"),
            ({"omit_d2p": True}, "p99"),
            (
                {"warnings": ["window was not active: present intervals are not recorded"]},
                "incomplete frame sampling",
            ),
            (
                # A spoofed invariant warning when revision_changes == 0 must not be ignored.
                {
                    "warnings": [
                        "INV5: document revision changed on 1 measured frames during drag-wire"
                    ]
                },
                "incomplete frame sampling",
            ),
        ]
        for override, pattern in cases:
            with self.subTest(override=override):
                with tempfile.TemporaryDirectory() as tmp:
                    write_campaign(Path(tmp), {("s100", "1k-drag-wire"): override})
                    with self.assertRaisesRegex(ValueError, pattern):
                        gate.summarize(Path(tmp))


if __name__ == "__main__":
    unittest.main()
