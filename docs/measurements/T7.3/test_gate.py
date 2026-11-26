#!/usr/bin/env python3
"""Unit tests for the T7.3 instrument window stub Orion gate script."""

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


def make_trace(late_measured_index=None, late_interval_refreshes=None):
    refresh = 16_664_444
    events = []
    t = 0
    prev_present_end = None
    for i in range(gate.FRAMES * 2):
        measured_idx = i - gate.FRAMES
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


TRACE_TEMPLATE = make_trace()


def make_motion(
    case,
    scale,
    viewport,
    *,
    p99=3.0,
    maximum=6.0,
    ours=0,
    memory=160.0,
    paints=None,
    ticks=None,
    renders=None,
    stride=None,
    trace=None,
    missed_refreshes=0,
    over_2_5_refreshes=0,
    warnings=None,
):
    hz = gate.case_hz(case)
    expected_stride = 60 // hz
    expected_paints = gate.FRAMES // expected_stride
    return {
        "schema": 1,
        "kind": "run",
        "config": {
            "sheet": "10k",
            "script": "pan",
            "frames": gate.FRAMES,
            "warmup": gate.FRAMES,
            "window_logical": viewport,
            "scene_counts": False,
            "diagnostic_trace": True,
            "instrument": True,
            "instrument_hz": hz,
        },
        "scene_details": {"seed": 42, "parts": 10_000, "wires": 20_000},
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
            "startup_draws": 0,
        },
        "warnings": list(warnings) if warnings is not None else [],
        "instrument": {
            "hz": hz,
            "stride": expected_stride if stride is None else stride,
            "paints": expected_paints if paints is None else paints,
            "renders": expected_paints if renders is None else renders,
            "ticks": expected_paints if ticks is None else ticks,
            "size_logical": [576.0, 468.0],
        },
        "metrics": {
            "paint_sampling": "every-frame",
            "paint_samples": gate.FRAMES,
            "frame_ms": {"p99": p99, "max": maximum},
            "allocs_per_frame": {"ours": {"max": ours}},
            "missed_refreshes": missed_refreshes,
            "present_intervals_over_2_5_refreshes": over_2_5_refreshes,
            "memory_mb": memory,
        },
        "diagnostic_trace": TRACE_TEMPLATE if trace is None else trace,
    }


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
    fp = {"sha256": "deadbeef", "commit": "8712d9b", "tracked_status": ""}
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
            kw = overrides.get((tag, case), {})
            report = make_motion(case, scale, viewport, **kw)
            stem.with_suffix(".json").write_text(json.dumps(report), encoding="utf-8")


class GateTest(unittest.TestCase):
    def test_schedule_interleaves_60hz_and_30hz_across_both_scales(self):
        order = gate.schedule()
        self.assertEqual(len(order), 40)
        for tag in gate.SCALES:
            for rep in range(1, gate.REPETITIONS + 1):
                cases = [c for t, r, c in order if t == tag and r == rep]
                expected = (
                    ["10k-pan-inst60", "10k-pan-inst30"]
                    if rep % 2
                    else ["10k-pan-inst30", "10k-pan-inst60"]
                )
                self.assertEqual(cases, expected)

    def test_args_for_builds_instrument_cli_flags(self):
        self.assertEqual(
            gate.args_for("10k-pan-inst60", "1920x1080"),
            (
                [
                    "run",
                    "--sheet",
                    "10k",
                    "--script",
                    "pan",
                    "--instrument",
                    "--instrument-hz",
                    "60",
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
            gate.args_for("10k-pan-inst30", "1536x864")[0][:9],
            ["run", "--sheet", "10k", "--script", "pan", "--instrument", "--instrument-hz", "30", "--warmup"],
        )

    def test_passing_campaign(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp))
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertTrue(ok, out)
            self.assertIn("PASS T7.3 10k-pan-inst60 frame p99", out)
            self.assertIn("PASS T7.3 10k-pan-inst30 frame p99", out)
            self.assertIn("T7.3 done-when (60 Hz p99 < 16 ms): PASS", out)
            self.assertIn("T7.3 done-when (30 Hz p99 < 16 ms): PASS", out)
            self.assertIn("Additional budgets (B4/B5/B9/B10, 60 Hz): PASS", out)
            self.assertIn("Additional budgets (B4/B5/B9/B10, 30 Hz): PASS", out)

    def test_60hz_over_16ms_fails_while_recording_30hz_fallback(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(
                Path(tmp),
                {
                    ("s100", "10k-pan-inst60"): {"p99": 16.1, "maximum": 20.0},
                    ("s125", "10k-pan-inst60"): {"p99": 16.1, "maximum": 20.0},
                },
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertFalse(ok)
            self.assertIn("FAIL T7.3 10k-pan-inst60 frame p99", out)
            self.assertIn("PASS T7.3 10k-pan-inst30 frame p99", out)
            self.assertIn("T7.3 done-when (60 Hz p99 < 16 ms): FAIL", out)
            self.assertIn("T7.3 done-when (30 Hz p99 < 16 ms): PASS", out)
            self.assertIn("Additional budgets (B4/B5/B9/B10, 30 Hz): PASS", out)
            self.assertIn("30 Hz fallback passed T7.3 done-when and all additional budgets", out)

    def test_b9_and_b10_and_attribution_failures(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s100", "10k-pan-inst60"): {"ours": 1}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertFalse(ok)
            self.assertIn("FAIL B10 10k-pan-inst60 max Omnia allocations/frame", out)
            self.assertIn("T7.3 done-when (60 Hz p99 < 16 ms): PASS", out)
            self.assertIn("Additional budgets (B4/B5/B9/B10, 60 Hz): FAIL", out)

        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(Path(tmp), {("s125", "10k-pan-inst60"): {"memory": 300.5}})
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertFalse(ok)
            self.assertIn("FAIL B9 10k-pan-inst60 private memory", out)
            self.assertIn("T7.3 done-when (60 Hz p99 < 16 ms): PASS", out)
            self.assertIn("Additional budgets (B4/B5/B9/B10, 60 Hz): FAIL", out)

        trace_2_6r = make_trace(late_measured_index=10, late_interval_refreshes=2.6)
        with tempfile.TemporaryDirectory() as tmp:
            write_campaign(
                Path(tmp),
                {
                    ("s100", "10k-pan-inst60"): {
                        "trace": trace_2_6r,
                        "missed_refreshes": 1,
                        "over_2_5_refreshes": 1,
                    }
                },
            )
            buf = io.StringIO()
            with redirect_stdout(buf):
                ok = gate.summarize(Path(tmp))
            out = buf.getvalue()
            self.assertFalse(ok)
            self.assertIn("FAIL B4 10k-pan-inst60 2.5R attribution 0/10 passed", out)
            self.assertIn("T7.3 done-when (60 Hz p99 < 16 ms): PASS", out)
            self.assertIn("Additional budgets (B4/B5/B9/B10, 60 Hz): FAIL", out)

    def test_incomplete_instrument_exercise_and_warnings_fail_closed(self):
        for override, pattern in [
            ({"paints": gate.FRAMES - 1}, "instrument exercise"),
            ({"ticks": gate.FRAMES - 1}, "instrument exercise"),
            ({"stride": 2}, "instrument exercise"),
            ({"warnings": ["unexpected"]}, "incomplete frame sampling"),
        ]:
            with self.subTest(override=override):
                with tempfile.TemporaryDirectory() as tmp:
                    write_campaign(Path(tmp), {("s100", "10k-pan-inst60"): override})
                    with self.assertRaisesRegex(ValueError, pattern):
                        gate.summarize(Path(tmp))


if __name__ == "__main__":
    unittest.main()
