#!/usr/bin/env python3
"""Build trusted ELF fixtures and retain actual diagnostic output. Never runs the fixture."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--binary', default='target/debug/ExeViewer')
p.add_argument('--work-dir', required=True, type=Path, help='New directory; never overwritten')
a = p.parse_args()
binary = Path(a.binary).resolve()
work = a.work_dir.resolve()
work.mkdir(parents=True, exist_ok=False)
loader = Path('/lib64/ld-linux-x86-64.so.2')
if not loader.exists():
    raise SystemExit('Demo requires Linux x86-64, glibc, GCC and Python 3')
for name in ['a-root', 'b-root']:
    (work/name/'lib').mkdir(parents=True)
    (work/name/'lib64').mkdir()
    shutil.copyfile(loader, work/name/'lib64'/loader.name)
(work/'foo.c').write_text('int foo(void) { return 0; }\n')
for name, version in [('a-root', 'FOO_2.0'), ('b-root', 'FOO_1.0')]:
    version_map = work/(name+'.map')
    version_map.write_text(version+' { global: foo; local: *; };\n')
    subprocess.run(['gcc','-nostdlib','-fPIC','-shared',str(work/'foo.c'),
                    '-Wl,-soname,libfoo.so','-Wl,--version-script='+str(version_map),
                    '-o',str(work/name/'lib/libfoo.so')], check=True)
(work/'app.c').write_text('extern int foo(void); void _start(void) { long s=foo(); __asm__ volatile("syscall"::"a"(60L),"D"(s):"rcx","r11","memory"); __builtin_unreachable(); }\n')
subprocess.run(['gcc','-nostdlib','-fPIC',str(work/'app.c'),'-L'+str(work/'a-root/lib'),
                '-lfoo','-Wl,-e,_start','-Wl,--dynamic-linker,'+str(loader),'-o',str(work/'app')],check=True)

def run(filename, args, expected):
    r = subprocess.run([str(binary), *map(str,args)], capture_output=True, text=True, check=False)
    if r.returncode != expected:
        raise RuntimeError(f'{args}: expected {expected}, got {r.returncode}\n{r.stdout}\n{r.stderr}')
    value = json.loads(r.stdout)
    (work/filename).write_text(r.stdout)
    return value

for name in ['a', 'b']:
    report = run(name+'.json', ['diagnose',work/'app','--rootfs',work/(name+'-root'),
                 '--target-path','/opt/demo/app','--default-path','/lib','--format','json'], 0 if name=='a' else 1)
    if name == 'b': broken = report
comparison = run('comparison.json', ['compare','--env-a',work/'a.json','--env-b',work/'b.json',
                                    '--explain','--format','json'], 1)
original_b = (work/'b-root/lib/libfoo.so').read_bytes()
(work/'fix.json').write_text(json.dumps({'schema_version':1,'kind':'diagnosis_overrides',
    'replacements':[{'target':'/lib/libfoo.so','source':'a-root/lib/libfoo.so'}]},indent=2)+'\n')
fixed = run('repair.json', ['diagnose',work/'app','--rootfs',work/'b-root','--target-path','/opt/demo/app',
            '--default-path','/lib','--overrides',work/'fix.json','--verification-dir',work/'verification','--format','json'],0)
assert (work/'b-root/lib/libfoo.so').read_bytes() == original_b, 'Demo modified the original B library'
# Screenshots are explicitly selected JSON fields, not a new application UI.
version = next(d for d in broken['diagnostics'] if d['rule_id']=='VERSION_MISSING')
root = next(d for d in comparison['explanation']['root_causes'] if d['rule_id']=='VERSION_MISSING')
scenes = [
 ('01-diagnosis', 'B requires a version its library does not provide',
  '$ ExeViewer diagnose "$DEMO/app" --rootfs "$DEMO/b-root" \\\n    --target-path /opt/demo/app --default-path /lib --format json',
  {'status':broken['status'],'rule_id':version['rule_id'],'object':version['object'],
   'message':version['message'],'evidence':version['evidence']},1),
 ('02-root-cause', 'A and B select the same path, but different library contents',
  '$ ExeViewer compare --env-a "$DEMO/a.json" --env-b "$DEMO/b.json" \\\n    --explain --format json',
  {k:root[k] for k in ['rule_id','certainty','first_divergence','consequence','explanation']},1),
 ('03-repair', 'A proposed replacement removes the static blockers',
  '$ ExeViewer diagnose "$DEMO/app" --rootfs "$DEMO/b-root" \\\n    --target-path /opt/demo/app --default-path /lib \\\n    --overrides "$DEMO/fix.json" --verification-dir "$DEMO/verification" \\\n    --format json',
  {'before_status':fixed['before_status'],'after_status':fixed['after_status'],
   'runtime_validation':fixed['runtime_validation'],
   'resolved_rules':[d['rule_id'] for d in fixed['resolved_findings']],
   'introduced_findings':fixed['introduced_findings']},0),
]
for name,title,command,excerpt,code in scenes:
    (work/(name+'.txt')).write_text(command+'\n\n# Selected fields from actual JSON output\n'+json.dumps(excerpt,indent=2)+'\n\n# ExeViewer exit code: '+str(code)+'\n')
(work/'scenes.json').write_text(json.dumps([{'name':s[0],'title':s[1]} for s in scenes],indent=2))
print(f'Actual reports, fixture sources, repair copies and transcripts saved in {work}')
