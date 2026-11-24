#!/usr/bin/env bash
# W2.2 WB3 round-2 diagnostic on a Windows development machine (not a reference run; certifies nothing).
# Alternates the pre-change (b521dd5) and post-change WB3 bench binaries, three passes each,
# 100 per-apply observations per pass (follow_command --bench), CSVs into raw/.
# Usage: run.sh <before.exe> <after.exe>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
before="$1"; after="$2"
mkdir -p "$here/raw"
: > "$here/raw/order.txt"
if command -v powershell >/dev/null 2>&1; then
  PS="powershell"
elif command -v powershell.exe >/dev/null 2>&1; then
  PS="powershell.exe"
else
  PS="/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"
fi
for pass in 1 2 3; do
  for which in before after; do
    exe="$before"; [ "$which" = after ] && exe="$after"
    echo "pass $pass $which" >> "$here/raw/order.txt"
    "$PS" -NoProfile -c "(Get-CimInstance Win32_Processor).LoadPercentage" > "$here/raw/load-$pass-$which-pre.txt"
    csv_path="$here/raw/$pass-$which.csv"
    if command -v wslpath >/dev/null 2>&1; then
      csv_path="$(wslpath -w "$csv_path")"
      export WSLENV="OMNIA_WB3_CSV${WSLENV:+:$WSLENV}"
    fi
    OMNIA_WB3_CSV="$csv_path" "$exe" --bench follow 2>/dev/null | grep -E "observations|time:" > "$here/raw/$pass-$which.txt"
    "$PS" -NoProfile -c "(Get-CimInstance Win32_Processor).LoadPercentage" > "$here/raw/load-$pass-$which-post.txt"
  done
done
