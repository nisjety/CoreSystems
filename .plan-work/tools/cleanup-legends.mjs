import fs from 'node:fs';
const F = process.argv[2];
let t = fs.readFileSync(F, 'utf8');
const R = [
  [/^\| Path legend \(this part\) \| \. \|$/m, '| Path convention | Every product path is written in full, relative to the repo root `C:/dev/CoresSystem/`. |'],
  [/^Paths: \.\s*$/gm, ''],
  [/^Paths: \. /gm, ''],
  [/Path prefixes used in the tables below: \. /g, 'Paths are written in full. '],
  [/Path prefixes used in section 3 \(all relative to the repo root\): \. /g, 'Paths in section 3 are written in full, relative to the repo root. '],
  [/Path prefixes used in WS14 to WS17 \(([^)]*)\): \. /g, 'Paths in WS14 to WS17 are written in full ($1). '],
  [/^Prefixes as in WS14.*?\. (?=Design)/gm, 'Paths are written in full. '],
  [/ ?\(its (?:own )?`Q\/`[^)]*\)/g, ''],
  [/\*\*Path legend\.\*\* \. /g, '**Paths.** Written in full. '],
  [/path legend and tags as in 3\.3 \(, `IC`, `MP`, `FE`, `CP`; /g, 'paths written in full, tags as in 3.3 ('],
  [/Path legend \(adds to the legend in section 1\): , where as in section 1\. /g, 'Paths are written in full. '],
];
let c = 0;
for (const [re, rep] of R) { const b = t; t = t.replace(re, rep); if (t !== b) c++; }
console.log('rules applied', c, 'of', R.length);
fs.writeFileSync(F, t);
