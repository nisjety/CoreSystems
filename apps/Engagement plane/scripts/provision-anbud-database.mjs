#!/usr/bin/env node
// Only the Studio-owned anbud_core database. Never rotates an existing role's
// password. No provider grant, external service or other plane's DB is touched.
import { constants, lstatSync, openSync, readFileSync, closeSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash, createHmac, pbkdf2Sync, randomBytes } from 'node:crypto';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..');
const adminArgs=(database)=>['exec','-i','engagement-postgres','sh','-c',
  'exec psql -X --no-password --username "$POSTGRES_USER" --dbname "$1" --tuples-only --no-align --set ON_ERROR_STOP=1','sh',database];
const inspectSQL=`SELECT CASE
 WHEN NOT EXISTS(SELECT FROM pg_database WHERE datname='anbud_core')
  AND NOT EXISTS(SELECT FROM pg_roles WHERE rolname IN ('anbud_app','anbud_migrator','anbud_dba')) THEN 'new'
 WHEN EXISTS(SELECT FROM pg_database d JOIN pg_roles r ON r.oid=d.datdba WHERE d.datname='anbud_core' AND r.rolname='anbud_migrator')
  AND (SELECT count(*) FROM pg_roles WHERE rolname IN ('anbud_app','anbud_migrator','anbud_dba'))=3 THEN 'ready'
 ELSE 'conflict' END;`;

function runDocker(args,input){
 const result=spawnSync('docker',args,{input,encoding:'utf8',maxBuffer:1024*1024,timeout:30_000});
 if(result.status!==0||result.error)throw new Error('Anbud database operation failed; no credentials were displayed. Review the dedicated database/role configuration.');
 return result.stdout.trim();
}

function privateHex(directory,name){
 const dir=lstatSync(directory);
 if(!dir.isDirectory()||dir.isSymbolicLink()||(dir.mode&0o777)!==0o700||dir.uid!==process.getuid())throw new Error('Local secrets directory must be owned, non-symlink and mode 0700');
 const path=join(directory,name);
 const st=lstatSync(path);
 if(!st.isFile()||st.isSymbolicLink()||![0o400,0o600].includes(st.mode&0o777)||st.uid!==process.getuid()||st.size>128)throw new Error(`${name} must be an owned private regular file`);
 const fd=openSync(path,constants.O_RDONLY|constants.O_NOFOLLOW);
 try{return readFileSync(fd,'utf8').trim();}finally{closeSync(fd);}
}

function verify(run,role,password){
 const args=['exec','-i','engagement-postgres','sh','-c',
 'IFS= read -r PGPASSWORD; export PGPASSWORD; exec psql -X --no-password --host 127.0.0.1 --username "$1" --dbname anbud_core --tuples-only --no-align --set ON_ERROR_STOP=1','sh',role];
 run(args,`${password}\nSELECT current_user;\n`);
}

// PostgreSQL receives only a salted SCRAM verifier, never a cleartext password
// in a SQL statement (including installations with statement logging enabled).
function scramVerifier(password){
 const salt=randomBytes(16);
 const salted=pbkdf2Sync(password,salt,4096,32,'sha256');
 const client=createHmac('sha256',salted).update('Client Key').digest();
 const stored=createHash('sha256').update(client).digest('base64');
 const server=createHmac('sha256',salted).update('Server Key').digest('base64');
 return `SCRAM-SHA-256$4096:${salt.toString('base64')}$${stored}:${server}`;
}

export function provisionAnbudDatabase({apply=false,credentials,rolesSQL,run=runDocker}={}){
 if(!apply)return {status:'planned'};
 const directory=process.env.STUDIO_LOCAL_SECRETS_DIR||join(root,'.local','secrets');
 const values=credentials||{runtime:privateHex(directory,'anbud_db_password'),migrator:privateHex(directory,'anbud_migrator_db_password')};
 if(!/^[a-f0-9]{64}$/.test(values.runtime)||!/^[a-f0-9]{64}$/.test(values.migrator))throw new Error('Anbud database passwords must be 64 hexadecimal characters');
 const state=run(adminArgs('postgres'),inspectSQL).trim();
 if(!['new','ready'].includes(state))throw new Error('Existing Anbud database or partial roles require manual review; nothing was overwritten');
 if(state==='new'){
  run(adminArgs('postgres'),`CREATE ROLE anbud_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${scramVerifier(values.migrator)}';
CREATE ROLE anbud_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION PASSWORD '${scramVerifier(values.runtime)}';
CREATE DATABASE anbud_core OWNER anbud_migrator;`);
  run(adminArgs('anbud_core'),`\\set dbname anbud_core\n${rolesSQL??readFileSync(join(root,'anbud-core','scripts','roles.sql'),'utf8')}`);
 }
 verify(run,'anbud_app',values.runtime);
 verify(run,'anbud_migrator',values.migrator);
 return {status:state==='new'?'created':'ready'};
}

if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 try{
  const args=process.argv.slice(2);
  if(args.length>1||(args.length===1&&args[0]!=='--apply'))throw new Error('Usage: node scripts/provision-anbud-database.mjs [--apply]');
  const result=provisionAnbudDatabase({apply:args[0]==='--apply'});
  console.log(result.status==='planned'?'Plan: create a dedicated anbud_core database and restricted roles, preserving all existing databases and credentials. Use --apply to execute.':`Anbud dedicated database ${result.status}; credentials were not displayed.`);
 }catch(error){console.error(error.message);process.exitCode=1;}
}
