"""Compare frame p99 on Orion under canvas plan section 2.1 (T2.6)."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

from compare_orion import FRAMES, REPETITIONS, SCALES, check_conditions, check_report

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

    mains = {
        str(supply): (supply / "online").read_text().strip()
        for supply in sorted(Path("/sys/class/power_supply").iterdir())
        if (supply / "type").read_text().strip() == "Mains"
    }
    profile = subprocess.run(["powerprofilesctl", "get"], capture_output=True, text=True) \
        if Path("/usr/bin/powerprofilesctl").exists() else None
    return {
        "panel": panel(),
        "mains_online": mains,
        "governor": read("devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor"),
        "power_profile": profile.stdout.strip() if profile and profile.returncode == 0 else None,
        "platform_profile": read("firmware/acpi/platform_profile"),
        "processes": command("ps", "-eo", "pid,comm").splitlines(),
    }


def campaign(binaries, output, env):
    cases = [(revision, sheet, script) for sheet in ("1k", "10k")
             for script in ("pan", "zoom") for revision in binaries]
    order = [(label, rep + 1, *case) for label in SCALES for rep in range(REPETITIONS)
             for case in cases[rep % len(cases):] + cases[:rep % len(cases)]]
    dump(output / "order.json", order)
    previous_scale = None
    for label, rep, revision, sheet, script in order:
        scale, size = SCALES[label]
        if scale != previous_scale:
            set_scale(scale)
        previous_scale = scale
        case_dir = output / revision / label
        case_dir.mkdir(parents=True, exist_ok=True)
        key = f"{sheet}-{script}-{rep}"
        print(f"{scale} {revision} {key}", flush=True)
        path = case_dir / f"{key}.json"
        before = snapshot()
        dump(case_dir / f"{key}.before.json", before)
        check_conditions(before, scale)
        with (case_dir / f"{key}.log").open("w") as log:
            result = subprocess.run([
                binaries[revision], "run", "--sheet", sheet, "--script", script,
                "--size", f"{size[0]}x{size[1]}", "--warmup", str(FRAMES),
                "--frames", str(FRAMES), "--json", path,
            ], env=env, stdout=log, stderr=subprocess.STDOUT)
        after = snapshot()
        dump(case_dir / f"{key}.after.json", after)
        result.check_returncode()
        check_conditions(after, scale)
        report = json.loads(path.read_text(encoding="utf-8"))
        try:
            check_report(report, (label, sheet, script))
        except (ValueError, KeyError, TypeError) as error:
            raise ValueError(f"{path}: {error}") from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    if any(not path.is_file() or not os.access(path, os.X_OK) for path in binaries.values()):
        parser.error("build both release bench executables before running the campaign")
    args.output = args.output.resolve()
    if not args.output.is_relative_to(Path.cwd().resolve()):
        parser.error("choose a new output directory inside the repository")
    cpu = Path("/proc/cpuinfo").read_text(encoding="utf-8")
    if "i5-7300U" not in cpu or os.environ.get("XDG_SESSION_TYPE") != "wayland":
        parser.error("this comparison requires Orion's Wayland desktop session")
    original_scale = panel()["scale"]
    args.output.mkdir(parents=True)
    status = args.output / "status.txt"
    status.write_text("incomplete\n", encoding="utf-8")

    def hashes():
        return {name: command("sha256sum", path) for name, path in binaries.items()}

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
        campaign(binaries, args.output, env)
    finally:
        set_scale(original_scale)
        after = hashes()
        dump(args.output / "binaries-after.json", after)
    if before != after:
        raise ValueError("bench binary changed during the campaign")
    comparator = Path(__file__).with_name("compare_orion.py")
    comparison = subprocess.run([
        sys.executable, comparator, args.output / "baseline", args.output / "candidate"
    ])
    if comparison.returncode in (0, 1):
        status.write_text("pass\n" if comparison.returncode == 0 else "fail\n", encoding="utf-8")
    return comparison.returncode


if __name__ == "__main__":
    raise SystemExit(main())
