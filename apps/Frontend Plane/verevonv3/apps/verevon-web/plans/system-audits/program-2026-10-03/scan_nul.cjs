// Scans every uncommitted file (git status, untracked included) of the real checkout for NUL bytes, a missing final newline on
// source files, and zero-length files, which are the signs of an interrupted or disk-full write. Prints findings only.
// usage: node scan_nul.cjs
const fs = require('fs')
const cp = require('child_process')
const ROOT = 'C:/dev/CoresSystem'
const out = cp.execFileSync('git', ['status', '--short', '-uall', '-z'], { cwd: ROOT, maxBuffer: 1 << 28 }).toString('utf8')
const entries = out.split('\0').filter(Boolean)
const files = []
for (let i = 0; i < entries.length; i++) {
  const e = entries[i]
  const code = e.slice(0, 2)
  const path = e.slice(3)
  if (code[0] === 'R' || code[0] === 'C') i++ // the next entry is the original path
  if (/^ ?D|^D/.test(code)) continue
  files.push(path)
}
let nul = 0, empty = 0, nofinal = 0, scanned = 0
const SRC = /\.(rs|go|ts|tsx|mjs|cjs|js|json|md|sql|toml|yml|yaml|sh|proto|css|html)$/
for (const f of files) {
  let b
  try { b = fs.readFileSync(ROOT + '/' + f) } catch (e) { continue }
  scanned++
  if (b.includes(0)) { nul++; console.log('NUL      ', f) }
  else if (b.length === 0) { empty++; console.log('EMPTY    ', f) }
  else if (SRC.test(f) && b[b.length - 1] !== 0x0a && !/\.sha256$/.test(f)) { nofinal++; console.log('NO-FINAL-NEWLINE', f, '(' + b.length + ' bytes)') }
}
console.log('scanned', scanned, 'files | NUL:', nul, '| empty:', empty, '| no final newline:', nofinal)
