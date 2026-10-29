import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const scriptsDirectory = path.dirname(fileURLToPath(import.meta.url));
const gatewayDirectory = path.resolve(scriptsDirectory, '..');
const verevonDirectory = path.resolve(gatewayDirectory, '..', '..');

test('the composer does not offer an unimplemented generic projects endpoint', () => {
  const composer = fs.readFileSync(
    path.join(verevonDirectory, 'src/features/dashboard/home/DashboardComposer.tsx'),
    'utf8',
  );

  assert.doesNotMatch(composer, /\/api\/v1\/projects/);
});

test("Studio's RAM-only repository is exposed as explicitly ephemeral", () => {
  const studioGateway = fs.readFileSync(
    path.join(gatewayDirectory, 'src/domains/studio.rs'),
    'utf8',
  );
  const studioPage = fs.readFileSync(
    path.join(verevonDirectory, 'src/features/studio/components/StudioPage.tsx'),
    'utf8',
  );

  assert.match(studioGateway, /const STUDIO_PERSISTENCE:\s*&str\s*=\s*"ephemeral"/);
  assert.match(studioGateway, /persistence:\s*STUDIO_PERSISTENCE/);
  assert.match(studioPage, /Midlertidig Studio-prosjekt/);
});
