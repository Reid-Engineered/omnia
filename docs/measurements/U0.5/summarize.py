"""Summarize a frozen 10-pass U0.5 doc_io campaign archive (raw.tar.gz).

Reads the archive path argument, defaulting to ``raw.tar.gz`` beside this file.
Verifies the frozen case list (``save_1k``, ``load_1k``, ``save_10k``, ``load_10k``),
exact pass coverage (``01`` through ``10``), 100 observations per invocation, and
consistent byte sizes per sheet. Computes nearest-rank p50, p99, max, and mean per
pass, then reports the median and range across the 10 passes, plus Criterion mean
estimates parsed from ``*.stdout.txt`` when present.

This script never certifies a budget on its own: UB10 compliance requires a complete
campaign on Orion under canvas-plan section 2.1 and separate human condition review.
"""
import csv
import io
import re
import statistics
import sys
import tarfile
from pathlib import Path

root = Path(__file__).parent
UB10_SAVE_10K_LIMIT_MS = 33.3
EXPECTED_CASES = ("save_1k", "load_1k", "save_10k", "load_10k")
EXPECTED_PASSES = {f"{i:02d}" for i in range(1, 11)}
EXPECTED_ROWS = 100

CRITERION_RE = re.compile(
    r"^(save_1k|load_1k|save_10k|load_10k)\s+time:\s+\[([0-9.]+)\s+(\S+)\s+([0-9.]+)\s+(\S+)\s+([0-9.]+)\s+(\S+)\]"
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
    groups: dict[str, list[tuple[float, float, float, float, int]]] = {}
    passes: dict[str, set[str]] = {}
    sheet_sizes: dict[str, set[int]] = {}
    criterion_est_ms: dict[str, list[float]] = {}

    with tarfile.open(archive_path) as archive:
        for member in sorted(archive.getmembers(), key=lambda m: m.name):
            if member.name.endswith(".stdout.txt"):
                raw_bytes = archive.extractfile(member).read()
                encoding = "utf-16" if raw_bytes.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8"
                text = raw_bytes.decode(encoding, errors="replace")
                for line in text.splitlines():
                    m = CRITERION_RE.match(line.strip())
                    if m:
                        case = m.group(1)
                        est = to_ms(float(m.group(4)), m.group(5))
                        criterion_est_ms.setdefault(case, []).append(est)
                continue

            if not member.name.endswith(".csv"):
                continue

            rows = list(csv.DictReader(io.TextIOWrapper(archive.extractfile(member), encoding="utf-8")))
            assert len(rows) == EXPECTED_ROWS, (
                f"{member.name}: expected {EXPECTED_ROWS} rows, got {len(rows)}"
            )
            stem = Path(member.name).stem
            pass_id, case = stem.split("-", 1)
            assert pass_id in EXPECTED_PASSES, f"invalid pass: {stem}"
            assert case in EXPECTED_CASES, f"unexpected case: {stem}"
            seen = passes.setdefault(case, set())
            assert pass_id not in seen, f"duplicate pass: {stem}"
            seen.add(pass_id)

            row_bytes = {int(row["bytes"]) for row in rows}
            assert len(row_bytes) == 1, f"{member.name}: inconsistent bytes values {row_bytes}"
            size_bytes = next(iter(row_bytes))
            sheet = "1k" if case.endswith("_1k") else "10k"
            sheet_sizes.setdefault(sheet, set()).add(size_bytes)

            values = sorted(int(row["ns"]) for row in rows)
            p50_ms = nearest_rank(values, 50) / 1e6
            p99_ms = nearest_rank(values, 99) / 1e6
            max_ms = values[-1] / 1e6
            mean_ms = statistics.mean(values) / 1e6
            groups.setdefault(case, []).append((p50_ms, p99_ms, max_ms, mean_ms, size_bytes))
            print(
                f"{stem}: bytes={size_bytes} p50_ms={p50_ms:.6f} p99_ms={p99_ms:.6f} "
                f"max_ms={max_ms:.6f} mean_ms={mean_ms:.6f}"
            )

    assert set(groups) == set(EXPECTED_CASES), (
        f"frozen case set mismatch: missing={sorted(set(EXPECTED_CASES) - set(groups))} "
        f"extra={sorted(set(groups) - set(EXPECTED_CASES))}"
    )
    assert all(passes.get(case) == EXPECTED_PASSES for case in EXPECTED_CASES), (
        "passes must be exactly 01-10 for every case"
    )
    for sheet, sizes in sorted(sheet_sizes.items()):
        assert len(sizes) == 1, f"sheet {sheet} produced multiple sizes: {sizes}"

    print("\nFile sizes:")
    for sheet in ("1k", "10k"):
        size = next(iter(sheet_sizes[sheet]))
        print(f"  {sheet}: {size} bytes ({size / (1024 * 1024):.4f} MiB)")

    print("\nMedian of 10 passes (nearest-rank per pass, n=100 observations/pass):")
    for case in EXPECTED_CASES:
        runs = groups[case]
        assert len(runs) == 10
        p50s = [r[0] for r in runs]
        p99s = [r[1] for r in runs]
        med_p50 = statistics.median(p50s)
        med_p99 = statistics.median(p99s)
        crit = criterion_est_ms.get(case, [])
        crit_str = (
            f" criterion_median_est_ms={statistics.median(crit):.6f} ({min(crit):.6f}..{max(crit):.6f})"
            if len(crit) == 10
            else ""
        )
        line = (
            f"{case}: bytes={runs[0][4]} median_p50_ms={med_p50:.6f} "
            f"p50_range_ms={min(p50s):.6f}..{max(p50s):.6f} "
            f"median_p99_ms={med_p99:.6f} p99_range_ms={min(p99s):.6f}..{max(p99s):.6f} "
            f"mean_ms={statistics.mean(r[3] for r in runs):.6f} max_ms={max(r[2] for r in runs):.6f}"
            f"{crit_str}"
        )
        if case == "save_10k":
            cmp = (
                f"median_p99 < {UB10_SAVE_10K_LIMIT_MS:.1f} ms"
                if med_p99 < UB10_SAVE_10K_LIMIT_MS
                else f"median_p99 >= {UB10_SAVE_10K_LIMIT_MS:.1f} ms"
            )
            line += f" ub10_comparison={cmp} (not a gate verdict until Orion section 2.1 conditions verified)"
        print(line)


if __name__ == "__main__":
    main()
