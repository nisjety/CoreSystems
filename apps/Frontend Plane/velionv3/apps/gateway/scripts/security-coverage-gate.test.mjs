import assert from 'node:assert/strict';
import test from 'node:test';

import { evaluateCoverage } from './security-coverage-gate.mjs';

function llvmCoverage(files) {
  return {
    type: 'llvm.coverage.json.export',
    version: '3.0.1',
    data: [{ files }],
  };
}

function coveredFile(filename, covered, count) {
  return {
    filename,
    summary: {
      lines: { covered, count, percent: (covered / count) * 100 },
    },
  };
}

test('accepts every required security module at the line threshold', () => {
  const result = evaluateCoverage({
    report: llvmCoverage([
      coveredFile('/repo/src/middleware.rs', 80, 100),
      coveredFile('/repo/src/audience_tokens.rs', 9, 10),
      coveredFile('/repo/src/domains/orgs/members.rs', 8, 10),
    ]),
    root: '/repo',
    requiredFiles: [
      'src/audience_tokens.rs',
      'src/domains/orgs/members.rs',
      'src/middleware.rs',
    ],
    minimumLines: 80,
  });

  assert.equal(result.ok, true);
  assert.deepEqual(result.measurements, [
    { file: 'src/audience_tokens.rs', covered: 9, count: 10, percent: 90 },
    {
      file: 'src/domains/orgs/members.rs',
      covered: 8,
      count: 10,
      percent: 80,
    },
    { file: 'src/middleware.rs', covered: 80, count: 100, percent: 80 },
  ]);
  assert.deepEqual(result.failures, []);
});

test('fails closed when a module is below threshold', () => {
  const result = evaluateCoverage({
    report: llvmCoverage([coveredFile('/repo/src/middleware.rs', 79, 100)]),
    root: '/repo',
    requiredFiles: ['src/middleware.rs'],
    minimumLines: 80,
  });

  assert.equal(result.ok, false);
  assert.match(result.failures[0], /79\.00%.*80\.00%/);
});

test('fails closed when a required module is missing or duplicated', () => {
  const missing = evaluateCoverage({
    report: llvmCoverage([]),
    root: '/repo',
    requiredFiles: ['src/middleware.rs'],
    minimumLines: 80,
  });
  assert.equal(missing.ok, false);
  assert.match(missing.failures[0], /missing/);

  const duplicated = evaluateCoverage({
    report: llvmCoverage([
      coveredFile('/repo/src/middleware.rs', 9, 10),
      coveredFile('/repo/src/middleware.rs', 9, 10),
    ]),
    root: '/repo',
    requiredFiles: ['src/middleware.rs'],
    minimumLines: 80,
  });
  assert.equal(duplicated.ok, false);
  assert.match(duplicated.failures[0], /2 coverage entries/);
});

test('rejects malformed or empty line summaries instead of treating them as covered', () => {
  const malformed = evaluateCoverage({
    report: llvmCoverage([
      {
        filename: '/repo/src/middleware.rs',
        summary: { lines: { covered: 0, count: 0, percent: 100 } },
      },
    ]),
    root: '/repo',
    requiredFiles: ['src/middleware.rs'],
    minimumLines: 80,
  });

  assert.equal(malformed.ok, false);
  assert.match(malformed.failures[0], /invalid line summary/);
});
