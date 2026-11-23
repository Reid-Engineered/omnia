"""Summarize a frozen 10-pass W2.2 / WB3 campaign archive (raw.tar.gz).

Reads the archive path argument, defaulting to ``raw.tar.gz`` beside this file.
Verifies the frozen case list (``follow_move_100_parts_10k_apply``), exact pass
coverage (``01`` through ``10``) and 100 per-apply observations per invocation.
Computes nearest-rank p50, p99, max and mean per pass, then reports the median of the
pass p99s and their range against WB3 (a ``Move`` of 100 parts with follow on ``10k``
under 2 ms), plus the Criterion mean estimates parsed from ``*.stdout.txt``.

This script never certifies a budget on its own: WB3 needs a complete campaign on
Orion under canvas-plan section 2.1 and a review of the recorded conditions.
"""
import csv
import io
import re
import statistics
import sys
import tarfile
from pathlib import Path

root = Path(__file__).parent
WB3_LIMIT_MS = 2.0
CASE = "follow_move_100_parts_10k_apply"
EXPECTED_PASSES = {f"{i:02d}" for i in range(1, 11)}
EXPECTED_ROWS = 100

CRITERION_RE = re.compile(
    r"^follow_move_100_parts_10k_apply\s+time:\s+\[([0-9.]+)\s+(\S+)\s+([0-9.]+)\s+(\S+)\s+([0-9.]+)\s+(\S+)\]"
)


def to_ms(value: float, unit: str) -> float:
    if unit == "ns":
        return value / 1e6
    if unit in ("µs", "us"):
        return value / 1e3
    if unit == "ms":
        return value
    if unit == "s":
        return value * 1e3
    raise ValueError(f"unsupported Criterion time unit: {unit}")


def nearest_rank(sorted_vals: list[int], pct: int) -> int:
    idx = (len(sorted_vals) * pct + 99) // 100 - 1
    return sorted_vals[idx]


def main() -> None:
    archive_path = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "raw.tar.gz"
    per_pass: dict[str, tuple[float, float, float, float]] = {}
    criterion_est_ms: list[float] = []

    with tarfile.open(archive_path) as archive:
        for member in sorted(archive.getmembers(), key=lambda m: m.name):
            if member.name.endswith(".stdout.txt"):
                text = archive.extractfile(member).read().decode("utf-8", errors="replace")
                for line in text.splitlines():
                    m = CRITERION_RE.match(line.strip())
                    if m:
                        criterion_est_ms.append(to_ms(float(m.group(3)), m.group(4)))
                continue
            if not member.name.endswith(".csv"):
                continue
            rows = list(csv.DictReader(io.TextIOWrapper(archive.extractfile(member), encoding="utf-8")))
            assert len(rows) == EXPECTED_ROWS, f"{member.name}: expected {EXPECTED_ROWS} rows, got {len(rows)}"
            stem = Path(member.name).stem
            pass_id, case = stem.split("-", 1)
            assert pass_id in EXPECTED_PASSES, f"invalid pass: {stem}"
            assert case == CASE, f"unexpected case: {stem}"
            assert pass_id not in per_pass, f"duplicate pass: {stem}"
            ns = sorted(int(r["ns"]) for r in rows)
            per_pass[pass_id] = (
                nearest_rank(ns, 50) / 1e6,
                nearest_rank(ns, 99) / 1e6,
                ns[-1] / 1e6,
                statistics.fmean(ns) / 1e6,
            )

    assert set(per_pass) == EXPECTED_PASSES, f"missing passes: {sorted(EXPECTED_PASSES - set(per_pass))}"
    print(f"case {CASE}: 10 passes x {EXPECTED_ROWS} per-apply observations")
    print("pass  p50_ms     p99_ms     max_ms     mean_ms")
    for pass_id in sorted(per_pass):
        p50, p99, mx, mean = per_pass[pass_id]
        print(f"{pass_id}    {p50:9.6f}  {p99:9.6f}  {mx:9.6f}  {mean:9.6f}")
    p99s = [v[1] for v in per_pass.values()]
    med = statistics.median(p99s)
    print(f"median of pass p99s: {med:.6f} ms (range {min(p99s):.6f} - {max(p99s):.6f} ms)")
    if criterion_est_ms:
        print(
            f"criterion mean estimates: median {statistics.median(criterion_est_ms):.6f} ms "
            f"(range {min(criterion_est_ms):.6f} - {max(criterion_est_ms):.6f} ms, n={len(criterion_est_ms)})"
        )
    verdict = "pass" if med < WB3_LIMIT_MS else "fail"
    print(f"WB3 (< {WB3_LIMIT_MS} ms, median of pass p99s): {verdict}")


if __name__ == "__main__":
    main()
