"""End-to-end fixtures, including independent glibc loader checks.
Run with: python3 tests/diagnose_scenarios.py target/debug/ExeViewer
Requires Linux x86-64, gcc and a glibc loader; never runs untrusted inputs.
"""
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest

BINARY = str(Path(sys.argv.pop(1)).resolve())
LOADER = Path('/lib64/ld-linux-x86-64.so.2')

class DiagnosisTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='exeviewer-test-')
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        self.build = self.base / 'build'
        self.build.mkdir()
        self.root = self.base / 'root'
        (self.root / 'lib64').mkdir(parents=True)
        (self.root / 'lib').mkdir()
        shutil.copyfile(LOADER, self.root / 'lib64' / LOADER.name)

    def cc(self, name, code, *args):
        source = self.build / (name + '.c')
        source.write_text(code)
        out = self.build / name
        subprocess.run(['gcc', '-nostdlib', '-fPIC', str(source), '-o', str(out), *args], check=True, capture_output=True)
        return out

    def fixture(self, rpath=None, inherited=False, version=None, transitive=False):
        if transitive:
            bar = self.cc('libbar.so', 'int bar(void){return 1;}', '-shared', '-Wl,-soname,libbar.so')
            extra = ['-L'+str(self.build), '-lbar']
            code = 'extern int bar(void);int foo(void){return bar();}'
        else:
            extra = []
            code = 'int foo(void){return 1;}'
        if version:
            script = self.build / 'version.map'
            script.write_text(version + ' { global: foo; local: *; };')
            extra += ['-Wl,--version-script='+str(script)]
        foo = self.cc('libfoo.so', code, '-shared', '-Wl,-soname,libfoo.so', *extra)
        args = ['-L'+str(self.build), '-lfoo', '-Wl,-rpath-link,'+str(self.build), '-Wl,-e,_start', '-Wl,--dynamic-linker,'+str(LOADER)]
        if rpath:
            args += ['-Wl,-rpath,'+rpath]
        if inherited:
            args += ['-Wl,--disable-new-dtags']
        app = self.cc('app', 'extern int foo(void);void _start(void){foo();}', *args)
        return app, foo

    def call(self, *args):
        p = subprocess.run([BINARY, *map(str,args)], capture_output=True, timeout=15)
        self.assertNotIn(b'panicked', p.stderr)
        self.assertNotIn(b'\x1b', p.stdout)
        return p.returncode, json.loads(p.stdout)

    def diagnose(self, app, *args):
        return self.call('diagnose', app, '--rootfs', self.root, '--target-path', '/app', '--default-path', '/lib', '--format', 'json', *args)

    def test_transitive_and_origin(self):
        app, foo = self.fixture('$ORIGIN/lib', inherited=True, transitive=True)
        shutil.copy(foo, self.root/'lib')
        shutil.copy(self.build/'libbar.so', self.root/'lib')
        code, d = self.diagnose(app)
        self.assertEqual(code, 0, d['diagnostics'])
        self.assertTrue(any(e['requester']=='/lib/libfoo.so' and e['selected']=='/lib/libbar.so' for e in d['dependencies']))

    def test_rpath_inherited_but_runpath_not_inherited(self):
        for inherited in (False, True):
            app, foo = self.fixture('/opt/lib', inherited=inherited, transitive=True)
            target=self.root/'opt/lib';target.mkdir(parents=True,exist_ok=True)
            shutil.copy(foo,target);shutil.copy(self.build/'libbar.so',target)
            code,d=self.diagnose(app)
            self.assertEqual(code,0 if inherited else 2,d['diagnostics'])
            self.assertTrue(any(e['needed']=='libbar.so' and (e['selected'] is not None)==inherited for e in d['dependencies']))

    def test_missing_interpreter_and_library(self):
        app,_=self.fixture()
        (self.root/'lib64'/LOADER.name).unlink()
        code,d=self.diagnose(app)
        self.assertEqual(code,2)  # unresolved symbols cannot be conclusively assessed
        self.assertTrue({'INTERPRETER_MISSING','LIBRARY_NOT_FOUND'} <= {x['rule_id'] for x in d['diagnostics']})
        self.assertTrue(all(e['selected'] is None for e in d['dependencies']))

    def test_version_mismatch_and_comparison(self):
        app,foo=self.fixture(version='FOO_2.0')
        shutil.copy(foo,self.root/'lib')
        code,a=self.diagnose(app);self.assertEqual(code,0,a['diagnostics'])
        self.fixture(version='FOO_1.0')  # overwrites app too, preserve original below
        # Use original executable captured before rebuild via a saved copy.
        # Build an executable requiring FOO_2.0, then replace only target library.
        oldlib=(self.build/'libfoo.so').read_bytes()
        app,foo=self.fixture(version='FOO_2.0')
        (self.root/'lib/libfoo.so').write_bytes(oldlib)
        code,b=self.diagnose(app)
        self.assertEqual(code,1,b['diagnostics'])
        self.assertTrue(any(x['rule_id']=='VERSION_MISSING' and 'FOO_2.0' in x['message'] for x in b['diagnostics']))
        for name,data in [('a',a),('b',b)]: (self.base/(name+'.json')).write_text(json.dumps(data))
        code,d=self.call('compare','--env-a',self.base/'a.json','--env-b',self.base/'b.json','--format','json')
        self.assertEqual(code,1,d)
        self.assertTrue(d['changes'])

    def test_absolute_target_symlink(self):
        app,foo=self.fixture()
        (self.root/'actual').mkdir();shutil.copy(foo,self.root/'actual')
        (self.root/'lib/libfoo.so').symlink_to('/actual/libfoo.so')
        code,d=self.diagnose(app)
        self.assertEqual(code,0,d['diagnostics'])
        self.assertEqual(next(e for e in d['dependencies'] if e['needed']=='libfoo.so')['selected'],'/lib/libfoo.so')

    def test_no_host_library_path_leak(self):
        app,foo=self.fixture()
        old=os.environ.get('LD_LIBRARY_PATH');os.environ['LD_LIBRARY_PATH']=str(self.build)
        try:
            code,d=self.diagnose(app)
            self.assertNotEqual(code,0)
            self.assertEqual(d['environment']['library_path'],[])
        finally:
            if old is None: os.environ.pop('LD_LIBRARY_PATH',None)
            else: os.environ['LD_LIBRARY_PATH']=old

    def test_sectionless_symbols_and_versions(self):
        app,foo=self.fixture(version='FOO_2.0');shutil.copy(foo,self.root/'lib')
        data=bytearray(app.read_bytes());data[40:48]=b'\0'*8;data[60:64]=b'\0'*4;app.write_bytes(data)
        code,d=self.diagnose(app)
        self.assertEqual(code,0,d['diagnostics'])
        self.assertTrue(d['objects'][0]['elf']['versions_needed'])
        self.assertTrue(d['objects'][0]['elf']['symbols'])

    def test_bad_input_is_json_error(self):
        app=self.build/'bad';app.write_bytes(b'\x7fELF')
        code,d=self.diagnose(app);self.assertEqual(code,2);self.assertEqual(d['kind'],'error')

    def test_local_matches_real_loader(self):
        code,d=self.call('diagnose','/bin/true','--library-path','','--preload','','--format','json')
        self.assertEqual(code,0,d.get('diagnostics'))
        oracle=subprocess.run([str(LOADER),'--list','/bin/true'],check=True,capture_output=True,text=True).stdout
        for edge in d['dependencies']:
            if edge['needed']=='libc.so.6':
                matches=[line for line in oracle.splitlines() if 'libc.so.6 =>' in line]
                self.assertTrue(matches)
                resolved=matches[0].split('=>')[1].split('(')[0].strip()
                self.assertEqual(Path(edge['selected']).resolve(),Path(resolved).resolve())

    def test_trace_import(self):
        env=os.environ.copy();env['LD_DEBUG']='libs,files,versions'
        p=subprocess.run(['/bin/true'],env=env,capture_output=True,check=True)
        log=self.base/'loader.log';log.write_bytes(p.stderr)
        code,d=self.call('trace','--log',log,'--format','json')
        self.assertEqual(code,0);self.assertTrue(any(e['kind']=='initialize' for e in d['events']))
        self.assertTrue(any(e['kind']=='candidate' for e in d['events']))

    def test_trace_rejects_application_messages(self):
        log = self.base/'application.log'
        log.write_text('application error: optional config absent; continuing normally\n'
                       'application: calling init: pretend.so\n'
                       '123: arbitrary error: recoverable\n')
        code, report = self.call('trace', '--log', log, '--format', 'json')
        self.assertEqual(code, 2)
        self.assertEqual(report['events'], [])

    def test_trace_recognizes_loader_failure(self):
        app, foo = self.fixture()
        env = os.environ.copy()
        env.pop('LD_LIBRARY_PATH', None)
        env.pop('LD_PRELOAD', None)
        p = subprocess.run([app], env=env, capture_output=True)
        self.assertNotEqual(p.returncode, 0)
        log = self.base/'failed.log'; log.write_bytes(p.stderr)
        code, report = self.call('trace', '--log', log, '--format', 'json')
        self.assertEqual(code, 1, report)
        self.assertEqual(report['events'][0]['kind'], 'loader_error')

    def test_trace_observes_preload_symbol_provider(self):
        foo = self.cc('libfoo.so', 'int foo(void){return 0;}', '-shared')
        override = self.cc('override.so', 'int foo(void){return 0;}', '-shared')
        source = self.build/'main.c'
        source.write_text('extern int foo(void);int main(void){return foo();}')
        app = self.build/'main'
        subprocess.run(['gcc', str(source), '-o', str(app), '-L'+str(self.build),
                        '-lfoo', '-Wl,-rpath,'+str(self.build)], check=True)
        for provider, preload in [(foo, ''), (override, str(override))]:
            env = os.environ.copy()
            env.update(LD_DEBUG='bindings', LD_PRELOAD=preload)
            env.pop('LD_DEBUG_OUTPUT', None)
            p = subprocess.run([app], env=env, capture_output=True, check=True)
            log = self.base/'bindings.log'; log.write_bytes(p.stderr)
            code, report = self.call('trace', '--log', log, '--format', 'json')
            self.assertEqual(code, 0)
            bindings = [e['binding'] for e in report['events'] if e['kind']=='binding']
            found = [b for b in bindings if b['symbol']=='foo']
            self.assertEqual(len(found), 1)
            self.assertEqual(found[0]['provider'], str(provider))
            self.assertEqual(found[0]['requester'], str(app))
            self.assertEqual(found[0]['requester_namespace'], '0')
            self.assertTrue(any(b['version'] and b['version'].startswith('GLIBC_') for b in bindings))
            code, diagnosis = self.call('diagnose', app, '--preload', preload, '--format', 'json')
            self.assertEqual(code, 0, diagnosis['diagnostics'])
            prediction = next(b for b in diagnosis['bindings'] if b['requester']==str(app) and b['symbol']=='foo')
            self.assertEqual(prediction['provider'], str(provider))
            self.assertEqual(prediction['certainty'], 'predicted')
            baseline = self.base/'baseline.json'; baseline.write_text(json.dumps(diagnosis))
            code, checked = self.call('trace', '--log', log, '--baseline', baseline, '--format', 'json')
            checks = checked['comparison']['bindings']['checks']
            self.assertTrue(any(c['status']=='match' and c['observed']['symbol']=='foo' for c in checks))
            if preload:
                self.assertIn(str(foo), prediction['shadowed'])
                original = self.base/'original.json'
                code, checked = self.call('trace', '--log', log, '--baseline', original, '--format', 'json')
                self.assertTrue(any(c['status']=='mismatch' and c['observed']['symbol']=='foo'
                                    for c in checked['comparison']['bindings']['checks']))
            else:
                (self.base/'original.json').write_text(json.dumps(diagnosis))


    def test_environment_capture_replay_and_drift(self):
        app, foo = self.fixture()
        shutil.copy(foo, self.root/'lib')
        snapshot = self.base/'env.json'
        code, summary = self.call('env', 'capture', app, '--rootfs', self.root,
                                 '--target-path', '/app', '--default-path', '/lib',
                                 '--output', snapshot, '--format', 'json')
        self.assertEqual(code, 0, summary)
        manifest = json.loads(snapshot.read_text())
        self.assertEqual(manifest['default_paths_source'], 'explicit input')
        self.assertEqual(manifest['cpu_flags'], [])
        moved = self.base/'moved-root'
        shutil.copytree(self.root, moved, symlinks=True)
        code, replay = self.call('diagnose', app, '--env', snapshot, '--rootfs', moved, '--format', 'json')
        self.assertEqual(code, 0, replay)
        self.assertTrue(any(e['selected']=='/lib/libfoo.so' for e in replay['dependencies']))
        (moved/'lib/libfoo.so').write_bytes(b'changed')
        code, bad = self.call('diagnose', app, '--env', snapshot, '--rootfs', moved, '--format', 'json')
        self.assertEqual(code, 2)
        self.assertIn('evidence changed', bad['error'])
        code, bad = self.call('env', 'capture', app, '--rootfs', self.root,
                             '--target-path', '/app', '--default-path', '/lib',
                             '--output', snapshot, '--format', 'json')
        self.assertEqual(code, 2)
        self.assertEqual(json.loads(snapshot.read_text()), manifest)

    def test_environment_rejects_new_search_candidate(self):
        app, foo = self.fixture()
        shutil.copy(foo, self.root/'lib')
        (self.root/'early').mkdir()
        snapshot = self.base/'env.json'
        code, _ = self.call('env', 'capture', app, '--rootfs', self.root,
                           '--target-path', '/app', '--default-path', '/lib',
                           '--library-path', '/early', '--output', snapshot, '--format', 'json')
        self.assertEqual(code, 0)
        shutil.copy(foo, self.root/'early')
        code, bad = self.call('diagnose', app, '--env', snapshot, '--rootfs', self.root, '--format', 'json')
        self.assertEqual(code, 2)
        self.assertIn('/early/libfoo.so', bad['error'])

    def test_binding_prediction_preserves_version_candidates(self):
        app, foo = self.fixture(version='FOO_2.0')
        replacement = self.cc('wrong.so', 'int unrelated(void){return 0;}', '-shared')
        shutil.copy(replacement, self.root/'lib/libfoo.so')
        code, report = self.diagnose(app)
        self.assertEqual(code, 1)
        binding = next(b for b in report['bindings'] if b['symbol']=='foo')
        self.assertEqual(binding['version'], 'FOO_2.0')
        self.assertIsNone(binding['provider'])
        self.assertEqual(binding['candidates'], [])
        (self.root/'lib/libfoo.so').unlink()
        code, report = self.diagnose(app)
        binding = next(b for b in report['bindings'] if b['symbol']=='foo')
        self.assertEqual(binding['certainty'], 'unknown')
        self.assertIsNone(binding['provider'])

    def test_cache_precedes_default_directory(self):
        app,foo=self.fixture()
        (self.root/'cached').mkdir();shutil.copy(foo,self.root/'cached')
        (self.root/'etc').mkdir()
        key=b'libfoo.so\0';val=b'/cached/libfoo.so\0'
        header=bytearray(48);header[:20]=b'glibc-ld.so.cache1.1';struct.pack_into('<II',header,20,1,len(key)+len(val))
        entry=struct.pack('<iIIIQ',0x303,72,72+len(key),0,0)
        (self.root/'etc/ld.so.cache').write_bytes(header+entry+key+val)
        code,d=self.diagnose(app);self.assertEqual(code,0,d['diagnostics'])
        edge=next(e for e in d['dependencies'] if e['needed']=='libfoo.so')
        self.assertEqual(edge['selected'],'/cached/libfoo.so')
        self.assertEqual(edge['attempts'][-1]['source'],'ld.so.cache')

    def test_rpath_and_runpath_library_path_priority(self):
        for inherited in (False,True):
            app,foo=self.fixture('/rpath',inherited=inherited)
            for name in ('rpath','env'):
                (self.root/name).mkdir(exist_ok=True);shutil.copy(foo,self.root/name)
            code,d=self.diagnose(app,'--library-path','/env')
            self.assertEqual(code,0,d['diagnostics'])
            self.assertEqual(d['dependencies'][0]['selected'],'/rpath/libfoo.so' if inherited else '/env/libfoo.so')

    def test_missing_strong_symbol(self):
        app,foo=self.fixture()
        replacement=self.cc('replacement.so','int other(void){return 1;}','-shared','-Wl,-soname,libfoo.so')
        shutil.copyfile(replacement,self.root/'lib/libfoo.so')
        code,d=self.diagnose(app)
        self.assertEqual(code,1,d['diagnostics'])
        self.assertTrue(any(x['rule_id']=='SYMBOL_UNRESOLVED' and 'foo' in x['message'] for x in d['diagnostics']))

    def test_preload_reuse(self):
        app,foo=self.fixture('/opt/lib',inherited=True)
        (self.root/'opt/lib').mkdir(parents=True);shutil.copy(foo,self.root/'opt/lib')
        code,d=self.diagnose(app,'--preload','libfoo.so')
        self.assertEqual(code,0,d['diagnostics'])
        edges=[e for e in d['dependencies'] if e['needed']=='libfoo.so']
        self.assertEqual(len(edges),2);self.assertTrue(edges[1]['reused'])

    def test_symlink_parent_is_resolved_before_dotdot(self):
        app,foo=self.fixture('/alias/../lib')
        (self.root/'nested/child').mkdir(parents=True);(self.root/'nested/lib').mkdir()
        shutil.copy(foo,self.root/'nested/lib');(self.root/'alias').symlink_to('/nested/child')
        code,d=self.diagnose(app);self.assertEqual(code,0,d['diagnostics'])
        self.assertEqual(d['dependencies'][0]['selected'],'/alias/../lib/libfoo.so')

    def test_unknown_cpu_hwcaps_never_claims_success(self):
        app,foo=self.fixture();shutil.copy(foo,self.root/'lib')
        (self.root/'lib/glibc-hwcaps').mkdir()
        code,d=self.diagnose(app)
        self.assertEqual(code,2);self.assertTrue(any(x['rule_id']=='HWCAPS_UNKNOWN' for x in d['diagnostics']))

    def test_unknown_install_path(self):
        app,foo=self.fixture('$ORIGIN/lib');shutil.copy(foo,self.root/'lib')
        code,d=self.call('diagnose',app,'--rootfs',self.root,'--format','json')
        self.assertEqual(code,2);self.assertTrue(any(x['rule_id']=='INSTALL_PATH_UNKNOWN' for x in d['diagnostics']))

    def test_trace_capture_and_stdout_separation(self):
        out=self.base/'capture'
        code,d=self.call('trace','--execute','/bin/echo','--output-dir',out,'--format','json','--','hello')
        self.assertEqual(code,0,d)
        self.assertEqual((out/'stdout.log').read_text(),'hello\n')
        self.assertEqual(d['kind'],'trace_capture')
        self.assertTrue(d['events'])
        code,d=self.call('trace','--execute','/bin/echo','--output-dir',out,'--format','json')
        self.assertEqual(code,2);self.assertEqual((out/'stdout.log').read_text(),'hello\n')

    def test_trace_timeout(self):
        code,d=self.call('trace','--execute','/bin/sleep','--output-dir',self.base/'sleep','--timeout','1','--format','json','--','5')
        self.assertEqual(code,2,d);self.assertEqual(d['capture']['stopped'],'timeout')

    def test_snapshot_comparison_identity_and_invalid_artifact(self):
        app,foo=self.fixture();shutil.copy(foo,self.root/'lib')
        code,d=self.diagnose(app);self.assertEqual(code,0,d['diagnostics'])
        a=self.base/'a.json';a.write_text(json.dumps(d))
        code,result=self.call('compare','--env-a',a,'--env-b',a,'--format','json')
        self.assertEqual(code,0);self.assertEqual(result['changes'],[])
        d['objects'][0]['elf']['sha256']='different';b=self.base/'b.json';b.write_text(json.dumps(d))
        code,result=self.call('compare','--env-a',a,'--env-b',b,'--format','json')
        self.assertEqual(code,2);self.assertEqual(result['kind'],'error')

    def note_fixture(self, properties):
        notes=b''
        for kind,desc in properties:
            notes+=struct.pack('<III',4,len(desc),kind)+b'GNU\0'+desc
        header=bytearray(64);header[:7]=b'\x7fELF\x02\x01\x01'
        struct.pack_into('<HHI',header,16,2,62,1)
        struct.pack_into('<Q',header,32,64)
        struct.pack_into('<HHH',header,52,64,56,1)
        ph=struct.pack('<IIQQQQQQ',4,4,120,0,0,len(notes),len(notes),4)
        path=self.build/'notes';path.write_bytes(header+ph+notes)
        return path

    def test_explicit_isa_requirement(self):
        app=self.note_fixture([(5,struct.pack('<IIII',0xc0008002,4,4,0))])
        code,d=self.diagnose(app,'--isa-level','2')
        self.assertEqual(code,1);self.assertTrue(any(x['rule_id']=='ISA_REQUIREMENT' for x in d['diagnostics']))
        code,d=self.diagnose(app,'--isa-level','3');self.assertEqual(code,0)
        code,d=self.diagnose(app);self.assertEqual(code,2)

    def test_explicit_minimum_kernel(self):
        app=self.note_fixture([(1,struct.pack('<IIII',0,5,15,0))])
        code,d=self.diagnose(app,'--kernel-release','4.19.0')
        self.assertEqual(code,1);self.assertTrue(any(x['rule_id']=='KERNEL_REQUIREMENT' for x in d['diagnostics']))
        code,d=self.diagnose(app,'--kernel-release','6.1.0-custom');self.assertEqual(code,0)
        code,d=self.diagnose(app);self.assertEqual(code,2)

    def test_real_loader_rpath_runpath_scope(self):
        target=self.base/'native-libs';target.mkdir()
        for inherited in (False,True):
            app,foo=self.fixture(str(target),inherited=inherited,transitive=True)
            shutil.copy(foo,target);shutil.copy(self.build/'libbar.so',target)
            clean=os.environ.copy();clean.pop('LD_LIBRARY_PATH',None);clean.pop('LD_PRELOAD',None)
            oracle=subprocess.run([str(LOADER),'--list',str(app)],capture_output=True,env=clean)
            code,d=self.call('diagnose',app,'--library-path','','--preload','','--format','json')
            self.assertEqual(oracle.returncode==0,inherited)
            self.assertEqual(code==0,inherited,d['diagnostics'])

    def test_reject_baseline_before_running(self):
        bad=self.base/'bad.json';bad.write_text('{}');out=self.base/'must-not-exist'
        code,d=self.call('trace','--execute','/bin/true','--output-dir',out,'--baseline',bad,'--format','json')
        self.assertEqual(code,2);self.assertFalse(out.exists())

    def test_unversioned_provider_is_warning_not_false_blocker(self):
        app,foo=self.fixture(version='FOO_2.0')
        replacement=self.cc('unversioned.so','int foo(void){return 1;}','-shared','-Wl,-soname,libfoo.so')
        shutil.copyfile(replacement,self.root/'lib/libfoo.so')
        code,d=self.diagnose(app)
        self.assertEqual(code,0,d['diagnostics'])
        self.assertTrue(any(x['rule_id']=='VERSION_METADATA_ABSENT' for x in d['diagnostics']))
        oracle=subprocess.run([str(LOADER),'--library-path',str(self.root/'lib'),'--list',str(app)],capture_output=True)
        self.assertEqual(oracle.returncode,0,oracle.stderr)

    def test_interactive_diagnosis_requires_terminal(self):
        p=subprocess.run([BINARY,'diagnose','/bin/true','--interactive'],capture_output=True)
        self.assertEqual(p.returncode,2);self.assertIn(b'requires a terminal',p.stderr)

    def test_interactive_diagnosis_navigation(self):
        import pty, fcntl, termios, select, time, signal
        pid,fd=pty.fork()
        if pid==0:
            os.execv(BINARY,[BINARY,'diagnose','/bin/true','--interactive'])
        fcntl.ioctl(fd,termios.TIOCSWINSZ,struct.pack('HHHH',30,120,0,0))
        collected=b'';sent=False;exited=False;deadline=time.monotonic()+8
        try:
            while time.monotonic()<deadline:
                readable,_,_=select.select([fd],[],[],0.1)
                if readable:
                    try: chunk=os.read(fd,65536)
                    except OSError: break
                    collected+=chunk
                    if b'no_blockers_in_checked_scope' in collected and not sent:
                        os.write(fd,b'2341q');sent=True
                done,status=os.waitpid(pid,os.WNOHANG)
                if done:
                    exited=True;self.assertEqual(os.waitstatus_to_exitcode(status),0);break
            self.assertTrue(sent,collected)
            if not exited:
                done,status=os.waitpid(pid,os.WNOHANG)
                if done: exited=True;self.assertEqual(os.waitstatus_to_exitcode(status),0)
            self.assertTrue(exited,'TUI did not exit')
        finally:
            if not exited:
                os.kill(pid,signal.SIGKILL);os.waitpid(pid,0)
            os.close(fd)

    def test_cycle_and_duplicate_reuse(self):
        app,foo=self.fixture('/lib',inherited=True,transitive=True)
        # Introduce libbar -> libfoo, closing the cycle after initial link.
        bar=self.cc('libbar.so','extern int foo(void);int bar(void){return foo();}',
                    '-shared','-Wl,-soname,libbar.so','-L'+str(self.build),'-lfoo')
        shutil.copy(foo,self.root/'lib');shutil.copy(bar,self.root/'lib')
        code,d=self.diagnose(app)
        self.assertEqual(code,0,d['diagnostics'])
        self.assertTrue(any(e['requester']=='/lib/libbar.so' and e['needed']=='libfoo.so' and e['reused'] for e in d['dependencies']))
        self.assertLess(len(d['objects']),6)

    def test_missing_preload_is_ignored_by_loader(self):
        app,foo=self.fixture();shutil.copy(foo,self.root/'lib')
        code,d=self.diagnose(app,'--preload','missing-preload.so')
        self.assertEqual(code,0,d['diagnostics'])
        self.assertTrue(any(x['rule_id']=='PRELOAD_NOT_FOUND' and x['severity']=='warning' for x in d['diagnostics']))

if __name__=='__main__':
    unittest.main()
