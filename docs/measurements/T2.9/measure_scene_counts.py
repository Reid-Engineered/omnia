#!/usr/bin/env python3
"""Capture `--features scene-counts` primitive counts on 10k zoom at key far-zoom frames."""

import argparse
import json
import math
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
FRAMES = (376, 420, 461, 500, 545)
VIEWPORTS = (
    ("s100", "1920x1080"),
    ("s125", "1536x864"),
)
MIN_ZOOM, MAX_ZOOM, ZOOM_PERIOD = 0.05, 32.0, 600


def zoom_at(frame: int) -> float:
    hi, lo = math.log2(MAX_ZOOM), math.log2(MIN_ZOOM)
    t = (frame % ZOOM_PERIOD) / ZOOM_PERIOD * 2 * (hi - lo)
    if t < hi:
        log = t
    elif t < hi + (hi - lo):
        log = hi - (t - hi)
    else:
        log = lo + (t - hi - (hi - lo))
    return min(max(2 ** log, MIN_ZOOM), MAX_ZOOM)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument(
        "--bench",
        type=Path,
        default=ROOT / "target/release/omnia-bench.exe",
    )
    args = parser.parse_args()
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    bench = args.bench.resolve()

    summary = []
    for tag, size in VIEWPORTS:
        for frame in FRAMES:
            path = out / f"{tag}-f{frame}.json"
            cmd = [
                str(bench),
                "run",
                "--sheet",
                "10k",
                "--script",
                "zoom",
                "--warmup",
                str(frame),
                "--frames",
                "1",
                "--size",
                size,
                "--json",
                str(path),
            ]
            proc = subprocess.run(cmd, capture_output=True, text=True)
            if proc.returncode != 0:
                print(f"FAIL {tag} f{frame}: exit {proc.returncode}\n{proc.stderr}", file=sys.stderr)
                return 1
            data = json.loads(path.read_text(encoding="utf-8"))
            pr = data["metrics"]["primitives"]
            env = data["environment"]
            alloc = data["metrics"]["allocs_per_frame"]["ours"]["max"]
            zoom = zoom_at(frame)
            row = {
                "tag": tag,
                "requested_size": size,
                "frame": frame,
                "zoom_pct": round(zoom * 100.0, 2),
                "quads": pr["quads"],
                "monochrome_sprites": pr["monochrome_sprites"],
                "paint_operations": pr["paint_operations"],
                "batches": pr["batches"],
                "ours_allocs_max": alloc,
                "scale_factor": env["scale_factor"],
                "viewport_logical": env["viewport_logical"],
                "viewport_device": env["viewport_device"],
                "refresh_hz": env["refresh_hz"],
                "gpu": env["gpu"]["device_name"],
            }
            summary.append(row)
            print(
                f"{tag} f{frame} ({zoom * 100:5.2f}%): quads={pr['quads']:5d} "
                f"sprites={pr['monochrome_sprites']:4d} ops={pr['paint_operations']:5d} "
                f"batches={pr['batches']} ours={alloc:g} sf={env['scale_factor']} "
                f"vp={env['viewport_logical']} gpu={env['gpu']['device_name']}",
                flush=True,
            )
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
