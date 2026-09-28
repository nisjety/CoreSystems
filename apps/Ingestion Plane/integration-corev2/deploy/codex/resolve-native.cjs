#!/usr/bin/env node
// Prints the real path of the native Codex binary that the npm launcher of the
// pinned @openai/codex release (bin/codex.js) spawns on this machine. It mirrors
// the launcher's findCodexExecutable() for Linux, the only platform the
// integration-api image targets: resolve <platform package>/package.json from
// the launcher's own directory, else fall back to <package>/vendor, then take
// vendor/<target triple>/bin/codex. It exits non-zero when that binary is
// missing or not executable, so the image build fails instead of shipping a
// broken CODEX_APP_SERVER_COMMAND.
//
// Usage: node resolve-native.cjs <path to @openai/codex bin/codex.js>
"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { createRequire } = require("node:module");

// The Linux entries of the launcher's PLATFORM_PACKAGE_BY_TARGET, keyed by
// process.arch as the launcher selects them.
const LINUX_TARGETS = {
  x64: { triple: "x86_64-unknown-linux-musl", platformPackage: "@openai/codex-linux-x64" },
  arm64: { triple: "aarch64-unknown-linux-musl", platformPackage: "@openai/codex-linux-arm64" },
};

function resolveNativeCodex(launcher, platform = process.platform, arch = process.arch) {
  const target = platform === "linux" || platform === "android" ? LINUX_TARGETS[arch] : undefined;
  if (!target) {
    throw new Error(`unsupported Codex platform: ${platform} (${arch})`);
  }
  // Node runs the launcher from its real path, so its require() and
  // __dirname start there.
  const launcherPath = fs.realpathSync(launcher);
  let vendorRoot;
  try {
    const packageJson = createRequire(launcherPath).resolve(`${target.platformPackage}/package.json`);
    vendorRoot = path.join(path.dirname(packageJson), "vendor");
  } catch {
    vendorRoot = path.join(path.dirname(launcherPath), "..", "vendor");
  }
  const binary = path.join(vendorRoot, target.triple, "bin", "codex");
  if (!fs.existsSync(binary)) {
    throw new Error(`missing vendored Codex binary for ${target.triple} (${target.platformPackage})`);
  }
  const resolved = fs.realpathSync(binary);
  fs.accessSync(resolved, fs.constants.X_OK);
  return resolved;
}

module.exports = { resolveNativeCodex };

if (require.main === module) {
  if (process.argv.length !== 3) {
    console.error("usage: resolve-native.cjs <path to @openai/codex bin/codex.js>");
    process.exit(2);
  }
  try {
    process.stdout.write(`${resolveNativeCodex(process.argv[2])}\n`);
  } catch (error) {
    console.error(`resolve-native: ${error.message}`);
    process.exit(1);
  }
}
