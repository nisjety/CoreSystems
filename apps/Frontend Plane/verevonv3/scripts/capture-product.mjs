import { existsSync, mkdirSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { execFileSync, spawn } from 'node:child_process';
import { productCaptureSettings } from '../tests/e2e/product-capture-policy.ts';

const frontend = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repository = resolve(frontend, '../../..');
const options = {};
for (let index = 2; index < process.argv.length; index += 2) {
  const key = process.argv[index];
  if (!['--mode','--output','--scenario'].includes(key) || !process.argv[index+1] || options[key]) throw new Error('Use --mode rehearsal|release --output ABSOLUTE_NEW_DIRECTORY [--scenario 01-kundesvar]');
  options[key] = process.argv[index+1];
}
const scenario = options['--scenario'] ?? '01-kundesvar';
if (!['01-kundesvar','02-salgsrapport','03-kampanje','04-prosjektplan'].includes(scenario)) throw new Error('Unknown recording scenario');
if (!options['--mode']) throw new Error('Select rehearsal or release explicitly');
const manifest = JSON.parse(readFileSync(join(frontend,'apps/verevon-web/plans/product-recordings/manifest.json'),'utf8'));
const capture = productCaptureSettings(options['--mode'], manifest, options['--output'], repository);
// Resolve existing ancestors as well, so a symlink cannot redirect a private
// capture into the repository. Never let Playwright clean an existing folder.
let ancestor = capture.outputDir;
const remainder = [];
while (!existsSync(ancestor)) {
  const parent = dirname(ancestor);
  if (parent === ancestor) throw new Error('Recording output has no accessible filesystem root');
  remainder.unshift(basename(ancestor)); ancestor = parent;
}
productCaptureSettings(capture.mode, manifest, join(realpathSync(ancestor), ...remainder), realpathSync(repository));
for (const path of [capture.outputDir, capture.outputDir+'.json', capture.outputDir+'.run.json']) {
  if (existsSync(path)) throw new Error('Use a new evidence directory; failed and earlier recordings must be preserved');
}
const components = ['model-plane-model-gateway-1','model-plane-inference-core-1','integration-api','frontend-plane-verevonv3-frontend-1'];
const runtime = components.map(name => {
  const values = execFileSync('docker', ['inspect','--format','{{.Image}} {{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}',name], {encoding:'utf8'}).trim().split(/\s+/);
  if (values[1] !== 'running' || values[2] !== 'healthy') throw new Error(`Recording preflight: ${name} is not healthy`);
  return {name,image:values[0],status:values[1],health:values[2]};
});
const require = createRequire(join(frontend,'package.json'));
const packagePath = require.resolve('@playwright/test/package.json');
const cli = join(dirname(packagePath), JSON.parse(readFileSync(packagePath,'utf8')).bin.playwright);
const customer = scenario === '01-kundesvar';
const args = [cli,'test',customer ? 'tests/e2e/product-recording.spec.ts' : 'tests/e2e/product-scenarios.spec.ts',
  '--project='+ (customer ? 'product-recording' : 'product-readiness'), '--no-deps', '--workers=1','--retries=0','--reporter=list,json'];
if (!customer) args.push('--grep', `Q05 ${scenario}`);
mkdirSync(dirname(capture.outputDir),{recursive:true});
const record = {schemaVersion:1,scenario,mode:capture.mode,publicationApproved:false,startedAt:new Date().toISOString(),
  model:'gpt-5.6-terra',provider:'openai-codex-subscription',runtime,output: capture.outputDir};
writeFileSync(capture.outputDir+'.run.json',JSON.stringify(record,null,2));
console.log(`Capturing ${scenario} (${capture.mode}); real Terra subscription, no automatic retry. Evidence: ${capture.outputDir}`);
const env = {...process.env, PRODUCT_RECORDING_MODE:capture.mode, PRODUCT_RECORDING_OUTPUT:capture.outputDir,
  PRODUCT_RECORDING_BUILD:runtime[0].image, PLAYWRIGHT_JSON_OUTPUT_FILE:capture.outputDir+'.json'};
delete env.PRODUCT_RECORDING_CAPTURE;
const child = spawn(process.execPath,args,{cwd:frontend,env,stdio:'inherit'});
const code = await new Promise((resolveCode,reject) => {child.on('error',reject);child.on('exit',value=>resolveCode(value ?? 1));});
writeFileSync(capture.outputDir+'.run.json',JSON.stringify({...record,finishedAt:new Date().toISOString(),exitCode:code},null,2));
process.exitCode = code;
