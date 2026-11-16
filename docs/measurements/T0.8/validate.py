"""Validate the recorded Orion T0.8 protocol and sample completeness."""
import json
from pathlib import Path

root = Path(__file__).parent
summary = []
for name, count, scale in [('', 80, 1.0), ('rest', 190, 1.0), ('s8', 20, 1.0), ('s125', 80, 1.25)]:
    folder = root / name
    rows = [json.loads(line) for line in (folder / 'runs.jsonl').read_text().splitlines()]
    assert len(rows) == 2 * count, (name, len(rows))
    for index in range(0, len(rows), 2):
        before, after = rows[index:index+2]
        assert before['phase'] == 'before' and after['phase'] == 'after'
        assert before['key'] == after['key'] and after['exit'] == 0
        for snapshot in [before, after]:
            assert snapshot['ac_online']['/sys/class/power_supply/AC/online'] == '1'
            assert set(snapshot['cpu_governor'].values()) == {'powersave'}
            assert snapshot['dpms'].endswith(': on')
        report = json.loads((folder / (before['key'] + '.json')).read_text())
        env = report.get('environment') or report['idle']['environment']
        assert env['gpu']['device_name'] == 'Intel(R) HD Graphics 620 (KBL GT2)'
        assert not env['gpu']['is_software_emulated'] and env['fullscreen'] is True
        assert env['scale_factor'] == scale and env['viewport_device'] == [1920, 1080]
        assert env['viewport_logical'] == ([1920, 1080] if scale == 1 else [1536, 864])
        assert env['refresh_hz'] == 60.008
        if report['kind'] in ['run', 'spike']:
            assert report['config']['frames'] == 600 and report['config']['warmup'] == 60
    before_rows = [row for row in rows if row['phase'] == 'before']
    base = [row['key'].rsplit('-', 1)[0] for row in before_rows if row['repetition'] == 1]
    for repetition in range(1, 11):
        order = [row['key'].rsplit('-', 1)[0] for row in before_rows if row['repetition'] == repetition]
        rotation = (repetition - 1) % len(base)
        assert order == base[rotation:] + base[:rotation], (name, repetition)
    summary.append(f'{name or "100% S1"}: {count} successful runs; ten rotating repetitions; '
                   'all AC, powersave, DPMS-on, Intel hardware, fullscreen, correct viewport/scale/60.008 Hz.')
summary.append('Total: 370 successful runs, 740 before/after power records (370 pairs).')
(root / 'validation.txt').write_text('\n'.join(summary) + '\n')
print('\n'.join(summary))
