import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import assert from 'node:assert/strict';
const [artifact, sourceRoot, output] = process.argv.slice(2);
fs.mkdirSync(output, {recursive:false});
const pin = file => {const bytes=fs.readFileSync(file);return {path:file,bytes:bytes.length,sha256:crypto.createHash('sha256').update(bytes).digest('hex')};};
const write = (file, object) => fs.writeFileSync(path.join(output,file),JSON.stringify(object,null,2)+'\n',{flag:'wx'});
const before=pin(artifact);
const sourcesManifest=path.join(sourceRoot,'fixtures/kerml-draft-pinned/sources.json');
const manifest=JSON.parse(fs.readFileSync(sourcesManifest,'utf8'));
const sources=manifest.sources.map(row=>{
 const file=path.join(sourceRoot,'fixtures/kerml-draft-pinned',row.file);
 const actual=pin(file);assert.equal(actual.sha256,row.sha256);assert.equal(actual.bytes,row.bytes);
 return {id:{uri:row.original_uri,sha256:row.sha256},kind:row.file.endsWith('.ecore')?'ecore':'xtext',text:fs.readFileSync(file,'utf8'),dependencies:[]};
});
sources[1].dependencies=[sources[0].id];sources[2].dependencies=[sources[0].id,sources[1].id];
const text='rcore 1; package emptyRules version "1";';
const behavior={id:{uri:'probe-behavior.rcore',sha256:crypto.createHash('sha256').update(text).digest('hex')},kind:'rcore_behavior',text,dependencies:[sources[0].id]};
assert.equal(behavior.id.sha256,'5eaff959a081947c82b71b13afd07e0547e2acb92cbc3bf98efba24511760c8c');
sources.push(behavior);
const uri='memory://intersect/native-minimal-package/package.kerml';
const request={method:'draft/kerml/analyze',original:{ecore:sources[0].id,materialized_types:[],xtext:sources[2].id,rcore_behavior:behavior.id,namespace:'https://www.omg.org/spec/SysML/20250201',bundle:{version:1,sources,namespaces:[]}},source:{uri,text:'package P {}'}};
const encoded=Buffer.from(JSON.stringify(request));assert.ok(encoded.length<=4000000);
fs.writeFileSync(path.join(output,'request.json'),encoded,{flag:'wx'});
const report={state:'running',artifact:before,request:pin(path.join(output,'request.json')),gate_closures:[],scope:'Normal release JSON/WASM ABI measurement in Node; no UI, browser lifecycle, total-work/memory or workflow award'};
try {
 const started=performance.now();const module=await WebAssembly.compile(fs.readFileSync(artifact));
 report.compile_ms=performance.now()-started;report.imports=WebAssembly.Module.imports(module);assert.equal(report.imports.length,0);
 const instance=await WebAssembly.instantiate(module,{});const api=instance.exports;
 const pointer=api.input_buffer(encoded.length);assert.notEqual(pointer,0);
 new Uint8Array(api.memory.buffer,pointer,encoded.length).set(encoded);
 report.memory_pages_before=api.memory.buffer.byteLength/65536;
 const start=performance.now();const length=api.execute();report.execute_ms=performance.now()-start;
 report.memory_pages_after=api.memory.buffer.byteLength/65536;assert.ok(length>0&&length<=3000000);
 const response=Buffer.from(new Uint8Array(api.memory.buffer,api.output_buffer(),length));
 fs.writeFileSync(path.join(output,'response.json'),response,{flag:'wx'});report.response=pin(path.join(output,'response.json'));
 const result=JSON.parse(response.toString('utf8'));
 assert.equal(result.kind,'frozen_kerml_load');assert.equal(result.linked,false);assert.equal(result.language_validated,false);assert.equal(result.publication_permitted,false);assert.equal(result.package,undefined);
 assert.deepEqual(result.diagnostics,[]);
 assert.deepEqual(result.model.elements.map(o=>o.class).sort(),['Namespace','OwningMembership','Package']);
 assert.equal(result.model.elements.find(o=>o.class==='Package').values.declaredName,'P');
 report.name='P';report.proposed_response_cap_pass=response.length<=3000000;report.proposed_watchdog_measurement_pass=report.execute_ms<=120000;
 assert.ok(report.proposed_watchdog_measurement_pass,'Normal call exceeds proposed120s host watchdog; review policy');
 report.state='pass_normal_release_json_abi_measurement_only';
} catch(error){report.state='fail_preserved';report.failure=error.stack;}
finally {assert.deepEqual(pin(artifact),before);report.artifact_postguard=true;write('terminal.json',report);}
console.log(JSON.stringify({state:report.state,execute_ms:report.execute_ms,response:report.response}));
if(report.state==='fail_preserved')process.exitCode=1;
