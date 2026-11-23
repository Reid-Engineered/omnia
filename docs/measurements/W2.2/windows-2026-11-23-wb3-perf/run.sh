#!/usr/bin/env bash
# W2.2 WB3 diagnostic on a Windows development machine (not a reference run; certifies nothing).
# Alternates the pre-change and post-change WB3 bench binaries, three passes each,
# 100 per-apply observations per pass (follow_command --bench), CSVs into raw/.
# Usage: run.sh <before.exe> <after.exe>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
before="$1"; after="$2"
: > "$here/raw/order.txt"
for pass in 1 2 3; do
  for which in before after; do
    exe="$before"; [ "$which" = after ] && exe="$after"
    echo "pass $pass $which" >> "$here/raw/order.txt"
    powershell -NoProfile -c "(Get-CimInstance Win32_Processor).LoadPercentage" > "$here/raw/load-$pass-$which-pre.txt"
    OMNIA_WB3_CSV="$here/raw/$pass-$which.csv" "$exe" --bench follow 2>/dev/null | grep -E "observations|time:" > "$here/raw/$pass-$which.txt"
    powershell -NoProfile -c "(Get-CimInstance Win32_Processor).LoadPercentage" > "$here/raw/load-$pass-$which-post.txt"
  done
done
