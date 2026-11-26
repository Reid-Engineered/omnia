#!/usr/bin/env python3
"""Run or replay the T7.5 final canvas budget gate (B1 to B10, canvas plan 2.1)."""

import argparse
from collections import defaultdict
from contextlib import redirect_stdout
from datetime import datetime, timezone
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import signal
from statistics import median
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "bench-baseline"))
from compare_orion import FRAMES, REPETITIONS, SCALES, check_conditions, read_report
from reference import PANEL, command, dump, set_scale, snapshot

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "T1.6"))
from attribute import attribute, describe

STARTS = ("start-empty", "start-1k")
MOTION = (
    "1k-pan",
    "1k-zoom",
    "10k-pan",
    "10k-zoom",
    "1k-drag-part",
    "10k-drag-part",
    "1k-drag-wire",
    "dense-drag-wire",
)
IDLES = ("idle-empty",)
CPU_CASES = ("hit-10k",)
SCENE_COUNTS = {
    "1k": (1000, 2000),
    "dense": (1000, 12000),
    "10k": (10000, 20000),
}
START_SAMPLES = 10
IDLE_SECONDS = 10
IDLE_SETTLE_MS = 1000
HIT_SAMPLES = 20000
HIT_SEED = 42
HIT_TOLERANCE_NM = 508000
GRID_NM = 2540000
CPU = "i5-7300U"


def schedule():
    # Rotate the startup pair as one group, reversing it each repetition to balance B2.
    # CPU-only B7 (`hit-10k`) runs 10 interleaved repetitions during the s100 pass.
    order = []
    for tag in SCALES:
        groups = [STARTS, *((case,) for case in MOTION), *((case,) for case in IDLES)]
        if tag == "s100":
            groups.extend((case,) for case in CPU_CASES)
        for index in range(REPETITIONS):
            offset = index % len(groups)
            for group in groups[offset:] + groups[:offset]:
                if group == STARTS and index % 2:
                    group = group[::-1]
                order.extend([tag, index + 1, case] for case in group)
    return order


def args_for(case, size):
    """Build bench CLI arguments (excluding `--json`) and whether tracing is enabled."""
    if case in STARTS:
        return [
            "start",
            "--sheet",
            case.split("-", 1)[1],
            "--runs",
            str(START_SAMPLES),
            "--size",
            size,
        ], False
    if case in IDLES:
        return [
            "idle",
            "--sheet",
            case.split("-", 1)[1],
            "--seconds",
            str(IDLE_SECONDS),
            "--settle-ms",
            str(IDLE_SETTLE_MS),
            "--size",
            size,
        ], False
    if case in CPU_CASES:
        return [
            "hit",
            "--sheet",
            "10k",
            "--samples",
            str(HIT_SAMPLES),
            "--seed",
            str(HIT_SEED),
        ], False
    sheet, script = case.split("-", 1)
    return [
        "run",
        "--sheet",
        sheet,
        "--script",
        script,
        "--warmup",
        str(FRAMES),
        "--frames",
        str(FRAMES),
        "--size",
        size,
    ], True


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def fingerprint(binary):
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {
        "sha256": digest,
        "commit": command("git", "-C", ROOT, "rev-parse", "HEAD"),
        "tracked_status": command(
            "git", "-C", ROOT, "status", "--porcelain", "--untracked-files=no"
        ),
    }


def active_panel():
    outputs = json.loads(command("kscreen-doctor", "--json"))["outputs"]
    native = next((output for output in outputs if output["name"] == PANEL), None)
    if native is None or not native["enabled"]:
        raise ValueError(f"expected the native {PANEL} panel to be enabled")
    return native


def capture():
    active_panel()
    conditions = snapshot()
    conditions["time_utc"] = datetime.now(timezone.utc).isoformat()
    conditions["dpms"] = command("kscreen-doctor", "--dpms", "show")
    return conditions


def validate_conditions(conditions, scale):
    check_conditions(conditions, scale)
    governors = conditions["governor"]
    if len(governors) != 4 or not conditions["processes"] or not conditions["time_utc"]:
        raise ValueError("expected all four CPU governors and a timestamped process snapshot")
    if set(governors.values()) != {"performance"}:
        raise ValueError(f"expected performance CPU governor on all four CPUs: {governors}")
    if not any(
        line.strip().lower() == "dpms mode for screen edp-1: on"
        for line in conditions["dpms"].splitlines()
    ):
        raise ValueError(f"expected active eDP-1 panel: {conditions['dpms']}")


def collect(binary, output):
    if platform.system() != "Linux" or os.environ.get("XDG_SESSION_TYPE") != "wayland":
        raise ValueError("run on Orion in its local Wayland session")
    cpu = next(
        line.split(":", 1)[1].strip()
        for line in Path("/proc/cpuinfo").read_text().splitlines()
        if line.startswith("model name")
    )
    if CPU not in cpu or not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("expected Orion's CPU and a prebuilt release bench executable")
    source_status = command("git", "-C", ROOT, "status", "--porcelain")
    if source_status:
        raise ValueError("run from a clean source checkout")
    initial_governors = capture()["governor"]
    if len(initial_governors) != 4 or set(initial_governors.values()) != {"performance"}:
        raise ValueError(
            f"expected performance CPU governor on all four CPUs: {initial_governors}"
        )
    original_scale = active_panel()["scale"]
    order = schedule()
    output.mkdir(parents=True)
    (output / "status.txt").write_text("incomplete\n", encoding="utf-8")
    for tag in SCALES:
        (output / tag).mkdir()
    dump(
        output / "hardware.json",
        {"cpu": cpu, "os": command("uname", "-a"), "session": "wayland", "panel": "eDP-1"},
    )
    dump(output / "source.json", {"binary": str(binary), "source_status": source_status})
    dump(output / "order.json", order)
    dump(output / "binary-before.json", fingerprint(binary))

    env = {**os.environ, "OMNIA_BENCH_FULLSCREEN": "1"}
    env.pop("WAYLAND_DEBUG", None)
    env.pop("OMNIA_BENCH_TRACE", None)
    previous_tag = None
    try:
        with (output / "runs.jsonl").open("x", encoding="utf-8") as journal:
            for tag, repetition, case in order:
                scale, viewport = SCALES[tag]
                if tag != previous_tag:
                    set_scale(scale)
                    previous_tag = tag
                stem = output / tag / f"{case}-{repetition}"
                size = f"{viewport[0]}x{viewport[1]}"
                path = stem.with_suffix(".json")
                args, traced_run = args_for(case, size)
                run_env = {**env, "OMNIA_BENCH_TRACE": "1"} if traced_run else env
                print(f"{tag} {repetition}/{REPETITIONS} {case}", flush=True)
                identity = [tag, repetition, case]
                before = capture()
                write_record(journal, identity, "before", before, None)
                validate_conditions(before, scale)
                with stem.with_suffix(".log").open("xb") as log:
                    result = subprocess.run(
                        [binary, *args, "--json", path],
                        env=run_env,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                    )
                after = capture()
                write_record(journal, identity, "after", after, result.returncode)
                result.check_returncode()
                validate_conditions(after, scale)
    finally:
        try:
            set_scale(original_scale)
        finally:
            dump(output / "binary-after.json", fingerprint(binary))


def write_record(journal, case, phase, conditions, exit_code):
    record = {"case": case, "phase": phase, "conditions": conditions, "exit": exit_code}
    journal.write(json.dumps(record) + "\n")
    journal.flush()


def metric(value):
    if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
        raise ValueError(f"invalid metric: {value!r}")
    return value


def integer_count(value):
    if type(value) is not int or value < 0:
        raise ValueError(f"invalid count: {value!r}")
    return value


def check_gui_env(environment, scale, viewport):
    gpu = environment["gpu"]
    if (
        environment["fullscreen"] is not True
        or environment["scale_factor"] != scale
        or environment["viewport_logical"] != viewport
        or environment["viewport_device"] != [1920, 1080]
        or not 59.9 <= environment["refresh_hz"] <= 60.1
        or gpu["is_software_emulated"] is not False
        or "HD Graphics 620" not in gpu["device_name"]
        or "Mesa" not in gpu["driver_name"]
    ):
        raise ValueError(
            f"expected the reference viewport, refresh and Vulkan adapter: {environment}"
        )


def startup(report, sheet, viewport):
    if report["config"] != {"sheet": sheet, "runs": START_SAMPLES, "window_logical": viewport}:
        raise ValueError("unexpected startup configuration")
    samples = report["samples_ms"]
    if len(samples) != START_SAMPLES or any(metric(value) == 0 for value in samples):
        raise ValueError("expected ten positive startup samples")
    return median(samples), max(samples)


def idle_empty(report, scale, viewport):
    expected_config = {
        "sheet": "empty",
        "seconds": IDLE_SECONDS,
        "settle_ms": IDLE_SETTLE_MS,
        "window_logical": viewport,
    }
    if report["config"] != expected_config:
        raise ValueError(f"unexpected idle configuration: {report['config']}")
    check_gui_env(report["environment"], scale, viewport)
    metrics = report["metrics"]
    frames = integer_count(metrics["frames"])
    renders = integer_count(metrics["renders"])
    cpu_pct = metric(metrics["cpu_percent"])
    measured_s = metric(report["measured_s"])
    if measured_s < IDLE_SECONDS - 0.5:
        raise ValueError(f"idle window measured too briefly: {measured_s}")
    return frames, renders, cpu_pct


def hit_10k(report):
    expected_config = {
        "sheet": "10k",
        "seed": HIT_SEED,
        "samples": HIT_SAMPLES,
        "tolerance_nm": HIT_TOLERANCE_NM,
        "grid_nm": GRID_NM,
    }
    if report["config"] != expected_config:
        raise ValueError(f"unexpected hit configuration: {report['config']}")
    if report["environment"] != {"os": "linux", "arch": "x86_64", "window": None}:
        raise ValueError(f"unexpected hit environment: {report['environment']}")
    if report["parts"] != 10000 or report["wires"] != 20000 or report["hits"] <= 0:
        raise ValueError("unexpected 10k hit counts")
    p99 = metric(report["metrics"]["hit_ms"]["p99"])
    if p99 == 0:
        raise ValueError("expected positive hit p99")
    return p99


def motion(report, sheet, script, scale, viewport):
    config = {
        "sheet": sheet,
        "script": script,
        "frames": FRAMES,
        "warmup": FRAMES,
        "window_logical": viewport,
        "scene_counts": False,
        "diagnostic_trace": True,
    }
    parts, wires = SCENE_COUNTS[sheet]
    if report["config"] != config or report["scene_details"] != {
        "seed": 42,
        "parts": parts,
        "wires": wires,
    }:
        raise ValueError("unexpected motion configuration or stress sheet")

    environment = report["environment"]
    check_gui_env(environment, scale, viewport)

    refresh_ms = 1000.0 / environment["refresh_hz"]
    frames = report["frames"]
    metrics = report["metrics"]
    expected_sampling = (
        "overlay-every-frame" if script in ("drag-part", "drag-wire") else "every-frame"
    )

    sheet_paints = None
    press_sheet_paints = None
    aux_sheet_paints = None
    revision_changes = None
    d2p_p99 = None
    allowed_warnings = set()

    if script == "drag-part":
        sheet_paints = integer_count(metrics["sheet_paints"])
        drag = report.get("drag") or {}
        press_sheet_paints = integer_count(drag.get("press_sheet_paints"))
        aux_sheet_paints = integer_count(drag.get("lift_sheet_paints"))
        revision_changes = integer_count(drag.get("revision_changes"))
        if (
            drag.get("overlay_paints") != FRAMES
            or drag.get("sheet_paints") != sheet_paints
            or drag.get("transitions") != FRAMES
            or drag.get("mismatches") != 0
            or drag.get("waypoints", 0) < 4
        ):
            raise ValueError(f"incomplete drag-part exercise: {drag}")
        if press_sheet_paints != 0:
            allowed_warnings.add(
                f"INV11: SheetView painted {press_sheet_paints} times during drag press, expected 0"
            )
        if aux_sheet_paints != 1:
            allowed_warnings.add(
                f"D7: SheetView painted {aux_sheet_paints} times during warm-up lift-off, expected 1"
            )
        if revision_changes > 0:
            allowed_warnings.add(
                f"INV5: document revision changed on {revision_changes} measured frames during drag"
            )
        d2p = metrics.get("dirty_to_present_ms") or {}
        d2p_p99 = metric(d2p["p99"])
        d2p_max = metric(d2p["max"])
        if not 0 < d2p_p99 <= d2p_max:
            raise ValueError(f"invalid dirty-to-present p99 or maximum: {d2p_p99}, {d2p_max}")
    elif script == "drag-wire":
        sheet_paints = integer_count(metrics["sheet_paints"])
        wire = report.get("wire") or {}
        press_sheet_paints = integer_count(wire.get("press_sheet_paints"))
        aux_sheet_paints = integer_count(wire.get("warmup_sheet_paints"))
        revision_changes = integer_count(wire.get("revision_changes"))
        two_leg_frames = integer_count(wire.get("two_leg_frames"))
        if (
            wire.get("overlay_paints") != FRAMES
            or wire.get("sheet_paints") != sheet_paints
            or wire.get("transitions") != FRAMES
            or wire.get("mismatches") != 0
            or wire.get("waypoints", 0) < 4
            or two_leg_frames == 0
        ):
            raise ValueError(f"incomplete drag-wire exercise: {wire}")
        if press_sheet_paints != 0:
            allowed_warnings.add(
                f"INV11: SheetView painted {press_sheet_paints} times during wire press, expected 0"
            )
        if aux_sheet_paints != 0:
            allowed_warnings.add(
                f"INV11: SheetView painted {aux_sheet_paints} times during wire warm-up, expected 0"
            )
        if revision_changes > 0:
            allowed_warnings.add(
                f"INV5: document revision changed on {revision_changes} measured frames during drag-wire"
            )
        d2p = metrics.get("dirty_to_present_ms") or {}
        d2p_p99 = metric(d2p["p99"])
        d2p_max = metric(d2p["max"])
        if not 0 < d2p_p99 <= d2p_max:
            raise ValueError(f"invalid dirty-to-present p99 or maximum: {d2p_p99}, {d2p_max}")

    unexpected_warnings = [w for w in report["warnings"] if w not in allowed_warnings]
    if (
        unexpected_warnings
        or frames["drawn"] != FRAMES
        or frames["unmeasured"] != 0
        or frames["on_time"] + frames["late"] != FRAMES
        or metrics["paint_sampling"] != expected_sampling
        or metrics["paint_samples"] != FRAMES
    ):
        raise ValueError(f"incomplete frame sampling: {frames}, warnings {report['warnings']}")

    p99 = metric(metrics["frame_ms"]["p99"])
    maximum = metric(metrics["frame_ms"]["max"])
    if not 0 < p99 <= maximum:
        raise ValueError(f"invalid frame p99 or maximum: {p99}, {maximum}")
    ours = metric(metrics["allocs_per_frame"]["ours"]["max"])
    memory = None
    if sheet == "10k":
        memory = metric(metrics["memory_mb"])
        if memory == 0:
            raise ValueError("expected positive private memory")
    limit = 2.5 if sheet == "10k" else 1.5
    traced = attribute(report, limit)
    return (
        p99,
        maximum,
        d2p_p99,
        refresh_ms,
        ours,
        memory,
        sheet_paints,
        press_sheet_paints,
        aux_sheet_paints,
        revision_changes,
        traced,
    )


def med_range(values):
    return f"{median(values):.3f} [{min(values):.3f}, {max(values):.3f}]"


def summarize(output):
    order = schedule()
    if load(output / "order.json") != order:
        raise ValueError("recorded order does not match the frozen schedule")
    records = [
        json.loads(line)
        for line in (output / "runs.jsonl").read_text(encoding="utf-8").splitlines()
    ]
    expected_records = [(case, phase) for case in order for phase in ("before", "after")]
    if [(record["case"], record["phase"]) for record in records] != expected_records:
        raise ValueError("incomplete campaign or run journal out of order")
    before = load(output / "binary-before.json")
    if before != load(output / "binary-after.json") or before["tracked_status"]:
        raise ValueError("benchmark executable or source checkout changed")
    hardware = load(output / "hardware.json")
    if (
        CPU not in hardware["cpu"]
        or not hardware["os"].startswith("Linux ")
        or hardware["session"] != "wayland"
        or hardware["panel"] != "eDP-1"
    ):
        raise ValueError("unexpected reference machine or session")
    print(
        f"Orion: {hardware['cpu']}, {hardware['os']}, {hardware['session']}, "
        f"{hardware['panel']}, source {before['commit']}, binary SHA-256 {before['sha256']}"
    )
    starts = {}
    idles = defaultdict(list)
    hits = []
    runs = defaultdict(list)
    attribution = []
    conditions = defaultdict(set)
    adapters = set()
    run_pairs = zip(records[::2], records[1::2], strict=True)
    for (pre, post), (tag, repetition, case) in zip(run_pairs, order, strict=True):
        scale, viewport = SCALES[tag]
        stem = output / tag / f"{case}-{repetition}"
        try:
            if pre["exit"] is not None or post["exit"] != 0:
                raise ValueError(
                    f"unexpected benchmark exit records: {pre['exit']}, {post['exit']}"
                )
            for record in (pre, post):
                state = record["conditions"]
                validate_conditions(state, scale)
                conditions[tag].add(
                    (
                        tuple(sorted(set(state["governor"].values()))),
                        state["power_profile"],
                        tuple(state["platform_profile"].values()),
                    )
                )
            report = read_report(stem)
            if case in STARTS:
                kind = "start"
            elif case in IDLES:
                kind = "idle"
            elif case in CPU_CASES:
                kind = "hit"
            else:
                kind = "run"
            if (
                report["schema"] != 1
                or report["kind"] != kind
                or report["environment"]["os"] != "linux"
            ):
                raise ValueError("unexpected report schema, kind or OS")
            if kind == "start":
                starts[tag, repetition, case] = startup(report, case.split("-", 1)[1], viewport)
            elif kind == "idle":
                idles[tag].append(idle_empty(report, scale, viewport))
                adapters.add(
                    (
                        report["environment"]["gpu"]["device_name"],
                        report["environment"]["gpu"]["driver_name"],
                    )
                )
            elif kind == "hit":
                hits.append(hit_10k(report))
            else:
                sheet, script = case.split("-", 1)
                runs[tag, case].append(motion(report, sheet, script, scale, viewport))
                adapters.add(
                    (
                        report["environment"]["gpu"]["device_name"],
                        report["environment"]["gpu"]["driver_name"],
                    )
                )
                attribution.extend(describe(f"{tag}/{stem.name}", runs[tag, case][-1][-1]))
        except (OSError, TypeError, KeyError, ValueError) as error:
            raise ValueError(f"{stem}: {error}") from error

    print(f"Vulkan device and driver: {sorted(adapters)}")
    results = []

    def verdict(passed, summary):
        results.append(passed)
        print(f"{'PASS' if passed else 'FAIL'} {summary}")

    verdict(
        median(hits) < 0.2,
        f"B7 hit-10k p99 {med_range(hits)} ms (< 0.2)",
    )

    for tag, (scale, viewport) in SCALES.items():
        print(f"\n{tag}: scale {scale}, {viewport}, native 1920x1080 at 60 Hz, AC power")
        print(
            f"CPU governors, power profile and platform profile: {sorted(conditions[tag], key=str)}"
        )
        empty_meds = [starts[tag, rep, "start-empty"][0] for rep in range(1, REPETITIONS + 1)]
        empty_maxes = [starts[tag, rep, "start-empty"][1] for rep in range(1, REPETITIONS + 1)]
        verdict(
            median(empty_meds) < 400,
            f"B1 start-empty median {med_range(empty_meds)} ms (< 400; worst sample {max(empty_maxes):.3f})",
        )
        deltas = [
            starts[tag, rep, "start-1k"][0] - starts[tag, rep, "start-empty"][0]
            for rep in range(1, REPETITIONS + 1)
        ]
        verdict(median(deltas) < 300, f"B2 startup delta {med_range(deltas)} ms (< 300)")

        idle_frames, idle_renders, idle_cpus = zip(*idles[tag], strict=True)
        verdict(
            max(idle_frames) == 0 and max(idle_renders) == 0,
            f"B8 idle-empty max frames {max(idle_frames)} (= 0), max renders {max(idle_renders)} (= 0), "
            f"CPU % {med_range(idle_cpus)}",
        )

        for case in MOTION:
            sheet, script = case.split("-", 1)
            if sheet == "10k":
                budget, p99_limit, max_limit = "B4", 33.3, 50
            else:
                budget, p99_limit, max_limit = "B3", 16.7, 33.3
            (
                p99s,
                maxes,
                d2p_p99s,
                refresh_ms_vals,
                allocs,
                memory,
                sheet_paints,
                press_paints,
                aux_paints,
                rev_changes,
                traced,
            ) = zip(*runs[tag, case], strict=True)
            if script in ("drag-part", "drag-wire"):
                refresh_limit = min(refresh_ms_vals)
                verdict(
                    median(d2p_p99s) <= refresh_limit,
                    f"B6 {case} dirty-to-present p99 {med_range(d2p_p99s)} ms "
                    f"(<= {refresh_limit:.3f})",
                )
            verdict(
                median(p99s) < p99_limit,
                f"{budget} {case} frame p99 {med_range(p99s)} ms (< {p99_limit})",
            )
            passed = sum(result["passed"] for result in traced)
            verdict(
                passed == REPETITIONS,
                f"{budget} {case} attribution {passed}/{REPETITIONS} passed",
            )
            verdict(
                max(maxes) <= max_limit,
                f"B5 {case} worst frame {max(maxes):.3f} ms (<= {max_limit})",
            )
            if script == "drag-part":
                verdict(
                    max(rev_changes) == 0,
                    f"INV5 {case} max revision changes {max(rev_changes):g} (= 0)",
                )
                verdict(
                    max(sheet_paints) == 0,
                    f"INV11 {case} max SheetView paints {max(sheet_paints):g} (= 0)",
                )
                verdict(
                    max(press_paints) == 0,
                    f"INV11 {case} max press SheetView paints {max(press_paints):g} (= 0)",
                )
                verdict(
                    all(p == 1 for p in aux_paints),
                    f"D7 {case} warm-up lift-off SheetView paints {sorted(set(aux_paints))} (= [1])",
                )
            elif script == "drag-wire":
                verdict(
                    max(rev_changes) == 0,
                    f"INV5 {case} max revision changes {max(rev_changes):g} (= 0)",
                )
                verdict(
                    max(sheet_paints) == 0,
                    f"INV11 {case} max SheetView paints {max(sheet_paints):g} (= 0)",
                )
                verdict(
                    max(press_paints) == 0,
                    f"INV11 {case} max press SheetView paints {max(press_paints):g} (= 0)",
                )
                verdict(
                    max(aux_paints) == 0,
                    f"INV11 {case} max warm-up SheetView paints {max(aux_paints):g} (= 0)",
                )
            verdict(
                max(allocs) == 0,
                f"B10 {case} max Omnia allocations/frame {max(allocs):g} (= 0)",
            )
            if sheet == "10k":
                verdict(
                    max(memory) < 300,
                    f"B9 {case} private memory {med_range(memory)} MB (all < 300)",
                )
    print("\nMissed-refresh attribution")
    print("\n".join(attribution))
    return all(results)


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    run = sub.add_parser("run", help="capture and summarize a fresh Orion campaign")
    run.add_argument("output", type=Path)
    run.add_argument("--bench", type=Path, default=ROOT / "target/release/omnia-bench")
    replay = sub.add_parser(
        "summarize", help="verify an existing campaign without running benchmarks"
    )
    replay.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        output = args.output.resolve()
        if args.action == "run":
            signal.signal(signal.SIGTERM, interrupted)
            collect(args.bench.resolve(), output)
            summary = io.StringIO()
            with redirect_stdout(summary):
                passed = summarize(output)
            (output / "summary.txt").write_text(summary.getvalue(), encoding="utf-8")
            (output / "status.txt").write_text(
                "pass\n" if passed else "fail\n", encoding="utf-8"
            )
            print(f"{'pass' if passed else 'fail'}: {output / 'summary.txt'}")
        else:
            passed = summarize(output)
        return int(not passed)
    except KeyboardInterrupt:
        print("campaign interrupted", file=sys.stderr)
        return 130
    except StopIteration:
        print("incomplete: could not identify the reference CPU or panel mode", file=sys.stderr)
        return 2
    except (OSError, subprocess.CalledProcessError, KeyError, TypeError, ValueError) as error:
        print(f"incomplete: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
