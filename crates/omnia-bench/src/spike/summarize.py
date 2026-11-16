"""Summarize the raw JSON without changing samples. Run from the repository root."""
from pathlib import Path
from collections import defaultdict
from statistics import median
import json

root=Path("docs/measurements/T0.4")
reports={str(p.relative_to(root).with_suffix("")).replace("\\","/"):json.loads(p.read_text(encoding="utf-8"))
         for p in root.rglob("*.json") if p.stem.startswith(("s","baseline-"))}
groups=defaultdict(list)
for key,r in reports.items():
    if r.get("kind")=="spike":
        groups[key.rsplit("-",1)[0]].append((key,r))
out=["# T0.4 spike measurements","",
"Provisional, non-reference host (Q4). No threshold or budget is marked met.",
"",
"Hardware: Windows 11; Intel Core i7-14650HX (24 logical CPUs); NVIDIA RTX 5060 Laptop plus Intel UHD.",
"The renderer reports Intel UHD Graphics, driver 32.0.101.7088. Display scale 125%, 60 Hz.",
"Viewport 3072x1728 logical / 3840x2160 device, enforced using WindowOptions.window_min_size.",
"Live 150%: not measured on this machine; to do on the reference machine (Q4).",
"No-GPU render_to_image: not measured on this machine; to do on the reference machine (Q4).",
"Visible monitor migration was measured: 125% to 100% and back to 125%.",
"",
"Build: cargo build --release -p omnia-bench --features scene-counts,readback.",
"Primitive/atlas observations require scene-counts (GPUI bench-support); readback enables Windows test-support only as an opt-in feature.",
"Reproduce with pwsh -NoProfile -File crates/omnia-bench/src/spike/measure.ps1. Three repetitions; 60 warmup + 600 measured frames each.",
"The base includes PR #4's corrected pre-draw baseline snapshot. Pre-rebase pilot runs were discarded.",
"",
"## Probe definitions and limits","",
"- S1 counts wire-shaped quads plus the live 20 logical-pixel dot grid; quads-only adds exact threshold loads without the grid. Paper adds one quad.",
"- S2 paints square, rounded, and one cached SVG dot at 3 logical pixels.",
"- S3 uses the lexically first 30 vendored SVGs, each in all eight orientations, at a fixed 24 logical-pixel footprint. The sweep requests all 39 raster buckets: 4 pixels, then the quarter-octave ladder through 2560 pixels. It keeps the projected footprint at 24 pixels to isolate cache growth from overdraw. Camera motion still advances one device pixel per frame.",
"- S3 atlas page sizes are inferred from the first observed tile on each texture ID and the pinned DirectX max(1024, requested tile size) allocation rule; no public atlas byte counter exists. Monochrome pages use one byte/pixel; subpixel pages use four. This is allocated texture payload, excluding driver overhead, and assumes no hidden monochrome allocations in these otherwise bare probes.",
"- S4 rebuilds a stroked zigzag resistor path each frame. PathBuilder/build/paint_path allocations are GPUI calls and therefore total, following Q5; own allocations are reported separately.",
"- S5 counts six-glyph labels, not glyphs: 2k/6k/12k labels mean 12k/36k/72k glyphs. Bundled Inter SemiBold, fixed 17 pixels, then 6.5..17 pixels in 0.5-pixel buckets; lines are cached per label/bucket. This spike retains all bucket shapes to separate shaping from the second atlas pass; production D6 retains one current bucket and will use less CPU heap. Direct mode emits the same shaped glyphs without per-label paint_layer. Both use the same baseline.",
"- S6 holds the camera fixed on a real cached child sheet while the parent overlay animates each frame. Repaint mode explicitly notifies the sheet; cached mode does not. paint_calls proves whether paint was skipped. Trace frame indices count scene paint invocations, so a cached scene has fewer rows than driver frames. Replay cost is Window::draw, not the absent sheet paint timer.",
"- All GPUI entry calls suspend own allocation counting. Paint traces include cold size transitions; warm fixed runs exclude preparation. Atlas observation/reporting belongs to the harness, outside own paint. Its work remains included in draw timings.",
"- S8 uses a real hidden application window, draw().clear(), then render_to_image inside cx.spawn. A bare paper Element is the only content. Nested start (10 child processes) and idle (10 seconds) reports use the T0.3 harness.",
"",
"## Repeated timings","",
"Values are median [minimum, maximum] of run p99s, in milliseconds. p50 range is shown because this host is bimodal/noisy. This is not a confidence interval.",
"",
"| Probe | Runs | draw p99 median [range] | draw p50 median [range] | paint p99 median | ours alloc/frame max |",
"|---|---:|---:|---:|---:|---:|"]
for name,items in sorted(groups.items()):
    ds=[r["metrics"]["draw_ms"]["p99"] for _,r in items]
    p50=[r["metrics"]["draw_ms"]["p50"] for _,r in items]
    ps=[r["metrics"].get("paint_ms",{}).get("p99") for _,r in items]
    ps=[p for p in ps if p is not None]
    ours=max(r["metrics"]["allocs_per_frame"]["ours"]["max"] for _,r in items)
    out.append(f"| {name} | {len(items)} | {median(ds):.3f} [{min(ds):.3f}, {max(ds):.3f}] | {median(p50):.3f} [{min(p50):.3f}, {max(p50):.3f}] | {median(ps) if ps else 0:.3f} | {ours:g} |")
out+=["","## Raw runs","","Full raw percentiles, allocations, bytes, memory, primitives, environment, frame warnings, and per-frame cold/hot paint traces are linked below.",
"| JSON | draw p50/p95/p99/max ms | paint p99 ms | dirty-present p99 ms | ours / total mean alloc/frame | private MB |",
"|---|---:|---:|---:|---:|---:|"]
for key,r in sorted(reports.items()):
    m=r.get("metrics",{})
    if "draw_ms" not in m:continue
    d=m["draw_ms"];a=m["allocs_per_frame"]
    out.append(f"| [{key}](T0.4/{key}.json) | "+"/".join(f"{d[k]:.3f}" for k in ["p50","p95","p99","max"])+f' | {m.get("paint_ms",{}).get("p99",0):.3f} | {m.get("dirty_to_present_ms",{}).get("p99",0):.3f} | {a["ours"]["mean"]:g} / {a["total"]["mean"]:.2f} | {m["memory_mb"]:.1f} |')
out+=["","## Atlas sweeps","","| Probe/run | First sweep pages / MB | Second sweep pages / MB | Cold first-use max paint ms |",
"|---|---:|---:|---:|"]
for key,r in sorted(reports.items()):
    if (r.get("spike") or {}).get("scene") not in ("s3","s5") or (r.get("spike") or {}).get("mode") not in (1,3,"sweep","direct-sweep"): continue  # codes before T0.8 review
    scene=r["spike"]["scene"]
    log=r["scene_details"]["atlas"]
    period=312 if scene=="s3" else 220
    def near(frame):
        return min(log,key=lambda s:abs(s["frame"]-frame)) if log else {"pages":0,"bytes":0}
    a,b=near(period),near(period*2)
    trace=r["scene_details"]["paint_trace"]
    cold=max((row[2] for row in trace[:period]),default=0)
    out.append(f'| {key} | {a["pages"]} / {a["bytes"]/1048576:.2f} | {b["pages"]} / {b["bytes"]/1048576:.2f} | {cold:.3f} |')
Path("docs/measurements/T0.4.md").write_text("\n".join(out)+"\n",encoding="utf-8")
print(f"{len(reports)} reports, {len(groups)} probe groups")
