#!/usr/bin/env bash
# Linux reference protocol (canvas-plan.md 2.1). No build or system-setting changes.
# Usage: measure-ref.sh [s1|s3|s4|s5|s6|s8|rest|all|campaign] [results] [size] [repeats]
set -euo pipefail
scene=${1:-s1}
results=${2:-docs/measurements/T0.8}
size=${3:-1920x1080}
repeats=${4:-10}
bench=${BENCH:-./target/release/omnia-bench}
export OMNIA_BENCH_FULLSCREEN=1
if [[ $scene == campaign ]]; then
    # The EXIT trap covers failed runs and signals as well as the successful path.
    trap 'kscreen-doctor output.eDP-1.scale.1' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    "$0" s1 "$results" 1920x1080 "$repeats"
    "$0" rest "$results/rest" 1920x1080 "$repeats"
    "$0" s8 "$results/s8" 1920x1080 "$repeats"
    kscreen-doctor output.eDP-1.scale.1.25
    "$0" s1 "$results/s125" 1536x864 "$repeats"
    exit
fi
case $scene in s1|s3|s4|s5|s6|s8|rest|all) ;; *) echo "Unknown scene: $scene" >&2; exit 2;; esac
[[ $repeats =~ ^[1-9][0-9]*$ ]] || { echo "repeats must be positive" >&2; exit 2; }
mkdir -p "$results"
cases=('baseline 0 fixed')
add() { if [[ $scene == all || $scene == "$1" || ($scene == rest && $1 != s1 && $1 != s8) ]]; then cases+=("$1 $2 $3"); fi; }
for n in 5000 11300 20000 60000 100000; do add s1 "$n" fixed; done
for n in 25000 70000; do add s1 "$n" quads-only; done
for n in 1000 5000 15000; do add s3 "$n" fixed; done
for n in 100 500 1500 5000; do add s4 "$n" fixed; done
for n in 2000 6000 12000; do for m in fixed direct; do add s5 "$n" "$m"; done; done
for n in 20000 60000; do for m in cached repaint; do add s6 "$n" "$m"; done; done
[[ $scene != s8 ]] || cases+=("s8 0 fixed")
# Exact T-present load: 18,999 wires + the paper quad = 19,000 primitives.
[[ $scene != rest ]] || cases+=("s1 18999 quads-only")
# Write separate before/after records, including failures, so interrupted runs retain power evidence.
power() {
    python3 - "$results/runs.jsonl" "$key" "$r" "$1" "${2:-null}" <<'PY'
import datetime, glob, json, pathlib, subprocess, sys
log, key, repetition, phase, code = sys.argv[1:]
def read(p):
    try: return pathlib.Path(p).read_text().strip()
    except OSError: return None
try: profile = subprocess.check_output(['powerprofilesctl', 'get'], text=True).strip()
except (OSError, subprocess.CalledProcessError): profile = None
try: dpms = subprocess.check_output(['kscreen-doctor', '--dpms', 'show'], text=True).strip()
except (OSError, subprocess.CalledProcessError): dpms = None
record = dict(key=key, repetition=int(repetition), phase=phase, exit=json.loads(code),
    time=datetime.datetime.now(datetime.timezone.utc).isoformat(),
    ac_online={p: read(p) for p in glob.glob('/sys/class/power_supply/*/online')},
    cpu_governor={p: read(p) for p in glob.glob('/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor')},
    power_profile=profile, dpms=dpms)
with open(log, 'a') as f: f.write(json.dumps(record)+'\n')
PY
}
for ((r=1; r<=repeats; r++)); do
    for ((j=0; j<${#cases[@]}; j++)); do
        index=$(((j+r-1)%${#cases[@]}))
        read -r s n m <<< "${cases[$index]}"
        if [[ $s == baseline ]]; then key=baseline-$r; else key=$s-$n-$m-$r; fi
        json=$results/$key.json
        [[ ! -e $json ]] || { echo "Refusing to overwrite $json" >&2; exit 1; }
        echo "[$r/$repeats] $key"
        power before
        code=0
        if [[ $s == baseline ]]; then
            "$bench" run --sheet empty --frames 600 --size "$size" --json "$json" > "$json.log" 2>&1 || code=$?
        else
            "$bench" spike "$s" --count "$n" --mode "$m" --frames 600 --size "$size" --json "$json" > "$json.log" 2>&1 || code=$?
        fi
        power after "$code"
        ((code == 0)) || exit "$code"
        python3 - "$json" "$size" <<'PY_CHECK'
import json, sys
report = json.load(open(sys.argv[1]))
env = report.get('environment') or report['idle']['environment']
assert '620' in str(env['gpu']) and 'llvmpipe' not in str(env['gpu']).lower(), env
expected = [int(x) for x in sys.argv[2].split('x')]
assert env['viewport_logical'] == expected, env
assert env['fullscreen'] is True, env
assert env['viewport_device'] == [1920, 1080], env
assert env['scale_factor'] == (1.0 if expected == [1920, 1080] else 1.25), env
PY_CHECK
    done
    if [[ $scene == rest && $r == 1 ]]; then
        # User-authorized fallback: keep one rest pass if the projected campaign exceeds 3 h.
        if python3 - "$results" "$repeats" "$SECONDS" <<'PY_BUDGET'
import datetime, json, pathlib, sys
folder, repeats, elapsed = pathlib.Path(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3])
rows = [json.loads(line) for line in (folder.parent / 'runs.jsonl').read_text().splitlines()]
s1_seconds = (datetime.datetime.fromisoformat(rows[-1]['time']) -
              datetime.datetime.fromisoformat(rows[0]['time'])).total_seconds()
projected = 2*s1_seconds + repeats*elapsed + 300  # both S1 scales + S8 allowance
if projected <= 10800:
    raise SystemExit(1)
(folder / 'protocol-reduction.txt').write_text(
    f'Projected campaign {projected/3600:.2f} h from S1={s1_seconds:.0f} s and '
    f'first rest pass={elapsed} s. Retained one rest pass; both S1 scales retain ten.\n')
PY_BUDGET
        then
            echo "Projected campaign exceeds three hours; keeping one rest pass (user fallback)."
            break
        fi
    fi
done
