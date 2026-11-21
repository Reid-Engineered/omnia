"""Fixed T2.7 diagnostic. Run from the patched d014883 checkout on Orion."""
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path("docs/measurements/T2.7").resolve()))
import gate

out = Path(sys.argv[1]).resolve()
bench = Path("target/release/omnia-bench").resolve()
order = [(tag, rep, case) for tag in gate.SCALES for rep in range(1, 11)
         for case in (("10k-zoom", "1k-pan") if rep % 2 else ("1k-pan", "10k-zoom"))]
original = gate.active_panel()["scale"]
out.mkdir()
gate.dump(out / "order.json", order)
gate.dump(out / "before.json", gate.fingerprint(bench))
(out / "status.txt").write_text("incomplete\n")
env = dict(os.environ, OMNIA_BENCH_TRACE="1", OMNIA_BENCH_FULLSCREEN="1")
env.pop("WAYLAND_DEBUG", None)
try:
    for tag, rep, case in order:
        scale, viewport = gate.SCALES[tag]
        if rep == 1 and case == "10k-zoom":
            gate.set_scale(scale)
        key = f"{tag}-{case}-{rep}"
        print(key, flush=True)
        sheet, script = case.split("-")
        pre = gate.capture()
        gate.dump(out / (key + ".before.json"), pre)
        gate.validate_conditions(pre, scale)
        with (out / (key + ".log")).open("xb") as log:
            result = subprocess.run([bench, "run", "--sheet", sheet, "--script", script,
                "--size", "x".join(map(str, viewport)), "--frames", "600", "--warmup", "600",
                "--json", out / (key + ".json")], env=env, stdout=log, stderr=subprocess.STDOUT)
        post = gate.capture()
        gate.dump(out / (key + ".after.json"), post)
        result.check_returncode()
        gate.validate_conditions(post, scale)
        report = gate.load(out / (key + ".json"))
        *_, attribution = gate.motion(report, sheet, script, scale, viewport)
        if not attribution["usable"]:
            raise ValueError(attribution["error"])
finally:
    gate.set_scale(original)
    gate.dump(out / "after.json", gate.fingerprint(bench))
if gate.load(out / "before.json") != gate.load(out / "after.json"):
    raise ValueError("binary or checkout changed")
(out / "status.txt").write_text("complete\n")
