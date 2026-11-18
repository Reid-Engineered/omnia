"""Compare frame p99 on Orion under canvas plan section 2.1 (T2.6)."""

import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import time


REPEATS = 10
FRAMES = 600
METRIC = "frame_ms.p99"
PANEL = "eDP-1"


def command(*args):
    return subprocess.check_output(args, text=True).strip()


def dump(path, data):
    path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")


def panel():
    screen = json.loads(command("kscreen-doctor", "--json"))
    output = next(o for o in screen["outputs"] if o["name"] == PANEL)
    mode = next(m for m in output["modes"] if m["id"] == output["currentModeId"])
    return {"scale": output["scale"], **mode}


def set_scale(scale):
    subprocess.run(["kscreen-doctor", f"output.{PANEL}.scale.{scale}"], check=True)
    time.sleep(3)


def snapshot():
    def read(pattern):
        return {str(p): p.read_text().strip() for p in sorted(Path("/sys").glob(pattern))}

    profile = subprocess.run(["powerprofilesctl", "get"], capture_output=True, text=True) \
        if Path("/usr/bin/powerprofilesctl").exists() else None
    return {
        "panel": panel(),
        "power_online": read("class/power_supply/*/online"),
        "governor": read("devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor"),
        "power_profile": profile.stdout.strip() if profile and profile.returncode == 0 else None,
        "platform_profile": read("firmware/acpi/platform_profile"),
        "processes": command("ps", "-eo", "pid,comm").splitlines(),
    }


def check_conditions(state, scale):
    screen = state["panel"]
    if (screen["scale"] != scale or screen["size"] != {"width": 1920, "height": 1080}
            or not 59.9 <= screen["refreshRate"] <= 60.1):
        raise ValueError(f"expected the 1920x1080 60 Hz panel at scale {scale}: {screen}")
    if "1" not in state["power_online"].values():
        raise ValueError("reference runs require AC power")


def check_report(report, scale, size):
    env = report["environment"]
    gpu = env.get("gpu") or {}
    if (env["os"] != "linux" or not env["fullscreen"] or env["scale_factor"] != scale
            or env["viewport_logical"] != list(size) or env["viewport_device"] != [1920, 1080]
            or not 59.9 <= (env["refresh_hz"] or 0) <= 60.1):
        raise ValueError(f"invalid reference window: {env}")
    gpu_names = f"{gpu.get('device_name', '')} {gpu.get('driver_name', '')}".lower()
    if gpu.get("is_software_emulated") is not False or not all(
        name in gpu_names for name in ("intel", "620", "mesa")
    ):
        raise ValueError(f"expected the Intel HD 620 Mesa Vulkan adapter: {gpu}")
    if (report["frames"]["drawn"] != FRAMES or report["frames"]["unmeasured"] != 0
            or report["warnings"]):
        raise ValueError(f"incomplete frame sample: {report['frames']}, {report['warnings']}")


def campaign(binaries, output, env):
    cases = [(revision, sheet, script) for sheet in ("1k", "10k")
             for script in ("pan", "zoom") for revision in binaries]
    sizes = {1.0: (1920, 1080), 1.25: (1536, 864)}
    order = [(scale, rep + 1, *case) for scale in sizes for rep in range(REPEATS)
             for case in cases[rep % len(cases):] + cases[:rep % len(cases)]]
    dump(output / "order.json", order)
    samples = {}
    previous_scale = None
    for scale, rep, revision, sheet, script in order:
        if scale != previous_scale:
            set_scale(scale)
        previous_scale = scale
        key = f"{scale}-{rep:02}-{revision}-{sheet}-{script}"
        print(key, flush=True)
        size = sizes[scale]
        path = output / f"{key}.json"
        before = snapshot()
        dump(output / f"{key}.before.json", before)
        check_conditions(before, scale)
        with (output / f"{key}.log").open("w") as log:
            result = subprocess.run([
                binaries[revision], "run", "--sheet", sheet, "--script", script,
                "--size", f"{size[0]}x{size[1]}", "--warmup", str(FRAMES),
                "--frames", str(FRAMES), "--json", path,
            ], env=env, stdout=log, stderr=subprocess.STDOUT)
        after = snapshot()
        dump(output / f"{key}.after.json", after)
        result.check_returncode()
        check_conditions(after, scale)
        report = json.loads(path.read_text(encoding="utf-8"))
        check_report(report, scale, size)
        samples.setdefault((scale, revision, sheet, script), []).append(report)
    return samples


def compare(binaries, output, samples):
    passed = True
    for (scale, revision, sheet, script), reports in samples.items():
        values = [r["metrics"]["frame_ms"]["p99"] for r in reports]
        median = statistics.median(values)
        print(f"{scale} {revision} {sheet} {script}: p99 {median:.4f} ms "
              f"({min(values):.4f}–{max(values):.4f})", flush=True)
        if len(values) != REPEATS or any(
            r["config"] != reports[0]["config"] or r["environment"] != reports[0]["environment"]
            for r in reports
        ):
            raise ValueError("mixed or incomplete samples")
        report = {key: reports[0][key] for key in ("schema", "kind", "config", "environment")}
        report["metrics"] = {"frame_ms": {"p99": median}}
        report["run_p99_range_ms"] = [min(values), max(values)]
        dump(output / f"{scale}-{sheet}-{script}-{revision}-median.json", report)

    for scale, revision, sheet, script in samples:
        if revision == "baseline":
            case = output / f"{scale}-{sheet}-{script}"
            status = subprocess.call([
                binaries["candidate"], "compare", f"{case}-baseline-median.json",
                f"{case}-candidate-median.json", "--metrics", METRIC,
                "--max-regression", "10", "--json", f"{case}-compare.json",
            ])
            passed &= status == 0
    return passed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    cpu = Path("/proc/cpuinfo").read_text(encoding="utf-8")
    if "i5-7300U" not in cpu or os.environ.get("XDG_SESSION_TYPE") != "wayland":
        parser.error("this comparison requires Orion's Wayland desktop session")
    original_scale = panel()["scale"]
    args.output.mkdir(parents=True)
    status = args.output / "status.txt"
    status.write_text("incomplete\n", encoding="utf-8")
    hashes = lambda: {name: command("sha256sum", path) for name, path in binaries.items()}
    before = hashes()
    dump(args.output / "binaries-before.json", before)
    dump(args.output / "hardware.json", {
        "cpu": next(line.split(":", 1)[1].strip() for line in cpu.splitlines()
                    if line.startswith("model name")),
        "os": command("uname", "-a"),
        "session": os.environ["XDG_SESSION_TYPE"],
    })
    env = {**os.environ, "OMNIA_BENCH_FULLSCREEN": "1"}
    env.pop("OMNIA_BENCH_TRACE", None)
    try:
        samples = campaign(binaries, args.output, env)
    finally:
        set_scale(original_scale)
        after = hashes()
        dump(args.output / "binaries-after.json", after)
    if before != after:
        raise ValueError("bench binary changed during the campaign")
    passed = compare(binaries, args.output, samples)
    status.write_text("pass\n" if passed else "fail\n", encoding="utf-8")
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
