"""Compare completed Orion campaigns (canvas plan 2.1, 10, and Q4)."""

import argparse
import gzip
import itertools
import json
import math
from pathlib import Path
from statistics import median

SCALES = {"s100": (1.0, [1920, 1080]), "s125": (1.25, [1536, 864])}
CASES = tuple(itertools.product(SCALES, ("1k", "10k"), ("pan", "zoom")))
REPETITIONS = 10
FRAMES = 600
MAX_REGRESSION = 0.10


def read_report(stem):
    plain = stem.with_name(f"{stem.name}.json")
    compressed = stem.with_name(f"{stem.name}.json.gz")
    if plain.is_file() == compressed.is_file():
        raise ValueError(f"expected exactly one report for {stem}")
    if compressed.is_file():
        with gzip.open(compressed, "rt", encoding="utf-8") as report:
            return json.load(report)
    return json.loads(plain.read_text(encoding="utf-8"))


def check_conditions(state, scale):
    screen = state["panel"]
    if (
        screen["scale"] != scale
        or screen["size"] != {"width": 1920, "height": 1080}
        or not 59.9 <= screen["refreshRate"] <= 60.1
    ):
        raise ValueError(f"expected the 1920x1080 60 Hz panel at scale {scale}: {screen}")
    if "1" not in state["mains_online"].values():
        raise ValueError("reference runs require AC power")
    if not state["governor"] or not all(state["governor"].values()):
        raise ValueError("reference runs require recorded CPU governors")


def check_report(report, case):
    scale_label, sheet, script = case
    scale, viewport = SCALES[scale_label]
    config = report["config"]
    environment = report["environment"]
    gpu = environment["gpu"] or {}
    gpu_names = f"{gpu.get('device_name', '')} {gpu.get('driver_name', '')}".lower()
    if (
        report["schema"] != 1
        or report["kind"] != "run"
        or config["sheet"] != sheet
        or config["script"] != script
        or config["window_logical"] != viewport
        or config["frames"] != FRAMES
        or config["warmup"] != FRAMES
        or config.get("diagnostic_trace")
        or report.get("diagnostic_trace") is not None
        or report["frames"]["drawn"] != FRAMES
        or report["frames"]["unmeasured"] != 0
        or report["warnings"] != []
        or environment["os"] != "linux"
        or not environment["fullscreen"]
        or environment["scale_factor"] != scale
        or environment["viewport_logical"] != viewport
        or environment["viewport_device"] != [1920, 1080]
        or not 59.9 <= (environment["refresh_hz"] or 0) <= 60.1
        or gpu.get("is_software_emulated") is not False
        or not all(name in gpu_names for name in ("intel", "620", "mesa"))
    ):
        raise ValueError("unexpected run configuration or Orion conditions")
    value = report["metrics"]["frame_ms"]["p99"]
    if not math.isfinite(value) or value <= 0:
        raise ValueError("invalid frame p99")
    return value


def load_case(directory, case):
    scale_label, sheet, script = case
    scale, _ = SCALES[scale_label]
    samples = []
    configurations = []
    for repetition in range(1, REPETITIONS + 1):
        stem = directory / scale_label / f"{sheet}-{script}-{repetition}"
        report = read_report(stem)
        try:
            for phase in ("before", "after"):
                conditions = read_report(stem.with_name(f"{stem.name}.{phase}"))
                check_conditions(conditions, scale)
            samples.append(check_report(report, case))
        except (ValueError, KeyError, TypeError) as error:
            raise ValueError(f"{stem}: {error}") from error
        configurations.append(report["config"])
    if any(config != configurations[0] for config in configurations):
        raise ValueError(f"inconsistent configurations for {' '.join(case)}")
    return configurations[0], samples


def repository_directory(value):
    root = Path.cwd().resolve()
    path = (root / value).resolve()
    if not path.is_relative_to(root) or not path.is_dir():
        raise argparse.ArgumentTypeError("expected a directory inside the repository")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=repository_directory)
    parser.add_argument("candidate", type=repository_directory)
    args = parser.parse_args()
    if args.baseline == args.candidate:
        parser.error("baseline and candidate must be different campaigns")
    regressions = False
    try:
        for case in CASES:
            baseline_config, baseline = load_case(args.baseline, case)
            candidate_config, candidate = load_case(args.candidate, case)
            if baseline_config != candidate_config:
                raise ValueError(f"campaign configurations differ for {' '.join(case)}")
            base_p99, candidate_p99 = median(baseline), median(candidate)
            change = (candidate_p99 / base_p99 - 1) * 100
            regressed = candidate_p99 > base_p99 * (1 + MAX_REGRESSION)
            regressions |= regressed
            verdict = "REGRESSION" if regressed else "PASS"
            print(
                f"{' '.join(case)}: {base_p99:.4f} [{min(baseline):.4f}, {max(baseline):.4f}]"
                f" -> {candidate_p99:.4f} [{min(candidate):.4f}, {max(candidate):.4f}] ms"
                f" ({change:+.2f}%) {verdict}"
            )
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.error(str(error))
    return int(regressions)


if __name__ == "__main__":
    raise SystemExit(main())
