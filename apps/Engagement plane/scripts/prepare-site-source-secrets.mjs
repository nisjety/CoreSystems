#!/usr/bin/env node
// Create inactive, protected placeholders only. Never print or replace a key.
import { lstat, open } from 'node:fs/promises';
import { constants } from 'node:fs';
import { basename, dirname, isAbsolute, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const files = Object.freeze([
  ['integration_cookiebot_data_api_key', ''],
  ['integration_algolia_analytics_api_key', ''],
  ['integration_marketing_providers_bindings.json', '{"version":1,"installations":[]}\n'],
]);
const failure = () => new Error('Use a private mode-0700 secrets directory and private regular mode-0600 files.');

async function verify(path, directory) {
  const info = await lstat(path);
  if (info.isSymbolicLink() || (directory ? !info.isDirectory() : !info.isFile()) ||
      (info.mode & 0o777) !== (directory ? 0o700 : 0o600) ||
      (typeof process.getuid === 'function' && info.uid !== process.getuid())) throw failure();
}

export async function prepareSiteSourceSecrets(directory) {
  if (!isAbsolute(directory) || basename(directory) !== 'secrets') throw failure();
  await verify(directory, true);
  for (const [name, initial] of files) {
    const path = resolve(directory, name);
    let file;
    try {
      file = await open(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
      await file.writeFile(initial);
      await file.sync();
    } catch (error) {
      if (error.code !== 'EEXIST') throw failure();
    } finally { await file?.close(); }
    await verify(path, false);
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    if (process.argv.length > 3) throw failure();
    const directory = process.argv[2] ?? resolve(dirname(fileURLToPath(import.meta.url)), '../.local/secrets');
    await prepareSiteSourceSecrets(directory);
    console.log('Protected placeholders prepared; existing keys and bindings were preserved.');
  } catch {
    console.error(failure().message);
    process.exitCode = 1;
  }
}
