"""Host-only progressive native Pilot runner over exact Cargo artifacts.

No semantic engine is implemented here. Inputs/config/source/exe are frozen before
one selected process; setup120 and model60 (tiny/parent-ends15) deadlines remain independent.
"""
import argparse,hashlib,json,math,os,shutil,subprocess,sys,time,zipfile
from pathlib import Path
SETUP_SECONDS=120.0
MODEL_SECONDS=60.0
PROCESS_SECONDS=180.0
POLL_SECONDS=0.02
def require(condition,message):
 if not condition:raise ValueError(message)

def expected_model_seconds(case):
 return 15 if case in ('tiny','parent-ends') else 60

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
 return next((r['cold_elapsed_seconds'] for r in phases if r['phase']=='native_physical_pipeline_begin'),None)

def launch_command(exe,case,corpus,out):
 return [str(exe),case,str(corpus),str(out)]

def completed_host(report,case):
 if not isinstance(report,dict) or report.get('schema')!='intersect.native-pilot.bench-terminal.v1':return None
 result=report.get('result',{})
 wire=result.get('Ok') if isinstance(result,dict) else None
 if not isinstance(wire,dict) or wire.get('schema')!='intersect.native-pilot.bench.v1' or wire.get('case')!=case or wire.get('result')!={'Ok':None}:return None
 return wire

def finite_duration(value,limit):
 return isinstance(value,(int,float)) and not isinstance(value,bool) and math.isfinite(value) and 0<=value<=limit

def self_check():
 require(expected_model_seconds('tiny')==15,'Runner deadline mapping failed')
 require(expected_model_seconds('parent-ends')==15,'Runner deadline mapping failed')
 require(expected_model_seconds('all94')==60,'Runner deadline mapping failed')
 require(stage_deadline(100,None)==(220,'setup'),'Runner self-check failed')
 require(stage_deadline(100,40)==(200,'model'),'Runner self-check failed')
 require(stage_deadline(100,120)==(280,'process'),'Runner self-check failed')
 require(stage_deadline(100,120.1)==(220,'setup'),'Runner self-check failed')
 require(first_pipeline([{'phase':'native_physical_pipeline_begin','cold_elapsed_seconds':12},{'phase':'native_physical_pipeline_begin','cold_elapsed_seconds':14}])==12,'Runner self-check failed')
 try:stage_deadline(100,float('nan'))
 except ValueError:pass
 else:raise AssertionError('Nonfinite timestamp accepted')
 require(complete_rows(b'{')==[],'Runner self-check failed')
 require(complete_rows(b'{}\n{')==[{}],'Runner self-check failed')
 wire={'schema':'intersect.native-pilot.bench.v1','case':'tiny','result':{'Ok':None}}
 nested={'schema':'intersect.native-pilot.bench-terminal.v1','result':{'Ok':wire}}
 require(launch_command('bench.exe','tiny','corpus','out')==['bench.exe','tiny','corpus','out'],'Runner self-check failed')
 require(completed_host(nested,'tiny') is wire,'Runner self-check failed')
 require(completed_host(wire,'tiny') is None,'Runner self-check failed')
 require(completed_host(nested,'all94') is None,'Runner self-check failed')
 print(json.dumps({'runner_controls':15,'passed':True,'native_qualification':False}))

if sys.argv[1:]==['--self-check']:
 self_check();raise SystemExit(0)
p=argparse.ArgumentParser();p.add_argument('guard',type=Path);p.add_argument('out',type=Path);p.add_argument('--root',type=Path,required=True);p.add_argument('--corpus',type=Path,required=True);p.add_argument('--case',required=True);a=p.parse_args()
a.root=a.root.resolve(strict=True);a.corpus=a.corpus.resolve(strict=True);a.guard=a.guard.resolve(strict=True);a.out=a.out.resolve()
a.out.mkdir(exist_ok=False)
sha=lambda b:hashlib.sha256(b).hexdigest()
digest=lambda path:sha(path.read_bytes())
g=json.loads(a.guard.read_text());require(g['target']=='native_pilot_bench','Guard is not the benchmark executable')
require(all(digest(a.root/n)==s for n,s in g['files'].items()),'Benchmark pin/schema/deadline precondition failed')
config_path=a.root/'fixtures/native-pilot/bench-cases-v1.json'
config_bytes=config_path.read_bytes();config=json.loads(config_bytes)
require(config['schema']=='intersect.native-pilot.bench-cases.v1','Benchmark pin/schema/deadline precondition failed')
require(g['files'].get('fixtures/native-pilot/bench-cases-v1.json')==sha(config_bytes),'Benchmark pin/schema/deadline precondition failed')
require(len(config['cases'])==len({c['id'] for c in config['cases']}),'Benchmark pin/schema/deadline precondition failed')
case=next(c for c in config['cases'] if c['id']==a.case)
SETUP_SECONDS=float(config['setup_safety_seconds']);MODEL_SECONDS=float(case['model_seconds']);PROCESS_SECONDS=SETUP_SECONDS+MODEL_SECONDS
require(SETUP_SECONDS==120 and MODEL_SECONDS==expected_model_seconds(a.case),'Benchmark pin/schema/deadline precondition failed')
artifact=next(x for x in g['artifacts'] if x['target']==g['target']);exe=a.out/(g['target']+'.exe')
require(digest(Path(artifact['artifact']))==g['binary_sha256'],'Selected executable changed');shutil.copy2(artifact['artifact'],exe)
shutil.copy2(a.guard,a.out/'source-guard.json')
with zipfile.ZipFile(a.out/'compiler-inputs.zip','x',compression=zipfile.ZIP_DEFLATED) as z:
 for n in sorted(g['files']):z.write(a.root/n,n)
manifest_bytes=(a.corpus/'manifest.json').read_bytes();manifest=json.loads(manifest_bytes)
require(sha(manifest_bytes)==config['manifest_sha256'] and manifest['pilot_commit']==config['pilot_commit'] and len(manifest['libraries'])==94,'Benchmark pin/schema/deadline precondition failed')
selected=[('definition',r) for r in manifest['definitions']]
for uri in case['libraries']:selected.append(('library',next(r for r in manifest['libraries'] if r['source_uri']==uri)))
if a.case=='all94':require(not case['controls'] and set(case['libraries'])=={r['source_uri'] for r in manifest['libraries']} and len(case['libraries'])==94,'all94 case must select the exact original roster')
inputs=[]
with zipfile.ZipFile(a.out/'runtime-inputs.zip','x',compression=zipfile.ZIP_DEFLATED) as z:
 z.writestr('manifest.json',manifest_bytes);z.writestr('bench-cases-v1.json',config_bytes)
 for kind,row in selected:
  source=(a.corpus/row['fixture_path']).resolve();require(source.is_relative_to(a.corpus),'Input resolves outside pinned corpus')
  b=source.read_bytes();require(len(b)==row['bytes'] and sha(b)==row['sha256'],'Pinned original body changed: '+row['source_uri'])
  z.writestr(row['fixture_path'],b);inputs.append({'uri':row['source_uri'],'sha256':sha(b),'bytes':len(b),'kind':kind,'archive_path':row['fixture_path'],'resource_uri':row['resource_uri'],'language':row['language']})
 for row in case['controls']:
  b=row['text'].encode();require(sha(b)==row['sha256'],'Pinned local control changed')
  path='local/'+row['source_uri'];z.writestr(path,b);inputs.append({'uri':row['source_uri'],'sha256':sha(b),'bytes':len(b),'kind':'control','archive_path':path,'resource_uri':row['resource_uri'],'language':row['language']})
require(len(inputs)==len({r['uri'] for r in inputs}),'Benchmark pin/schema/deadline precondition failed')
(a.out/'runtime-inputs.json').write_text(json.dumps(inputs,indent=2)+'\n')
env=os.environ.copy();env.pop('RUST_MIN_STACK',None)
start=time.monotonic();pipeline_elapsed=None;timeout_stage=None;externally_killed=False
with (a.out/'stdout.log').open('wb') as stdout,(a.out/'stderr.log').open('wb') as stderr:
 proc=subprocess.Popen(launch_command(exe,a.case,a.corpus,a.out),cwd=a.root,env=env,stdout=stdout,stderr=stderr)
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
actual=[]
actual_resources={}
if seen:
 actual=seen['data']['definitions']+[r['source'] for r in seen['data']['sources']]
 actual_resources={r['source']['uri']:r['resource_uri'] for r in seen['data']['sources']}
actual_pins={r['uri']:r['sha256'] for r in actual}
expected_pins={r['uri']:r['sha256'] for r in inputs}
expected_resources={r['uri']:r['resource_uri'] for r in inputs if r['kind']!='definition'}
models_match=bool(seen) and len(actual)==len(actual_pins)==len(inputs) and actual_pins==expected_pins and actual_resources==expected_resources
with zipfile.ZipFile(a.out/'compiler-inputs.zip') as z:archive_ok=all(sha(z.read(n))==s for n,s in g['files'].items())
with zipfile.ZipFile(a.out/'runtime-inputs.zip') as z:
 runtime_ok=sha(z.read('manifest.json'))==sha(manifest_bytes) and sha(z.read('bench-cases-v1.json'))==sha(config_bytes) and all(sha(z.read(r['archive_path']))==r['sha256'] for r in inputs)
report=json.loads((a.out/'report.json').read_text()) if (a.out/'report.json').exists() else None
partial=json.loads((a.out/'result-details.json').read_text()) if (a.out/'result-details.json').exists() else None
ok=completed_host(report,a.case)
counts=ok.get('timing',{}).get('pipeline_counts',{}) if ok else {}
expected_counts={'real_libraries':len(case['libraries']),'local_controls':len(case['controls']),'kerml_documents':sum(r['kind']!='definition' and r['language']=='kerml' for r in inputs),'sysml_documents':sum(r['kind']!='definition' and r['language']=='sysml' for r in inputs)}
observations=ok.get('observations',{}) if ok else {}
timing=ok.get('timing',{}) if ok else {}
admitted=bool(observations.get('resource_set_admitted') is True and observations.get('resources')==len(case['libraries'])+len(case['controls']))
phase_complete=timing.get('json_suppressed')==0 and timing.get('observer_suppressed')==0 and timing.get('machine',{}).get('incomplete') is False and all(finite_duration(timing.get('stage_seconds',{}).get(k),MODEL_SECONDS) for k in ['physical_setup_parse_construction','postparse_linking_and_audits','physical_tail_after_link','retention_services_after_physical','resource_set_admission_including_source_checks','selected_checks'])
profile=ok.get('functional_profile',{}) if ok else {}
profile_match=all(profile.get(k)==str(config['profile'][k]) for k in ['request_work','preparation_work']) and profile.get('logical_storage_ceiling')==str(config['profile']['logical_storage_bytes']) and profile.get('resources')==config['profile']['resources']
qualified=bool(code==0 and not timed_out and pipeline_elapsed is not None and ok and admitted and observations.get('constructed_linked') is True and observations.get('selected_checks_passed') is True and observations.get('diagnostics')==0 and ok.get('input_counts')==expected_counts and finite_duration(ok.get('model_seconds'),MODEL_SECONDS) and phase_complete and profile_match and (a.case not in ('tiny','parent-ends') or counts.get('paired_reference_applied',0)>0))
result={'schema':'intersect.native-pilot.bench-receipt.v1','case':a.case,'protocol':{'setup_safety_seconds':SETUP_SECONDS,'model_limit_seconds':MODEL_SECONDS,'process_safety_seconds':PROCESS_SECONDS},
 'cold_process_seconds':elapsed,'first_pipeline_host_elapsed_seconds':pipeline_elapsed,'conservative_model_process_seconds':elapsed-pipeline_elapsed if pipeline_elapsed is not None else None,
 'timed_out':timed_out,'timeout_stage':timeout_stage,'externally_killed':externally_killed,'exit_code':code,'last_observed_phase':phases[-1]['phase'] if phases else None,
 'host_report':report,'partial_details':partial if not ok else None,'last_execution_phase':next((r['phase'] for r in reversed(phases) if r['phase']!='request_terminal'),None),'resource_set_admitted':admitted,'library_count':len(case['libraries']),'control_count':len(case['controls']),'pipeline_counts':counts,
 'source_inputs':len(g['files']),'prelaunch_source_match':True,'qualification_source_basis':'prelaunch checked actual Cargo inputs plus immutable matching compiler ZIP/executable/runtime bodies; later live source is informational','current_source_match':all((a.root/n).is_file() and digest(a.root/n)==s for n,s in g['files'].items()),'source_archive_match':archive_ok,'binary_match':digest(exe)==g['binary_sha256'],
 'binary_sha256':digest(exe),'runtime_input_count':len(inputs),'runtime_archive_match':runtime_ok,'runtime_models_match_actual_host':models_match,'runtime_archive_sha256':digest(a.out/'runtime-inputs.zip'),
 'source_archive_sha256':digest(a.out/'compiler-inputs.zip'),'runner_sha256':digest(Path(__file__)),'config_sha256':sha(config_bytes),'phase_timing_complete':phase_complete,'functional_profile_matches_config':profile_match,'qualified_terminal':qualified,'whole_language_semantics_qualified':False,'wasm_qualified':False}
result['qualified_terminal']=qualified and all(result[k] for k in ['prelaunch_source_match','source_archive_match','binary_match','runtime_archive_match','runtime_models_match_actual_host'])
result['full94_constructed_linked_and_admitted_under60']=a.case=='all94' and result['qualified_terminal']
(a.out/'runner.py').write_bytes(Path(__file__).read_bytes());(a.out/'terminal-postguard.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result),flush=True)
