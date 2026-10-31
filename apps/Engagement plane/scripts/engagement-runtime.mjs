#!/usr/bin/env node
// One local runtime, canonical owner entry points. Never starts Model Plane or
// production frontend services and never removes containers, images or data.
import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const ingestion = resolve(root, '../Ingestion Plane/scripts/run-engagement-ingestion.sh');
const frontend = resolve(root, '../Frontend Plane/verevonv3/scripts/run-frontend-plane.mjs');

export function runtimePlan(command) {
  if (!['up', 'config', 'status'].includes(command)) {
    throw new Error('Choose up, config, status. Destructive cleanup is intentionally not supported.');
  }
  const args = command === 'up' ? ['up', '-d', '--build']
    : command === 'config' ? ['config', '--quiet'] : ['ps'];
  return [
    { owner: 'control', command: 'sh', args: ['scripts/auth-marketing-local.sh', ...args], env: { SHARED_NATS_ENABLED: '1' } },
    { owner: 'frontend', command: 'node', args: [frontend, ...args], env: {} },
    { owner: 'ingestion', command: 'sh', args: [ingestion, ...args], env: {
      INTEGRATION_LOCAL_PAGESPEED: '1', INTEGRATION_LOCAL_CLARITY: '1',
      INTEGRATION_LOCAL_MICROSOFT: '1',
      INTEGRATION_LOCAL_TRENDS: '1',
      INTEGRATION_LOCAL_CLARITY_TRIODELAB: '1', INTEGRATION_LOCAL_META: '1',
      INTEGRATION_LOCAL_WEB_VITALS: '1', INTEGRATION_LOCAL_CRUX: '1',
      INTEGRATION_LOCAL_SITE_SOURCES: '1',
      INTEGRATION_LOCAL_ALGOLIA_OAUTH: '1',
      SHARED_NATS_ENABLED: '1',
    } },
    { owner: 'engagement', command: 'sh', args: ['scripts/engagement-compose.sh', '--profile', 'marketing', ...args], env: {
      STUDIO_ANBUD_ENABLED: '1', STUDIO_PLANE_LINKS: '1', SHARED_NATS_ENABLED: '1',
      STUDIO_WEB_VITALS_ENABLED: '1',
    } },
  ];
}

export function runRuntime(command, run = spawnSync) {
  for (const step of runtimePlan(command)) {
    const result = run(step.command, step.args, {
      cwd: root, env: { ...process.env, ...step.env }, stdio: 'inherit',
    });
    if (result.error || result.status !== 0) {
      // Do not serialize child environments, credentials or raw spawn errors.
      throw new Error(`${step.owner} ${command} failed; subsequent planes were not changed.`);
    }
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    if (process.argv.length !== 3) throw new Error('Choose up, config, status.');
    runRuntime(process.argv[2]);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
