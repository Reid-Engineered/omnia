#!/usr/bin/env bash
# Diagnostic only (Windows development machine; certifies nothing): bench frame trace on/off,
# interleaved ABBA over five pairs, 125% gate arguments. Results summarised in results.csv.
# Usage (repo root, release build done): bash docs/measurements/T1.6/trace-cost-windows/run.sh OUT
set -euo pipefail
out=$1
mkdir "$out"
for i in 1 2 3 4 5; do
    if (( i % 2 )); then modes="u t"; else modes="t u"; fi
    for m in $modes; do
        args=(run --sheet empty --script pan --frames 600 --warmup 60 --size 1536x864 --json "$out/$m-$i.json")
        if [[ $m == t ]]; then OMNIA_BENCH_TRACE=1 ./target/release/omnia-bench.exe "${args[@]}"
        else ./target/release/omnia-bench.exe "${args[@]}"; fi > /dev/null 2>&1
        echo "$m-$i $?"
    done
done
