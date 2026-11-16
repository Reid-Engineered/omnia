#!/usr/bin/env bash
# T1.6 milestone-1 gate on the reference machine (canvas-plan.md 2.1, roadmap T1.6).
# Cases B1 start, B3/B10 pan and zoom on the empty sheet, B8 idle; ten interleaved
# repetitions per case (order rotates each pass), at 100% and then 125% panel scaling.
# Only system setting touched: the eDP-1 scale, restored on exit (success, failure, signal).
# Usage (repo root, release build already done): gate.sh [results] [repeats]
set -euo pipefail
out=${1:-docs/measurements/T1.6}
repeats=${2:-10}
bench=${BENCH:-./target/release/omnia-bench}
export OMNIA_BENCH_FULLSCREEN=1
[[ -x $bench ]] || { echo "build first: cargo build --release -p omnia-bench" >&2; exit 2; }
original_scale=$(kscreen-doctor --json | python3 -c '
import json, sys
print(next(o["scale"] for o in json.load(sys.stdin)["outputs"] if o["name"] == "eDP-1"))')
[[ $original_scale =~ ^[0-9]+(\.[0-9]+)?$ ]] || { echo "bad scale $original_scale" >&2; exit 1; }
trap 'kscreen-doctor "output.eDP-1.scale.$original_scale" >/dev/null 2>&1' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

snapshot() { # results-dir key repetition phase exit
    python3 - "$@" <<'PY'
import datetime, glob, json, pathlib, subprocess, sys
d, key, rep, phase, code = sys.argv[1:]
def read(p):
    try: return pathlib.Path(p).read_text().strip()
    except OSError: return None
def run(*a):
    try: return subprocess.check_output(a, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError): return None
screen = run('kscreen-doctor', '--json')
edp = None
if screen:
    o = next(o for o in json.loads(screen)['outputs'] if o['name'] == 'eDP-1')
    mode = next((m for m in o['modes'] if m['id'] == o['currentModeId']), {})
    edp = dict(scale=o['scale'], size=mode.get('size'), refresh=mode.get('refreshRate'))
rec = dict(key=key, repetition=int(rep), phase=phase, exit=json.loads(code),
    time=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    ac_online={p: read(p) for p in glob.glob('/sys/class/power_supply/*/online')},
    battery_status={p: read(p) for p in glob.glob('/sys/class/power_supply/BAT*/status')},
    cpu_governor={p: read(p) for p in glob.glob('/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor')},
    power_profile=run('powerprofilesctl', 'get'),
    platform_profile=read('/sys/firmware/acpi/platform_profile'),
    dpms=run('kscreen-doctor', '--dpms', 'show'), edp=edp,
    loadavg=read('/proc/loadavg'))
with open(pathlib.Path(d) / 'runs.jsonl', 'a') as f: f.write(json.dumps(rec) + '\n')
PY
}

cases=(start pan zoom idle)
for scale in 1 1.25; do
    if [[ $scale == 1 ]]; then dir=$out/s100; size=1920x1080; else dir=$out/s125; size=1536x864; fi
    mkdir -p "$dir"
    kscreen-doctor "output.eDP-1.scale.$scale" >/dev/null 2>&1
    sleep 3
    ps -eo pid,user,comm,args --sort=pid > "$dir/processes.txt"
    for ((r=1; r<=repeats; r++)); do
        for ((j=0; j<${#cases[@]}; j++)); do
            c=${cases[$(((j+r-1)%${#cases[@]}))]}
            key=$c-$r
            json=$dir/$key.json
            [[ ! -e $json ]] || { echo "refusing to overwrite $json" >&2; exit 1; }
            echo "[$scale $r/$repeats] $c"
            snapshot "$dir" "$key" "$r" before null
            code=0
            case $c in
                start) "$bench" start --sheet empty --runs 10 --size "$size" --json "$json" ;;
                pan|zoom) "$bench" run --sheet empty --script "$c" --frames 600 --warmup 60 \
                    --size "$size" --json "$json" ;;
                idle) "$bench" idle --sheet empty --seconds 10 --size "$size" --json "$json" ;;
            esac > "$json.log" 2>&1 || code=$?
            snapshot "$dir" "$key" "$r" after "$code"
            [[ $code == 0 ]] || { echo "$key exited $code" >&2; exit "$code"; }
        done
    done
done
