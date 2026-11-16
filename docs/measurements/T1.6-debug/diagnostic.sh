#!/usr/bin/env bash
# Eight serial pan diagnostics at 125%; not gate results. Restore panel scale on exit.
# Run under kde-inhibit --power --screenSaver bash, with BENCH pointing to a built binary.
set -euo pipefail
out=${1:?unique output directory required}
[[ ! -e $out ]] || { echo "output already exists" >&2; exit 2; }
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

# Bounded diagnostic campaign, not section 2.1 gate data. Never overwrite output.
kscreen-doctor output.eDP-1.scale.1.25 >/dev/null 2>&1
sleep 3
mkdir -p "$out"
ps -eo pid,user,comm,args --sort=pid > "$out/processes.txt"
sha256sum "$bench" > "$out/binary.sha256"
for r in {1..8}; do
    trace=1; warmup=60
    [[ $r == 1 || $r == 8 ]] && trace=0
    [[ $r == 6 || $r == 7 ]] && warmup=600
    key=pan-$r-trace$trace-warm$warmup
    json=$out/$key.json
    [[ ! -e $json ]] || exit 2
    snapshot "$out" "$key" "$r" before null
    code=0
    if [[ $trace == 1 ]]; then
        OMNIA_BENCH_TRACE=1 WAYLAND_DEBUG=client "$bench" run --sheet empty --script pan --frames 600 --warmup "$warmup" --size 1536x864 --json "$json" > "$json.log" 2>&1 || code=$?
    else
        "$bench" run --sheet empty --script pan --frames 600 --warmup "$warmup" --size 1536x864 --json "$json" > "$json.log" 2>&1 || code=$?
    fi
    snapshot "$out" "$key" "$r" after "$code"
    [[ $code == 0 ]] || exit "$code"
done