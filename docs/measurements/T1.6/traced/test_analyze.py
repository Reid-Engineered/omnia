"""Portable diagnostic tests; run with Python on the development machine."""
import contextlib
import copy
import gzip
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('analyze', ROOT / 'analyze.py')
analyze = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analyze)
condition_spec = importlib.util.spec_from_file_location('conditions', ROOT / 'conditions.py')
conditions = importlib.util.module_from_spec(condition_spec)
condition_spec.loader.exec_module(conditions)


def output(*paths):
    stream = io.StringIO()
    with contextlib.redirect_stdout(stream):
        code = analyze.main([str(p) for p in paths])
    return code, stream.getvalue()


class DiagnosticTests(unittest.TestCase):
    def test_preflight_rejects_power_scale_and_missing_governor(self):
        rec = dict(mains_online={'AC': '1'}, cpu_governor={'cpu0': 'powersave'},
                   edp=dict(scale=1, size=dict(width=1920, height=1080), refresh=60.008),
                   enabled_outputs=1, dpms='DPMS: on', processes='desktop services')
        conditions.check(rec, 1)
        for key, value in [('mains_online', {'AC': '0'}), ('cpu_governor', {}),
                           ('enabled_outputs', 2), ('dpms', 'off')]:
            with self.assertRaises(ValueError):
                conditions.check(dict(rec, **{key: value}), 1)
        with self.assertRaises(ValueError):
            conditions.check(rec, 1.25)

    def test_real_orion_conditions_in_hertz(self):
        rec = json.loads((ROOT.parent / 's125/runs.jsonl').read_text().splitlines()[0])
        # Historical snapshots predate these inventory fields. Preserve actual
        # AC/governor/panel readings and add the one-panel/process inventory shape.
        rec.update(mains_online={key: value for key, value in rec['ac_online'].items()
                                 if '/AC/' in key}, enabled_outputs=1,
                   processes=(ROOT.parent / 's125/processes.txt').read_text())
        self.assertEqual(rec['edp']['refresh'], 60.007999420166016)
        self.assertEqual(set(rec['cpu_governor'].values()), {'powersave'})
        conditions.check(rec, 1.25)
        bad = copy.deepcopy(rec)
        bad['edp']['refresh'] = 59.0
        with self.assertRaises(ValueError):
            conditions.check(bad, 1.25)

    def test_real_client_formats_and_midnight_wrap(self):
        archive = ROOT.parent.parent / 'T1.6-debug/pan-2-trace1-warm60.json.log.gz'
        lines = gzip.decompress(archive.read_bytes()).decode().splitlines()
        excerpt = next(line for line in lines if '{mesa vk display queue}' in line
                       and '-> wl_surface#31.commit()' in line)
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'client.log'
            path.write_text(excerpt + '\n[23:59:59.999000] {queue} wl_callback#43.done(1)\n'
                            '[00:00:00.001000] {queue} -> wl_surface#31.frame(new id wl_callback#43)\n')
            parsed = analyze.wayland_lines(path)
            self.assertEqual(len(parsed), 3)
            self.assertIn('wl_surface#31.commit()', parsed[0][2])
            self.assertAlmostEqual((parsed[2][0] - parsed[1][0]) / 1e6, 2, places=4)
            path.write_text('[4294967295.000] wl_callback@8.done(1)\n[1.000] wl_callback@8.done(2)\n')
            parsed = analyze.wayland_lines(path)
            self.assertEqual((parsed[1][0] - parsed[0][0]) / 1e6, 2)

    def test_archived_trace_correlates_surface_31(self):
        base = ROOT.parent.parent / 'T1.6-debug'
        report = json.loads(gzip.decompress((base / 'pan-2-trace1-warm60.json.gz').read_bytes()))
        rows, measured = analyze.timeline(report)
        self.assertEqual(len(rows), 658)
        target = max(measured, key=lambda row: row[2]['animation_interval_ns'])
        stream = io.StringIO()
        with contextlib.redirect_stdout(stream):
            surface, lower, upper = analyze.correlate(base / 'pan-2-trace1-warm60.json.log.gz',
                                                      rows, target[3]['end_ns'], target[2]['end_ns'])
        self.assertEqual(surface, '31')
        self.assertAlmostEqual((upper - lower) / 1e6, .545, delta=.01)
        self.assertIn('wl_surface#31.frame', stream.getvalue())
        self.assertIn('wl_callback#43.done', stream.getvalue())

    def test_existing_untraced_campaign_does_not_claim_clean(self):
        code, text = output(ROOT.parent / 's100', ROOT.parent / 's125')
        self.assertEqual(code, 0)
        self.assertIn('40 motion runs; 0 usable traces', text)
        self.assertIn('aggregate missed_refreshes=1', text)
        self.assertNotIn('not reproduced in', text)

    def test_captured_miss_orders_dirty_before_signal_and_correlates(self):
        code, text = output(ROOT / 'fixtures' / 'pan-1.json')
        self.assertEqual(code, 0)
        self.assertIn('measured_index=0; interval_ms=50.000000', text)
        self.assertLess(text.index('index 0 dirty'), text.index('index 0 signal'))
        self.assertIn('platform_present_ms=1.000000', text)
        self.assertIn('index-0 warmup/baseline boundary', text)
        self.assertIn('Candidate Wayland surface 7', text)
        self.assertIn('wl_callback@8.done', text)
        self.assertIn('wp_presentation@10.feedback', text)
        self.assertIn('index 1 signal: unavailable', text)

    def test_clean_truncated_and_ambiguous_logs(self):
        report = json.loads((ROOT / 'fixtures/pan-1.json').read_text())
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'pan-1.json'
            clean = copy.deepcopy(report)
            clean['environment']['refresh_hz'] = 20
            clean['metrics']['missed_refreshes'] = 0
            path.write_text(json.dumps(clean))
            self.assertIn('incomplete diagnostic dataset', output(path)[1])
            self.assertNotIn('not reproduced in', output(path)[1])
            path.write_text(json.dumps(report))
            self.assertIn('Wayland log unavailable', output(path)[1])
            path.with_suffix('.wayland.log').write_text('[1.000] wl_callback@8.done(1)\n')
            self.assertIn('correlation unresolved', output(path)[1])
            report['diagnostic_trace']['events'].pop()
            path.write_text(json.dumps(report))
            code, text = output(path)
            self.assertEqual(code, 1)
            self.assertIn('analysis incomplete', text)
            self.assertNotIn('not reproduced in', text)

    def test_complete_campaign_and_aggregate_disagreement(self):
        report = json.loads((ROOT / 'fixtures/pan-1.json').read_text())
        report['environment']['refresh_hz'] = 20
        report['metrics']['missed_refreshes'] = 0
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            for tag in ('s100', 's125'):
                (root / tag).mkdir()
                for case in ('pan', 'zoom'):
                    for rep in range(1, 11):
                        report['config']['script'] = case
                        (root / tag / f'{case}-{rep}.json').write_text(json.dumps(report))
            self.assertIn('incomplete diagnostic dataset', output(root)[1])
            (root / 'status.txt').write_text('incomplete\n')
            self.assertNotIn('not reproduced in', output(root)[1])
            (root / 'status.txt').write_text('complete\n')
            self.assertIn('not reproduced in 40 motion runs', output(root)[1])
            report['config']['script'] = 'pan'
            report['metrics']['missed_refreshes'] = 1
            (root / 's125/pan-1.json').write_text(json.dumps(report))
            text = output(root)[1]
            self.assertIn('aggregate/raw disagreement', text)
            self.assertIn('1 aggregate/raw disagreements', text)
            self.assertNotIn('not reproduced in', text)
            (root / 's125/pan-1.json').unlink()
            self.assertIn('incomplete diagnostic dataset', output(root)[1])


if __name__ == '__main__':
    unittest.main()
