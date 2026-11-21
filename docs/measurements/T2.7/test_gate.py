"""Exercise T2.7 evidence replay without running a timed campaign."""

from contextlib import redirect_stdout
import io
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

import gate


class CampaignTest(unittest.TestCase):
    def test_schedule_rotates_and_balances_startup_pairs(self):
        order = gate.schedule()
        self.assertEqual(len(order), 120)
        self.assertEqual(sum(case in gate.MOTION for _, _, case in order), 80)
        for tag in gate.SCALES:
            first_cases = set()
            for repetition in range(1, 11):
                cases = [case for scale, rep, case in order if (scale, rep) == (tag, repetition)]
                first_cases.add(cases[0])
                index = min(cases.index(case) for case in gate.STARTS)
                pair = gate.STARTS if repetition % 2 else gate.STARTS[::-1]
                self.assertEqual(cases[index:index + 2], list(pair))
                self.assertCountEqual(cases, (*gate.STARTS, *gate.MOTION))
            self.assertEqual(len(first_cases.intersection(gate.MOTION)), 4)

    def test_replay_checks_all_motion_runs_and_rejects_incomplete_evidence(self):
        order = gate.schedule()
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp)
            gate.dump(output / "order.json", order)
            gate.dump(output / "hardware.json", {
                "cpu": gate.CPU, "os": "Linux reference", "session": "wayland", "panel": "eDP-1",
            })
            fingerprint = {"sha256": "a" * 64, "commit": "b" * 40, "tracked_status": ""}
            for phase in ("before", "after"):
                gate.dump(output / f"binary-{phase}.json", fingerprint)
            for tag in gate.SCALES:
                (output / tag).mkdir()
            with (output / "runs.jsonl").open("w", encoding="utf-8") as journal:
                for tag, repetition, case in order:
                    scale, viewport = gate.SCALES[tag]
                    conditions = {
                        "panel": {"size": {"width": 1920, "height": 1080},
                                  "scale": scale, "refreshRate": 60},
                        "mains_online": {"AC": "1"}, "governor": {str(i): "powersave" for i in range(4)},
                        "dpms": "dpms mode for screen eDP-1: on", "processes": ["bench"],
                        "time_utc": "2026-11-17T00:00:00+00:00",
                        "power_profile": None, "platform_profile": {},
                    }
                    gate.write_record(journal, [tag, repetition, case], "before", conditions, None)
                    gate.write_record(journal, [tag, repetition, case], "after", conditions, 0)
                    path = output / tag / f"{case}-{repetition}.json"
                    gate.dump(path, report_for(case, scale, viewport))

            verdict = {"passed": True}
            with patch.object(gate, "attribute", return_value=verdict) as attribute, \
                    patch.object(gate, "describe", return_value=[]), redirect_stdout(io.StringIO()):
                self.assertTrue(gate.summarize(output))
                limits = [call.args[1] for call in attribute.call_args_list]
                self.assertEqual(limits.count(1.5), 40)
                self.assertEqual(limits.count(2.5), 40)

                path = output / "s125" / "10k-zoom-10.json"
                report = gate.load(path)
                report["metrics"]["frame_ms"]["max"] = 50.001
                gate.dump(path, report)
                self.assertFalse(gate.summarize(output))

                report["metrics"]["frame_ms"]["max"] = 50
                report["metrics"]["memory_mb"] = 300
                gate.dump(path, report)
                self.assertFalse(gate.summarize(output))

                path.unlink()
                with self.assertRaisesRegex(ValueError, "10k-zoom-10"):
                    gate.summarize(output)


def report_for(case, scale, viewport):
    start = case in gate.STARTS
    environment = {"os": "linux"}
    report = {"schema": 1, "kind": "start" if start else "run", "environment": environment}
    if start:
        sheet = case.split("-")[1]
        report["config"] = {"sheet": sheet, "runs": 10, "window_logical": viewport}
        report["samples_ms"] = [300 if sheet == "empty" else 400] * 10
        return report

    sheet, script = case.split("-")
    parts = 1000 if sheet == "1k" else 10000
    report["config"] = {"sheet": sheet, "script": script, "frames": 600, "warmup": 600,
                        "window_logical": viewport, "scene_counts": False, "diagnostic_trace": True}
    report["scene_details"] = {"seed": 42, "parts": parts, "wires": 2 * parts}
    environment.update(fullscreen=True, scale_factor=scale, viewport_logical=viewport,
                       viewport_device=[1920, 1080], refresh_hz=60,
                       gpu={"device_name": "HD Graphics 620", "driver_name": "Mesa",
                            "is_software_emulated": False})
    report["frames"] = {"drawn": 600, "unmeasured": 0, "on_time": 600, "late": 0}
    report["warnings"] = []
    report["metrics"] = {"paint_sampling": "every-frame", "paint_samples": 600,
                         "frame_ms": {"p99": 12, "max": 20}, "memory_mb": 200,
                         "allocs_per_frame": {"ours": {"max": 0}}}
    return report


if __name__ == "__main__":
    unittest.main()
