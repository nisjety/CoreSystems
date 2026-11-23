/**
 * Offline reproduction of `convex codegen`'s `_generated/api.d.ts` and `api.js`.
 *
 * The CLI needs a configured deployment, but both files are a pure function
 * of the function-module list: the installed convex package's own template
 * (`apiCodegen` in dist/cjs/cli/codegen_templates/api.js), formatted with the
 * prettier the CLI bundles, exactly as `doApiCodegen` / `writeFormattedFile` do.
 * The module list follows the bundler's `entryPoints` filter.
 *
 * Regenerate the checked-in files with:
 *   node test/helpers/convex-api-codegen.cjs --write
 */
const fs = require('node:fs');
const path = require('node:path');

const PACKAGE_ROOT = path.resolve(__dirname, '..', '..');
const FUNCTIONS_DIR = path.join(PACKAGE_ROOT, 'convex');
const API_DTS = path.join(FUNCTIONS_DIR, '_generated', 'api.d.ts');
const API_JS = path.join(FUNCTIONS_DIR, '_generated', 'api.js');
const ENTRY_POINT_EXTENSIONS = ['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs'];

function convexPackageDir() {
  return path.dirname(require.resolve('convex/package.json', { paths: [PACKAGE_ROOT] }));
}

function walk(directory, out) {
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      // A nested component directory is skipped by the bundler.
      if (fs.existsSync(path.join(full, 'convex.config.ts'))) continue;
      walk(full, out);
    } else {
      out.push(path.relative(FUNCTIONS_DIR, full).split(path.sep).join('/'));
    }
  }
  return out;
}

/** The module paths (relative, forward slashes, sorted) codegen would register. */
function functionModulePaths() {
  return walk(FUNCTIONS_DIR, [])
    .filter((relPath) => ENTRY_POINT_EXTENSIONS.some((ext) => relPath.endsWith(ext)))
    .filter((relPath) => {
      const base = path.posix.basename(relPath);
      if (relPath.startsWith('_generated/')) return false;
      if (base.startsWith('.') || base.startsWith('#')) return false;
      if (base === 'schema.ts' || base === 'schema.js') return false;
      if ((base.match(/\./g) || []).length > 1) return false;
      if (relPath.includes(' ')) return false;
      return true;
    })
    .sort();
}

/** The { DTS, JS } texts the pinned convex codegen would write for this tree. */
async function generateApiFiles() {
  const convexDir = convexPackageDir();
  const { apiCodegen } = require(path.join(convexDir, 'dist', 'cjs', 'cli', 'codegen_templates', 'api.js'));
  const prettier = require(require.resolve('prettier', { paths: [convexDir] }));
  const content = apiCodegen(functionModulePaths(), { useTypeScript: false });
  const format = (text) => prettier.format(text, { parser: 'typescript', pluginSearchDirs: false });
  return { DTS: await format(content.DTS), JS: await format(content.JS) };
}

function convexVersion() {
  return require(path.join(convexPackageDir(), 'package.json')).version;
}

module.exports = { API_DTS, API_JS, convexVersion, functionModulePaths, generateApiFiles };

if (require.main === module && process.argv.includes('--write')) {
  generateApiFiles().then(({ DTS, JS }) => {
    fs.writeFileSync(API_DTS, DTS);
    fs.writeFileSync(API_JS, JS);
    console.log(`wrote convex/_generated/api.d.ts and api.js (convex ${convexVersion()}, ${functionModulePaths().length} modules)`);
  });
}
