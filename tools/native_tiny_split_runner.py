"""Host-only exact-artifact tiny KerML runner: setup30/model15/total45.

This preserves the original input/archive checks. The first physical pipeline
marker starts the model allowance once; neither a late nor repeated marker can
reset setup or extend the process deadline. Clocks and subprocess control live
only in this host tool. The previous combined15 protocol is a separate receipt.
"""
import argparse,hashlib,json,math,os,re,shutil,subprocess,sys,time,zipfile
from pathlib import Path

SETUP_SECONDS=30.0
MODEL_SECONDS=15.0
PROCESS_SECONDS=45.0
POLL_SECONDS=0.02

def stage_deadline(start,pipeline_elapsed):
 if pipeline_elapsed is None:return start+SETUP_SECONDS,'setup'
 if not math.isfinite(pipeline_elapsed) or pipeline_elapsed<0:
  raise ValueError('Invalid first physical pipeline timestamp')
 if pipeline_elapsed>SETUP_SECONDS:return start+SETUP_SECONDS,'setup'
 model=start+pipeline_elapsed+MODEL_SECONDS
 total=start+PROCESS_SECONDS
 return (model,'model') if model<total else (total,'process')

def complete_rows(raw):
 if len(raw)>1024*1024:raise ValueError('Bounded tiny trace exceeded1MiB')
 # Flushes can be observed between a JSON write and its newline; wait for the
 # complete row rather than interpreting an incomplete diagnostic as a failure.
 complete=raw[:raw.rfind(b'\n')+1]
 return [json.loads(line) for line in complete.splitlines() if line]

def read_phases(path):
 return complete_rows(path.read_bytes()) if path.exists() else []

def first_pipeline(phases):
 return next((r['cold_elapsed_seconds'] for r in phases if r['phase']=='kerml_physical_pipeline_begin'),None)

def self_check():
 assert stage_deadline(100,None)==(130,'setup')
 assert stage_deadline(100,12)==(127,'model')
 assert stage_deadline(100,30)==(145,'process')
 assert stage_deadline(100,30.1)==(130,'setup')
 assert first_pipeline([{'phase':'kerml_physical_pipeline_begin','cold_elapsed_seconds':12},{'phase':'kerml_physical_pipeline_begin','cold_elapsed_seconds':14}])==12
 try:stage_deadline(100,float('nan'))
 except ValueError:pass
 else:raise AssertionError('Nonfinite timestamp accepted')
 assert complete_rows(b'{')==[]
 assert complete_rows(b'{}\n{')==[{}]
 print(json.dumps({'runner_controls':8,'passed':True,'native_qualification':False}))

if sys.argv[1:]==['--self-check']:
 self_check();raise SystemExit(0)

p=argparse.ArgumentParser();p.add_argument('guard',type=Path);p.add_argument('out',type=Path);p.add_argument('--root',type=Path,required=True);p.add_argument('--corpus',type=Path,required=True);p.add_argument('--definition-set',choices=['kerml'],default='kerml');a=p.parse_args()
a.out.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest()
digest=lambda path:sha(path.read_bytes())
g=json.loads(a.guard.read_text());assert all(digest(a.root/n)==s for n,s in g['files'].items())
artifact=next(x for x in g['artifacts'] if x['target']==g['target']);exe=a.out/(g['target']+'.exe')
assert digest(Path(artifact['artifact']))==g['binary_sha256'];shutil.copy2(artifact['artifact'],exe)
shutil.copy2(a.guard,a.out/'source-guard.json')
with zipfile.ZipFile(a.out/'compiler-inputs.zip','x',compression=zipfile.ZIP_DEFLATED) as z:
 for n in sorted(g['files']):z.write(a.root/n,n)
manifest_bytes=(a.corpus/'manifest.json').read_bytes();manifest=json.loads(manifest_bytes)
assert manifest['pilot_commit']=='692170b71867353b8f90341e61556f49a5beb0e5' and len(manifest['definitions'])==4
host=(a.root/'crates/intersect-kerml/examples'/ (g['target']+'.rs')).read_text()
constant=lambda name:json.loads(re.search(r'const '+name+r':\s*&str\s*=\s*("(?:[^"\\]|\\.)*")\s*;',host).group(1))
selected=manifest['definitions'] if a.definition_set=='paired' else [r for r in manifest['definitions'] if r['source_uri']!='org.omg.sysml.xtext/src/org/omg/sysml/xtext/SysML.xtext']
assert len(selected)==(4 if a.definition_set=='paired' else 3)
inputs=[]
with zipfile.ZipFile(a.out/'runtime-inputs.zip','x',compression=zipfile.ZIP_DEFLATED) as z:
 z.writestr('manifest.json',manifest_bytes)
 for row in selected:
  source=(a.corpus/row['fixture_path']).resolve();assert source.is_relative_to(a.corpus.resolve())
  b=source.read_bytes();assert len(b)==row['bytes'] and sha(b)==row['sha256']
  z.writestr(row['fixture_path'],b);inputs.append({'uri':row['source_uri'],'sha256':sha(b),'bytes':len(b),'kind':'original-definition'})
 for prefix in ['KERNEL','SYSTEMS']:
  uri=constant(prefix+'_URI');b=constant(prefix+'_TEXT').encode();z.writestr('local/'+uri,b)
  inputs.append({'uri':uri,'sha256':sha(b),'bytes':len(b),'kind':'local-model'})
(a.out/'runtime-inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
env=os.environ.copy();env.pop('RUST_MIN_STACK',None)
start=time.monotonic();pipeline_elapsed=None;timeout_stage=None;externally_killed=False
with (a.out/'stdout.log').open('wb') as stdout,(a.out/'stderr.log').open('wb') as stderr:
 proc=subprocess.Popen([str(exe),str(a.corpus),str(a.out)],cwd=a.root,env=env,stdout=stdout,stderr=stderr)
 (a.out/'process.json').write_text(json.dumps({'pid':proc.pid,'setup_cap_seconds':SETUP_SECONDS,'model_cap_seconds':MODEL_SECONDS,'process_cap_seconds':PROCESS_SECONDS,'whole_language_qualified':False},indent=2)+'\n')
 print(json.dumps({'pid':proc.pid,'setup_seconds':SETUP_SECONDS,'model_seconds':MODEL_SECONDS,'process_seconds':PROCESS_SECONDS,'binary_sha256':digest(exe)}),flush=True)
 try:
  while True:
   phases=read_phases(a.out/'phases.jsonl')
   if pipeline_elapsed is None:pipeline_elapsed=first_pipeline(phases)
   deadline,stage=stage_deadline(start,pipeline_elapsed)
   now=time.monotonic()
   if now>=deadline:
    timeout_stage=stage
    if proc.poll() is None:externally_killed=True;proc.kill()
    code=proc.wait();break
   if proc.poll() is not None:code=proc.returncode;break
   try:proc.wait(timeout=min(POLL_SECONDS,deadline-now))
   except subprocess.TimeoutExpired:continue
 except BaseException:
  if proc.poll() is None:proc.kill();proc.wait()
  raise
 elapsed=time.monotonic()-start
phases=read_phases(a.out/'phases.jsonl')
if pipeline_elapsed is None:pipeline_elapsed=first_pipeline(phases)
deadline,stage=stage_deadline(start,pipeline_elapsed)
if elapsed>deadline-start:timeout_stage=stage
timed_out=timeout_stage is not None
seen=next((r for r in phases if r['phase']=='inputs_complete'),None)
actual_models=seen['data']['models'] if seen else []
models_match=bool(seen) and all(any(m['uri']==r['uri'] and m['sha256']==r['sha256'] for m in actual_models) for r in inputs if r['kind']=='local-model')
with zipfile.ZipFile(a.out/'compiler-inputs.zip') as z:archive_ok=all(sha(z.read(n))==s for n,s in g['files'].items())
with zipfile.ZipFile(a.out/'runtime-inputs.zip') as z:
 runtime_ok=sha(z.read('manifest.json'))==sha(manifest_bytes)
 for row in selected:runtime_ok=runtime_ok and sha(z.read(row['fixture_path']))==row['sha256']
 for row in inputs:
  if row['kind']=='local-model':runtime_ok=runtime_ok and sha(z.read('local/'+row['uri']))==row['sha256']
paired={}
for row in phases:
 if row['phase'].startswith('paired_reference_'):paired[row['phase']]=paired.get(row['phase'],0)+1
host_report=json.loads((a.out/'report.json').read_text()) if (a.out/'report.json').exists() else None
host_ok=host_report.get('result',{}).get('Ok') if host_report else None
reported_paired=host_ok.get('paired',{}) if host_ok else {}
actual_typing=bool(host_ok and host_ok.get('observations',{}).get('actual_typing') and host_ok.get('observations',{}).get('physical_constructed_linked'))
result={'protocol':'setup30-model15-total45','setup_cap_seconds':SETUP_SECONDS,'model_cap_seconds':MODEL_SECONDS,'process_cap_seconds':PROCESS_SECONDS,
 'cold_process_seconds':elapsed,'timed_out':timed_out,'timeout_stage':timeout_stage,'externally_killed':externally_killed,'exit_code':code,
 'first_pipeline_host_elapsed_seconds':pipeline_elapsed,'conservative_model_process_seconds':elapsed-pipeline_elapsed if pipeline_elapsed is not None else None,
 'deadline_process_seconds':deadline-start,'reported_paired_events':reported_paired,'actual_typing_verified_by_host':actual_typing,
 'trace_suppressed':host_ok.get('trace_suppressed') if host_ok else None,'original_combined15_cold_gate_claimed':False,
 'last_observed_phase':phases[-1]['phase'] if phases else 'no-host-event','request_reached':any(r['phase']=='request_begin' for r in phases),
 'phase_events':len(phases),'paired_events':paired,'new_path_observed_applied':paired.get('paired_reference_applied',0)>0,'new_path_reported_applied':reported_paired.get('paired_reference_applied',0)>0,
 'source_inputs':len(g['files']),'current_source_match':all(digest(a.root/n)==s for n,s in g['files'].items()),'source_archive_match':archive_ok,
 'binary_match':digest(exe)==g['binary_sha256'],'binary_sha256':digest(exe),'runtime_input_count':len(inputs),'runtime_archive_match':runtime_ok,
 'runtime_models_match_actual_host':models_match,'runtime_archive_sha256':digest(a.out/'runtime-inputs.zip'),'source_archive_sha256':digest(a.out/'compiler-inputs.zip'),
 'runner_sha256':digest(Path(__file__)),'definition_set':a.definition_set,'stdlib_files':0,'whole_language_qualified':False,'new_path_success_claimed':code==0 and not timed_out and actual_typing and reported_paired.get('paired_reference_applied',0)>0}
result['new_path_success_claimed']=result['new_path_success_claimed'] and all(result[key] for key in ['current_source_match','source_archive_match','binary_match','runtime_archive_match','runtime_models_match_actual_host'])
(a.out/'runner.py').write_bytes(Path(__file__).read_bytes())
(a.out/'terminal-postguard.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result),flush=True)
