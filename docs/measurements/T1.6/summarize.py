#!/usr/bin/env python3
"""Validate the T1.6 gate results against canvas-plan 2.1 and print the gate tables.

Run from the repository root: python3 docs/measurements/T1.6/summarize.py
Exit 0 when every check holds and every budget passes, 1 otherwise.
"""
import json, pathlib, statistics, sys

ROOT = pathlib.Path(__file__).parent
SCALES = {"s100": (1.0, [1920.0, 1080.0]), "s125": (1.25, [1536.0, 864.0])}
CASES = ["start", "pan", "zoom", "idle"]
problems = []


def check(ok, what):
    if not ok:
        problems.append(what)


def med_range(xs):
    return f"{statistics.median(xs):.3f} [{min(xs):.3f}, {max(xs):.3f}]"


def env_ok(where, env, scale, viewport):
    gpu = env.get("gpu") or {}
    check("HD Graphics 620" in (gpu.get("device_name") or ""), f"{where}: device {gpu}")
    check(gpu.get("is_software_emulated") is False, f"{where}: software emulated")
    check("Mesa" in (gpu.get("driver_name") or ""), f"{where}: driver {gpu.get('driver_name')}")
    check(env.get("fullscreen") is True, f"{where}: not fullscreen")
    check(env.get("scale_factor") == scale, f"{where}: scale {env.get('scale_factor')}")
    check(env.get("viewport_logical") == viewport, f"{where}: viewport {env.get('viewport_logical')}")
    check(abs((env.get("refresh_hz") or 0) - 60.0) < 0.1, f"{where}: refresh {env.get('refresh_hz')}")


verdicts = []
for tag, (scale, viewport) in SCALES.items():
    d = ROOT / tag
    reports = {c: [json.loads((d / f"{c}-{r}.json").read_text()) for r in range(1, 11)] for c in CASES}
    # Power records: every one on AC, governors recorded, panel awake at the right scale.
    order = []
    for line in (d / "runs.jsonl").read_text().splitlines():
        rec = json.loads(line)
        k = rec["key"]
        check(all(v == "1" for p, v in rec["ac_online"].items() if "AC" in p), f"{tag}/{k}: not on AC")
        check(rec["exit"] in (None, 0), f"{tag}/{k}: exit {rec['exit']}")
        check(rec["edp"] and rec["edp"]["scale"] == scale, f"{tag}/{k}: panel scale {rec['edp']}")
        check(rec["edp"] and rec["edp"]["size"] == {"width": 1920, "height": 1080}, f"{tag}/{k}: mode")
        check("on" in (rec["dpms"] or "").lower(), f"{tag}/{k}: dpms {rec['dpms']}")
        if rec["phase"] == "before":
            order.append(k)
    check(len(order) == 40, f"{tag}: {len(order)} runs")
    for r in range(10):
        expected = [CASES[(j + r) % 4] + f"-{r + 1}" for j in range(4)]
        check(order[4 * r:4 * r + 4] == expected, f"{tag}: pass {r + 1} order {order[4 * r:4 * r + 4]}")
    govs = sorted({v for line in (d / "runs.jsonl").read_text().splitlines()
                   for v in json.loads(line)["cpu_governor"].values()})
    profiles = sorted({str(json.loads(line)["power_profile"]) + "/" + str(json.loads(line)["platform_profile"])
                       for line in (d / "runs.jsonl").read_text().splitlines()})

    print(f"\n## {tag}: scale {scale}, {viewport[0]:.0f}x{viewport[1]:.0f} logical; "
          f"governors {govs}; power-profiles-daemon/platform_profile {profiles}\n")
    # B3, B10, B5 (info)
    print("| Script | frame p99 ms, median [min, max] | draw p99 ms | present interval p50 ms | "
          "max frame ms | late frames | allocs ours max | allocs total mean |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- |")
    for script in ["pan", "zoom"]:
        rs = reports[script]
        for i, rep in enumerate(rs, 1):
            where = f"{tag}/{script}-{i}"
            env_ok(where, rep["environment"], scale, viewport)
            check(rep["warnings"] == [], f"{where}: warnings {rep['warnings']}")
            check(rep["frames"]["drawn"] == 600, f"{where}: drew {rep['frames']['drawn']}")
            check(rep["config"]["sheet"] == "empty" and rep["config"]["script"] == script, where)
        m = [rep["metrics"] for rep in rs]
        p99 = [x["frame_ms"]["p99"] for x in m]
        pi50 = [x["present_interval_ms"]["p50"] for x in m]
        refresh_ms = 1000.0 / rs[0]["environment"]["refresh_hz"]
        ours = max(x["allocs_per_frame"]["ours"]["max"] for x in m)
        print(f"| {script} | {med_range(p99)} | {med_range([x['draw_ms']['p99'] for x in m])} | "
              f"{med_range(pi50)} | {med_range([x['frame_ms']['max'] for x in m])} | "
              f"{med_range([x['late_frames'] for x in m])} | {ours:g} | "
              f"{med_range([x['allocs_per_frame']['total']['mean'] for x in m])} |")
        verdicts.append((f"B3 {script} {tag} frame p99", f"median of run p99s "
                         f"{statistics.median(p99):.3f} ms, limit < 16.7", statistics.median(p99) < 16.7))
        # Strict, no tolerance: plan B3 says "median present interval <= one refresh".
        pi = statistics.median(pi50)
        verdicts.append((f"B3 {script} {tag} present interval", f"median of run p50s {pi:.3f} ms, "
                         f"limit <= {refresh_ms:.3f} (1/{rs[0]['environment']['refresh_hz']} Hz); "
                         f"over by {max(0.0, pi - refresh_ms) * 1000:.0f} us", pi <= refresh_ms))
        verdicts.append((f"B10 {script} {tag}", f"allocs_per_frame.ours max over 10 runs = {ours:g}", ours == 0))
    # B1
    starts = reports["start"]
    meds = [x["metrics"]["start_ms"]["median"] for x in starts]
    maxes = [x["metrics"]["start_ms"]["max"] for x in starts]
    samples = [s for x in starts for s in x["samples_ms"]]
    for i, x in enumerate(starts, 1):
        check(x["config"]["sheet"] == "empty" and len(x["samples_ms"]) == 10, f"{tag}/start-{i}")
    print(f"\nB1 start: per-run median {med_range(meds)} ms; per-run max {med_range(maxes)} ms; "
          f"all {len(samples)} starts {min(samples):.1f}..{max(samples):.1f} ms")
    verdicts.append((f"B1 start {tag}", f"median of run medians {statistics.median(meds):.1f} ms < 400 "
                     f"(slowest of {len(samples)} starts: {max(samples):.1f} ms)",
                     statistics.median(meds) < 400))
    # B8
    idles = reports["idle"]
    frames = [x["metrics"]["frames"] for x in idles]
    for i, x in enumerate(idles, 1):
        env_ok(f"{tag}/idle-{i}", x["environment"], scale, viewport)
        check(x["config"].get("sheet") == "empty", f"{tag}/idle-{i}: sheet")
    cpu = [x["metrics"].get("cpu_percent", float("nan")) for x in idles]
    print(f"B8 idle: frames per run {frames}; renders {[x['metrics']['renders'] for x in idles]}; "
          f"CPU % {med_range(cpu)}; startup frames {[x['startup']['frames'] for x in idles]}")
    verdicts.append((f"B8 idle {tag}", f"frames in 10 s, every run: {sorted(set(frames))}",
                     all(f == 0 for f in frames)))

print("\n## Gate\n\n| Budget | Measured | Result |\n| --- | --- | --- |")
for name, text, ok in verdicts:
    print(f"| {name} | {text} | {'PASS' if ok else 'FAIL'} |")
print(f"\nProtocol checks: {'all passed' if not problems else str(len(problems)) + ' failed'}")
for p in problems:
    print(" -", p)
sys.exit(0 if not problems and all(ok for *_, ok in verdicts) else 1)
