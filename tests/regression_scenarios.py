"""Regression contracts. Reuses fixture builders, not the original test cases.
Run: python3 tests/regression_scenarios.py target/debug/ExeViewer
"""
import json
from pathlib import Path
import random
import shutil
import struct
import unittest
import diagnose_scenarios as fixtures


class RegressionTests(unittest.TestCase):
    setUp = fixtures.DiagnosisTests.setUp
    cc = fixtures.DiagnosisTests.cc
    fixture = fixtures.DiagnosisTests.fixture
    call = fixtures.DiagnosisTests.call
    diagnose = fixtures.DiagnosisTests.diagnose

    def capture(self, app, expected=0):
        path = self.base/'environment.json'
        code, report = self.call('env', 'capture', app, '--rootfs', self.root,
                                 '--target-path', '/app', '--default-path', '/lib',
                                 '--output', path, '--format', 'json')
        self.assertEqual(code, expected, report)
        return path

    def replay(self, app, path):
        return self.call('diagnose', app, '--env', path, '--rootfs', self.root, '--format', 'json')

    def repair(self, app, replacements):
        plan = self.base/'repair.json'
        plan.write_text(json.dumps({'schema_version':1, 'kind':'diagnosis_overrides',
                                    'replacements':replacements}))
        return self.diagnose(app, '--overrides', plan, '--verification-dir', self.base/'verification')

    def test_snapshot_rejects_unknown_schema_and_wrong_kind(self):
        app, foo = self.fixture(); shutil.copy(foo, self.root/'lib')
        path = self.capture(app)
        original = json.loads(path.read_text())
        for field, value in [('schema_version', 999), ('kind', 'diagnosis')]:
            with self.subTest(field=field):
                invalid = dict(original); invalid[field] = value
                path.write_text(json.dumps(invalid))
                code, error = self.replay(app, path)
                self.assertEqual(code, 2)
                self.assertEqual(error['kind'], 'error')
                self.assertIn('environment_snapshot', error['error'])

    def test_snapshot_rejects_different_executable(self):
        app, foo = self.fixture(); shutil.copy(foo, self.root/'lib')
        path = self.capture(app)
        code, error = self.replay('/bin/true', path)
        self.assertEqual(code, 2)
        self.assertIn('artifact SHA-256 mismatch', error['error'])

    def test_snapshot_records_missing_interpreter(self):
        app, foo = self.fixture(); shutil.copy(foo, self.root/'lib')
        interpreter = self.root/'lib64'/fixtures.LOADER.name
        interpreter.unlink()
        path = self.capture(app, expected=1)
        manifest = json.loads(path.read_text())
        evidence = next(f for f in manifest['files'] if f['path']==str(fixtures.LOADER))
        self.assertIsNone(evidence['sha256'])
        shutil.copyfile(fixtures.LOADER, interpreter)
        code, error = self.replay(app, path)
        self.assertEqual(code, 2)
        self.assertIn('snapshot evidence changed', error['error'])

    def test_snapshot_detects_symlink_retarget_with_same_bytes(self):
        app, foo = self.fixture()
        for name in ['first', 'second']:
            (self.root/name).mkdir(); shutil.copy(foo, self.root/name)
        link = self.root/'lib/libfoo.so'; link.symlink_to('/first/libfoo.so')
        path = self.capture(app)
        link.unlink(); link.symlink_to('/second/libfoo.so')
        code, error = self.replay(app, path)
        self.assertEqual(code, 2)
        self.assertIn('/lib/libfoo.so', error['error'])

    def test_repair_rejects_duplicate_normalized_targets(self):
        app, foo = self.fixture(); shutil.copy(foo, self.root/'lib')
        original = (self.root/'lib/libfoo.so').read_bytes()
        code, error = self.repair(app, [
            {'target':'/lib/libfoo.so','source':str(foo)},
            {'target':'/lib//libfoo.so','source':str(foo)},
        ])
        self.assertEqual(code, 2)
        self.assertIn('duplicate replacement target', error['error'])
        self.assertFalse((self.base/'verification').exists())
        self.assertEqual((self.root/'lib/libfoo.so').read_bytes(), original)

    def test_repair_preserves_original_symlink_and_referent(self):
        app, foo = self.fixture()
        bad = self.cc('wrong.so', 'int other(void){return 0;}', '-shared')
        (self.root/'actual').mkdir(); shutil.copy(bad, self.root/'actual/libfoo.so')
        link = self.root/'lib/libfoo.so'; link.symlink_to('/actual/libfoo.so')
        code, result = self.repair(app, [{'target':'/lib/libfoo.so','source':str(foo)}])
        self.assertEqual(code, 0, result)
        self.assertEqual(str(link.readlink()), '/actual/libfoo.so')
        self.assertEqual((self.root/'actual/libfoo.so').read_bytes(), bad.read_bytes())
        self.assertEqual(result['runtime_validation'], 'not_run')

    def test_repair_new_transitive_requirement_remains_incomplete(self):
        app, foo = self.fixture(); shutil.copy(foo, self.root/'lib')
        self.cc('libbar.so', 'int bar(void){return 1;}', '-shared', '-Wl,-soname,libbar.so')
        replacement = self.cc('replacement.so', 'extern int bar(void);int foo(void){return bar();}',
                              '-shared', '-L'+str(self.build), '-lbar')
        code, result = self.repair(app, [{'target':'/lib/libfoo.so','source':str(replacement)}])
        self.assertEqual(code, 2, result)
        self.assertTrue(any(d['rule_id']=='LIBRARY_NOT_FOUND' and 'libbar.so' in d['message']
                            for d in result['introduced_findings']))
        self.assertEqual(result['runtime_validation'], 'not_run')
        self.assertEqual((self.root/'lib/libfoo.so').read_bytes(), foo.read_bytes())

    def test_trace_reports_target_failure_without_fabricating_loader_error(self):
        code, result = self.call('trace', '--execute', '/bin/false', '--output-dir',
                                 self.base/'trace', '--format', 'json')
        self.assertEqual(code, 1)
        self.assertEqual(result['status'], 'runtime_failure')
        self.assertEqual(result['capture']['target_exit_code'], 1)
        self.assertFalse(any(e['kind']=='loader_error' for e in result['events']))

    def test_trace_preserves_namespace_pid_line_and_unobserved_semantics(self):
        app, foo = self.fixture(); shutil.copy(foo, self.root/'lib')
        _, baseline = self.diagnose(app)
        saved = self.base/'baseline.json'; saved.write_text(json.dumps(baseline))
        log = self.base/'loader.log'
        log.write_text("ignored application output\n"
                       "  77: binding file /app [1] to /lib/libfoo.so [1]: normal symbol `foo'\n"
                       "  88: binding file /app [0] to /lib/libfoo.so [0]: normal symbol `foo'\n")
        code, result = self.call('trace', '--log', log, '--baseline', saved, '--format', 'json')
        self.assertEqual(code, 0)
        events = result['events']
        self.assertEqual([(e['pid'],e['line']) for e in events], [(77,2),(88,3)])
        self.assertTrue(all(e['source']==str(log) for e in events))
        checks = result['comparison']['bindings']['checks']
        self.assertEqual([c['status'] for c in checks], ['unmodeled','match'])

    def test_trace_rejects_oversized_log_before_parsing(self):
        log = self.base/'oversized.log'
        with log.open('wb') as stream:
            stream.truncate(32 * 1024 * 1024 + 1)
        code, error = self.call('trace', '--log', log, '--format', 'json')
        self.assertEqual(code, 2)
        self.assertIn('32 MiB', error['error'])

    def test_malformed_elf_headers_return_json_errors_without_panics(self):
        app, _ = self.fixture()
        original = app.read_bytes()
        cases = {f'truncated-{n}': original[:n] for n in [0,1,4,16,63]}
        for label, offset, fmt, value in [
            ('phoff-overflow',32,'Q',2**64-1),
            ('bad-phentsize',54,'H',1),
            ('extended-phnum',56,'H',65535),
            ('bad-header-version',20,'I',99),
        ]:
            data = bytearray(original); struct.pack_into('<'+fmt,data,offset,value); cases[label]=data
        for label, data in cases.items():
            with self.subTest(case=label):
                path = self.base/label; path.write_bytes(data)
                for command in ['inspect','diagnose']:
                    code, error = self.call(command, path, '--format', 'json')
                    self.assertEqual(code, 2, (label,command,error))
                    self.assertEqual(error['kind'], 'error')
                    self.assertEqual(error['status'], 'incomplete')

    def test_seeded_elf_mutations_never_panic_or_emit_invalid_json(self):
        app, _ = self.fixture()
        original = app.read_bytes()
        rng = random.Random(20261030)
        mutated = self.base/'mutated.elf'
        for case in range(32):
            with self.subTest(seed=20261030,case=case):
                data = bytearray(original)
                for _ in range(1 + case % 4):
                    offset = rng.randrange(len(data)); data[offset] ^= 1 << rng.randrange(8)
                mutated.write_bytes(data)
                code, report = self.call('inspect', mutated, '--format', 'json')
                self.assertIn(code, [0,2])
                self.assertEqual(report['schema_version'], 1)
                self.assertEqual(report['kind'], 'inspection' if code==0 else 'error')


if __name__ == '__main__':
    unittest.main()
