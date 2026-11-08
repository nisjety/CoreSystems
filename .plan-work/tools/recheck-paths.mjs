import fs from 'node:fs';
const plan = fs.readFileSync(process.argv[2], 'utf8').split('\n');
const tsv = fs.readFileSync('.plan-work/live-uncommitted.tsv', 'utf8').split('\n').filter(Boolean).map(l => l.split('\t'));
const status = (p) => { for (const [s, q] of tsv) { if (q === p || (q.endsWith('/') && p.startsWith(q))) return s; } return ''; };
const LIVE = 'C:/dev/CoresSystem/';
let bad = [], n = 0;
plan.forEach((line, i) => {
  const re = /`((?:apps|\.github|scripts|docs)\/[^`*{}]+?)(?::\d+(?:-\d+)?)?`/g; let m;
  while ((m = re.exec(line))) {
    const p = m[1]; n++;
    const exists = fs.existsSync(LIVE + p);
    const after = line.slice(m.index + m[0].length, m.index + m[0].length + 14);
    const tag = (after.match(/^\s*\[(C|M|U|NEW)\]/) || [])[1];
    if (!exists && !/NEW/.test(line) && !/UNVERIFIED|absent|not exist|missing|MISSING|does not exist/i.test(line)) bad.push([i + 1, 'MISSING-untagged', p]);
    if (exists && tag === 'NEW') bad.push([i + 1, 'NEW-collision', p]);
    if (exists && tag) { const s = status(p); const want = s === '??' ? 'U' : s ? 'M' : 'C'; if (!s && (tag === 'M' || tag === 'U')) bad.push([i + 1, `tag ${tag} but unchanged/committed`, p]); if (s === '??' && tag !== 'U') bad.push([i + 1, `tag ${tag} but untracked`, p]); if (s && s !== '??' && tag === 'C') bad.push([i + 1, 'tag C but modified', p]); }
  }
});
console.log('path tokens', n, 'flags', bad.length);
for (const b of bad.slice(0, 80)) console.log(b.join(' | '));
