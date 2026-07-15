import { spawnSync } from 'node:child_process';
import { readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const projectRoot = path.resolve(fileURLToPath(new URL('..', import.meta.url)));

export function changedTypescriptFiles({ tracked, untracked }) {
  return [...new Set(`${tracked}\n${untracked}`.split(/\r?\n/u))]
    .map((file) => file.trim())
    .filter((file) => file.endsWith('.ts'))
    .sort();
}

export function buildEslintArgs({ paths, suppressionsPath }) {
  const args = [...paths];
  if (paths.some((entry) => !entry.endsWith('.ts'))) {
    args.push('--ext', '.ts');
  }
  args.push(
    '--concurrency',
    'off',
    '--max-warnings',
    '0',
    '--suppressions-location',
    suppressionsPath,
  );
  return args;
}

export function assertBaselineExcludesChangedFiles(baseline, changedFiles) {
  const changedEntries = changedFiles.filter((file) => baseline[file]);
  if (changedEntries.length > 0) {
    throw new Error(
      `Legacy lint baseline contains changed files: ${changedEntries.join(', ')}`,
    );
  }
}

export function summarizeSuppressions(baseline) {
  const lines = [];
  let violations = 0;
  let rules = 0;

  for (const file of Object.keys(baseline).sort()) {
    for (const rule of Object.keys(baseline[file]).sort()) {
      const count = baseline[file][rule]?.count;
      if (!Number.isInteger(count) || count < 1) {
        throw new Error(`Invalid suppression count for ${file} / ${rule}`);
      }
      rules += 1;
      violations += count;
      lines.push(`${file} | ${rule} | ${count}`);
    }
  }

  return {
    files: Object.keys(baseline).length,
    rules,
    violations,
    lines,
  };
}

export function compareLintDebt({
  baseline,
  results,
  projectRoot,
  changedFiles,
}) {
  assertBaselineExcludesChangedFiles(baseline, changedFiles);
  const actual = {};
  const failures = [];

  for (const result of results) {
    const file = path.relative(projectRoot, result.filePath).split(path.sep).join('/');
    for (const message of result.messages) {
      if (!message.ruleId) {
        failures.push(`${file}: ${message.message}`);
        continue;
      }
      actual[file] ??= {};
      actual[file][message.ruleId] = (actual[file][message.ruleId] ?? 0) + 1;
    }
  }

  for (const file of Object.keys(actual).sort()) {
    for (const rule of Object.keys(actual[file]).sort()) {
      const count = actual[file][rule];
      if (!baseline[file]?.[rule]) {
        failures.push(
          `${file}: ${rule} has ${count} violation(s) but is not baselined`,
        );
      }
    }
  }

  for (const file of Object.keys(baseline).sort()) {
    for (const rule of Object.keys(baseline[file]).sort()) {
      const expected = baseline[file][rule].count;
      const count = actual[file]?.[rule] ?? 0;
      if (count !== expected) {
        failures.push(
          `${file}: ${rule} has ${count} violation(s); baseline requires exactly ${expected}`,
        );
      }
    }
  }

  failures.sort();
  return { ok: failures.length === 0, failures };
}

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: projectRoot,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  if (result.error) {
    throw result.error;
  }
  return result;
}

function gitOutput(args) {
  const result = run('git', args);
  if (result.status !== 0) {
    throw new Error(result.stderr.trim() || `git ${args.join(' ')} failed`);
  }
  return result.stdout;
}

function listTypescriptFiles() {
  const files = [];
  const visit = (relativeDirectory) => {
    const absoluteDirectory = path.join(projectRoot, relativeDirectory);
    for (const entry of readdirSync(absoluteDirectory, { withFileTypes: true })) {
      const relativePath = path.join(relativeDirectory, entry.name);
      if (entry.isDirectory()) {
        visit(relativePath);
      } else if (entry.isFile() && entry.name.endsWith('.ts')) {
        files.push(relativePath.split(path.sep).join('/'));
      }
    }
  };
  visit('src');
  visit('test');
  return files.sort();
}

function runEslintBatch(paths) {
  if (paths.length === 0) {
    return [];
  }
  const eslintBin = path.join(projectRoot, 'node_modules/eslint/bin/eslint.js');
  const result = spawnSync(
    process.execPath,
    [
      '--max-old-space-size=8192',
      eslintBin,
      ...buildEslintArgs({
        paths,
        suppressionsPath: '.eslint-clean-suppressions.json',
      }),
      '--format',
      'json',
    ],
    { cwd: projectRoot, encoding: 'utf8' },
  );
  if (result.error) {
    throw result.error;
  }
  if (result.signal) {
    throw new Error(`ESLint batch terminated by ${result.signal}`);
  }
  if ((result.status ?? 2) > 1) {
    throw new Error(result.stderr.trim() || `ESLint exited ${result.status}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch {
    throw new Error(`ESLint returned invalid JSON: ${result.stdout.slice(0, 500)}`);
  }
}

function runEslintBatches(files, batchSize = 20) {
  const results = [];
  for (let offset = 0; offset < files.length; offset += batchSize) {
    const batch = files.slice(offset, offset + batchSize);
    console.log(
      `Lint batch ${Math.floor(offset / batchSize) + 1}/${Math.ceil(files.length / batchSize)} (${batch.length} files)`,
    );
    results.push(...runEslintBatch(batch));
  }
  return results;
}

function main() {
  const baselinePath = path.join(projectRoot, 'eslint-suppressions.json');
  const baseline = JSON.parse(readFileSync(baselinePath, 'utf8'));
  const changedFiles = changedTypescriptFiles({
    tracked: gitOutput([
      'diff',
      '--relative',
      '--name-only',
      '--diff-filter=ACMR',
      'HEAD',
      '--',
      '*.ts',
    ]),
    untracked: gitOutput([
      'ls-files',
      '--others',
      '--exclude-standard',
      '--',
      '*.ts',
    ]),
  });

  assertBaselineExcludesChangedFiles(baseline, changedFiles);
  const summary = summarizeSuppressions(baseline);
  console.log(
    `Legacy lint debt: ${summary.violations} violation(s), ${summary.rules} rule/file pair(s), ${summary.files} file(s)`,
  );
  for (const line of summary.lines) {
    console.log(`  ${line}`);
  }

  const files = listTypescriptFiles();
  const intendedFiles = new Set(files);
  const intendedChangedFiles = changedFiles.filter((file) => intendedFiles.has(file));
  console.log(`TypeScript files enumerated: ${files.length}`);
  const results = runEslintBatches(files);
  const comparison = compareLintDebt({
    baseline,
    results,
    projectRoot,
    changedFiles: intendedChangedFiles,
  });
  for (const failure of comparison.failures) {
    console.error(`lint ratchet: ${failure}`);
  }
  console.log(
    `Changed TypeScript files checked without suppressions: ${intendedChangedFiles.length}`,
  );
  return comparison.ok ? 0 : 1;
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
