import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, chmodSync, writeFileSync, rmSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { provisionAnbudDatabase } from './provision-anbud-database.mjs';

const credentials = {runtime: 'a'.repeat(64), migrator: 'b'.repeat(64)};
test('plan never touches Docker or reads credentials', () => {
  assert.deepEqual(provisionAnbudDatabase({run: () => assert.fail('Docker called')}), {status:'planned'});
});
test('new database creates only dedicated roles and applies existing boundary', () => {
  const calls = [];
  const result = provisionAnbudDatabase({apply:true, credentials, rolesSQL:'-- existing roles policy', run:(...args)=>{
    calls.push(args); return calls.length===1 ? 'new' : '';
  }});
  assert.equal(result.status,'created');
  assert.equal(calls.length,5);
  const sql=calls[1][1];
  assert.ok(!sql.includes(credentials.runtime)&&!sql.includes(credentials.migrator));
  assert.match(sql,/PASSWORD 'SCRAM-SHA-256\$4096:/);
  assert.match(sql,/CREATE DATABASE anbud_core OWNER anbud_migrator/);
  assert.match(sql,/CREATE ROLE anbud_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS/);
  assert.ok(calls[2][1].includes('existing roles policy'));
  for(const [args] of calls){assert.ok(!args.join(' ').includes(credentials.runtime));assert.ok(!args.join(' ').includes(credentials.migrator));}
});
test('existing correctly owned database verifies credentials without rotation',()=>{
  const calls=[];
  assert.equal(provisionAnbudDatabase({apply:true,credentials,rolesSQL:'policy',run:(...args)=>{calls.push(args);return calls.length===1?'ready':'';}}).status,'ready');
  assert.equal(calls.length,3);
  assert.ok(calls.every(([,input])=>!input.includes('CREATE')&&!input.includes('ALTER')));
});
test('conflicting database or partial roles fail before writes',()=>{
  let calls=0;
  assert.throws(()=>provisionAnbudDatabase({apply:true,credentials,rolesSQL:'policy',run:()=>{calls++;return 'conflict';}}),/manual review/);
  assert.equal(calls,1);
});
test('SQL-sensitive or missing credentials are refused before Docker',()=>{
  for(const bad of ['', 'x'.repeat(64),"a'; DROP DATABASE other; --"]){
    assert.throws(()=>provisionAnbudDatabase({apply:true,credentials:{...credentials,runtime:bad},run:()=>assert.fail('Docker called')}),/64 hexadecimal/);
  }
});

test('protected filesystem secrets are read without forwarding cleartext in commands',()=>{
 const fixture=mkdtempSync(join(tmpdir(),'studio-anbud-test-'));
 const directory=join(fixture,'secrets');mkdirSync(directory,{mode:0o700});
 const previous=process.env.STUDIO_LOCAL_SECRETS_DIR;
 process.env.STUDIO_LOCAL_SECRETS_DIR=directory;
 try{
  for(const [name,value]of [['anbud_db_password',credentials.runtime],['anbud_migrator_db_password',credentials.migrator]]){
   writeFileSync(join(directory,name),value,{mode:0o600});
  }
  let calls=0;
  assert.equal(provisionAnbudDatabase({apply:true,run:()=>++calls===1?'ready':''}).status,'ready');
  chmodSync(join(directory,'anbud_db_password'),0o644);
  assert.throws(()=>provisionAnbudDatabase({apply:true,run:()=>assert.fail('Docker called')}),/private regular file/);
  chmodSync(join(directory,'anbud_db_password'),0o600);
  chmodSync(directory,0o755);
  assert.throws(()=>provisionAnbudDatabase({apply:true,run:()=>assert.fail('Docker called')}),/mode 0700/);
  chmodSync(directory,0o700);
  rmSync(join(directory,'anbud_db_password'));
  symlinkSync(join(directory,'anbud_migrator_db_password'),join(directory,'anbud_db_password'));
  assert.throws(()=>provisionAnbudDatabase({apply:true,run:()=>assert.fail('Docker called')}),/private regular file/);
 }finally{if(previous===undefined)delete process.env.STUDIO_LOCAL_SECRETS_DIR;else process.env.STUDIO_LOCAL_SECRETS_DIR=previous;rmSync(fixture,{recursive:true,force:true});}
});

test('CLI defaults to plan and rejects unsupported arguments',()=>{
 const path=fileURLToPath(new URL('./provision-anbud-database.mjs',import.meta.url));
 const plan=spawnSync(process.execPath,[path],{encoding:'utf8'});
 assert.equal(plan.status,0);assert.match(plan.stdout,/Plan:/);
 const invalid=spawnSync(process.execPath,[path,'--rotate'],{encoding:'utf8'});
 assert.equal(invalid.status,1);assert.match(invalid.stderr,/Usage:/);
});
