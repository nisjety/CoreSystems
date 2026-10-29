import { readFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export function evaluateCoverage({ report, root, requiredFiles, minimumLines }) {
  const files = report?.data?.[0]?.files;
  if (!Array.isArray(files)) {
    return {
      ok: false,
      measurements: [],
      failures: ['coverage report does not contain data[0].files'],
    };
  }

  const normalized = new Map();
  for (const file of files) {
    if (typeof file?.filename !== 'string') {
      continue;
    }
    const relative = path.relative(root, file.filename).split(path.sep).join('/');
    const entries = normalized.get(relative) ?? [];
    normalized.set(relative, [...entries, file]);
  }

  const measurements = [];
  const failures = [];
  for (const requiredFile of [...requiredFiles].sort()) {
    const entries = normalized.get(requiredFile) ?? [];
    if (entries.length === 0) {
      failures.push(`${requiredFile}: missing from coverage report`);
      continue;
    }
    if (entries.length !== 1) {
      failures.push(`${requiredFile}: found ${entries.length} coverage entries`);
      continue;
    }

    const { covered, count } = entries[0]?.summary?.lines ?? {};
    if (
      !Number.isFinite(covered) ||
      !Number.isFinite(count) ||
      covered < 0 ||
      count <= 0 ||
      covered > count
    ) {
      failures.push(`${requiredFile}: invalid line summary`);
      continue;
    }
    const percent = (covered / count) * 100;
    measurements.push({ file: requiredFile, covered, count, percent });
    if (percent < minimumLines) {
      failures.push(
        `${requiredFile}: ${percent.toFixed(2)}% line coverage is below ${minimumLines.toFixed(2)}%`,
      );
    }
  }

  return { ok: failures.length === 0, measurements, failures };
}

function main() {
  const [reportPath, root, minimumRaw, ...requiredFiles] = process.argv.slice(2);
  const minimumLines = Number(minimumRaw);
  if (
    !reportPath ||
    !root ||
    !Number.isFinite(minimumLines) ||
    minimumLines < 0 ||
    minimumLines > 100 ||
    requiredFiles.length === 0
  ) {
    console.error(
      'usage: node security-coverage-gate.mjs <report.json> <root> <minimum-lines> <required-file>...',
    );
    return 2;
  }

  const result = evaluateCoverage({
    report: JSON.parse(readFileSync(reportPath, 'utf8')),
    root: path.resolve(root),
    requiredFiles,
    minimumLines,
  });
  for (const measurement of result.measurements) {
    console.log(
      `${measurement.file}: ${measurement.percent.toFixed(2)}% lines (${measurement.covered}/${measurement.count})`,
    );
  }
  for (const failure of result.failures) {
    console.error(`coverage gate: ${failure}`);
  }
  return result.ok ? 0 : 1;
}

if (process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url) {
  try {
    process.exitCode = main();
  } catch (error) {
    console.error(error instanceof Error ? error.message : error);
    process.exitCode = 1;
  }
}
