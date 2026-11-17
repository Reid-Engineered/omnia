"""Read-only T1.6 preflight, snapshots, and report integrity checks (no budgets)."""
import datetime
import glob
import json
import pathlib
import shutil
import subprocess
import sys


def read(path):
    try:
        return pathlib.Path(path).read_text().strip()
    except OSError:
        return None


def run(*args):
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.DEVNULL).strip()
    except (OSError, subprocess.CalledProcessError):
        return None


def snapshot():
    screen = json.loads(run('kscreen-doctor', '--json') or '{}')
    outputs = [o for o in screen.get('outputs', []) if o.get('enabled')]
    panel = next((o for o in outputs if o['name'] == 'eDP-1'), None)
    edp = None
    if panel:
        mode = next((m for m in panel['modes'] if m['id'] == panel['currentModeId']), {})
        edp = dict(scale=panel['scale'], size=mode.get('size'), refresh=mode.get('refreshRate'))
    return dict(
        time=datetime.datetime.now(datetime.timezone.utc).isoformat(),
        ac_online={p: read(p) for p in glob.glob('/sys/class/power_supply/*/online')},
        mains_online={p: read(p) for p in glob.glob('/sys/class/power_supply/*/online')
                      if read(str(pathlib.Path(p).with_name('type'))) == 'Mains'},
        battery_status={p: read(p) for p in glob.glob('/sys/class/power_supply/BAT*/status')},
        cpu_governor={p: read(p) for p in glob.glob('/sys/devices/system/cpu/cpu[0-9]*/cpufreq/scaling_governor')},
        power_profile=run('powerprofilesctl', 'get') or 'unavailable',
        platform_profile=read('/sys/firmware/acpi/platform_profile') or 'unavailable',
        edp=edp, enabled_outputs=len(outputs), screen=screen,
        dpms=run('kscreen-doctor', '--dpms', 'show'),
        loadavg=read('/proc/loadavg'),
        processes=run('ps', '-eo', 'pid,ppid,user,comm,args', '--sort=pid'),
    )


def check(rec, scale):
    if '1' not in rec['mains_online'].values():
        raise ValueError('AC power could not be verified')
    if not rec['cpu_governor'] or not all(rec['cpu_governor'].values()):
        raise ValueError('CPU governors unavailable')
    panel = rec['edp'] or {}
    if (rec['enabled_outputs'] != 1 or panel.get('scale') != scale
            or panel.get('size') != dict(width=1920, height=1080)
            or abs((panel.get('refresh') or 0) - 60.0) > 0.1):
        raise ValueError(f'require only native eDP-1, 1920x1080 ~60 Hz, scale {scale}; set display settings first')
    if 'on' not in (rec['dpms'] or '').lower() or not rec['processes']:
        raise ValueError('panel awake/process inventory could not be verified')


def report(path, scale):
    data = json.loads(pathlib.Path(path).read_text())
    if data['kind'] == 'start':
        if len(data['samples_ms']) != 10:
            raise ValueError('incomplete start samples')
        return
    env = data['environment']
    gpu = env.get('gpu') or {}
    if (env.get('scale_factor') != scale or env.get('fullscreen') is not True
            or env.get('viewport_logical') != [1920 / scale, 1080 / scale]
            or abs((env.get('refresh_hz') or 0) - 60) > .1
            or gpu.get('is_software_emulated') is not False
            or 'HD Graphics 620' not in gpu.get('device_name', '')
            or 'Mesa' not in gpu.get('driver_name', '') or data.get('warnings')):
        raise ValueError('invalid environment/warnings; retain incomplete dataset')
    if data['kind'] == 'run':
        if (data['frames']['drawn'] != 600 or not data.get('diagnostic_trace')
                or data['config'].get('diagnostic_trace') is not True):
            raise ValueError('missing motion frames or diagnostic_trace')


if __name__ == '__main__':
    try:
        mode = sys.argv[1]
        if mode == 'report':
            report(sys.argv[2], float(sys.argv[3]))
        else:
            rec = snapshot()
            if mode == 'check':
                if shutil.disk_usage(pathlib.Path(__file__).parent).free < 1_000_000_000:
                    raise ValueError('at least 1 GB free space required for diagnostic evidence')
                check(rec, float(sys.argv[2]))
            elif mode == 'record':
                directory, key, rep, phase, code = sys.argv[2:]
                rec.update(key=key, repetition=int(rep), phase=phase, exit=json.loads(code))
                with (pathlib.Path(directory) / 'runs.jsonl').open('a') as stream:
                    stream.write(json.dumps(rec) + '\n')
                print(json.dumps(rec, indent=2))
            else:
                print(json.dumps(rec, indent=2))
    except (ValueError, KeyError, OSError) as error:
        sys.exit(f'Refused: {error}')
