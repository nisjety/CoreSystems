import assert from 'node:assert/strict';
import test from 'node:test';

import {
  assertBaselineExcludesChangedFiles,
  buildEslintArgs,
  changedTypescriptFiles,
  compareLintDebt,
  summarizeSuppressions,
} from './lint-ratchet.mjs';

test('full lint uses explicit roots, native suppressions, and never fixes files', () => {
  const args = buildEslintArgs({
    paths: ['src', 'test'],
    suppressionsPath: 'eslint-suppressions.json',
  });

  assert.deepEqual(args, [
    'src',
    'test',
    '--ext',
    '.ts',
    '--concurrency',
    'off',
    '--max-warnings',
    '0',
    '--suppressions-location',
    'eslint-suppressions.json',
  ]);
  assert.equal(args.includes('--fix'), false);
});

test('changed lint uses only changed TypeScript files and an empty baseline', () => {
  const files = changedTypescriptFiles({
    tracked: 'src/z.ts\nsrc/a.ts\nREADME.md\nsrc/a.ts\n',
    untracked: 'test/new.spec.ts\nscripts/helper.mjs\n',
  });

  assert.deepEqual(files, ['src/a.ts', 'src/z.ts', 'test/new.spec.ts']);
  assert.deepEqual(
    buildEslintArgs({ paths: files, suppressionsPath: '.eslint-clean-suppressions.json' }),
    [
      'src/a.ts',
      'src/z.ts',
      'test/new.spec.ts',
      '--concurrency',
      'off',
      '--max-warnings',
      '0',
      '--suppressions-location',
      '.eslint-clean-suppressions.json',
    ],
  );
});

test('legacy baseline rejects entries for changed files', () => {
  const baseline = {
    'src/legacy.ts': { '@typescript-eslint/no-unsafe-call': { count: 2 } },
    'src/changed.ts': { '@typescript-eslint/no-explicit-any': { count: 1 } },
  };

  assert.throws(
    () => assertBaselineExcludesChangedFiles(baseline, ['src/changed.ts']),
    /src\/changed\.ts/,
  );
});

test('legacy debt report is stable and sorted', () => {
  const baseline = {
    'src/z.ts': { zeta: { count: 2 }, alpha: { count: 1 } },
    'src/a.ts': { beta: { count: 3 } },
  };

  assert.deepEqual(summarizeSuppressions(baseline), {
    files: 2,
    rules: 3,
    violations: 6,
    lines: [
      'src/a.ts | beta | 3',
      'src/z.ts | alpha | 1',
      'src/z.ts | zeta | 2',
    ],
  });
});

test('debt comparison fails on new, removed, changed-file, and fatal violations', () => {
  const baseline = {
    'src/legacy.ts': { legacy_rule: { count: 2 } },
  };
  const results = [
    {
      filePath: '/repo/src/legacy.ts',
      messages: [{ ruleId: 'legacy_rule' }],
    },
    {
      filePath: '/repo/src/changed.ts',
      messages: [{ ruleId: 'new_rule' }],
    },
    {
      filePath: '/repo/src/fatal.ts',
      messages: [{ ruleId: null, message: 'parser failed', fatal: true }],
    },
  ];

  const comparison = compareLintDebt({
    baseline,
    results,
    projectRoot: '/repo',
    changedFiles: ['src/changed.ts'],
  });

  assert.equal(comparison.ok, false);
  assert.deepEqual(comparison.failures, [
    'src/changed.ts: new_rule has 1 violation(s) but is not baselined',
    'src/fatal.ts: parser failed',
    'src/legacy.ts: legacy_rule has 1 violation(s); baseline requires exactly 2',
  ]);
});

test('debt comparison accepts an exact legacy count and clean changed files', () => {
  const comparison = compareLintDebt({
    baseline: {
      'src/legacy.ts': { legacy_rule: { count: 2 } },
    },
    results: [
      {
        filePath: '/repo/src/legacy.ts',
        messages: [{ ruleId: 'legacy_rule' }, { ruleId: 'legacy_rule' }],
      },
      { filePath: '/repo/src/changed.ts', messages: [] },
    ],
    projectRoot: '/repo',
    changedFiles: ['src/changed.ts'],
  });

  assert.deepEqual(comparison, { ok: true, failures: [] });
});
