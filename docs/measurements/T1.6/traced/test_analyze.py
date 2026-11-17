"""Portable diagnostic tests; run with Python on the development machine."""
import contextlib
import copy
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
                   edp=dict(scale=1, size=dict(width=1920, height=1080), refresh=60008),
                   enabled_outputs=1, dpms='DPMS: on', processes='desktop services')
        conditions.check(rec, 1)
        for key, value in [('mains_online', {'AC': '0'}), ('cpu_governor', {}),
                           ('enabled_outputs', 2), ('dpms', 'off')]:
            with self.assertRaises(ValueError):
                conditions.check(dict(rec, **{key: value}), 1)
        with self.assertRaises(ValueError):
            conditions.check(rec, 1.25)

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
            path.write_text(json.dumps(clean))
            self.assertIn('not reproduced in 1 motion runs', output(path)[1])
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


if __name__ == '__main__':
    unittest.main()
