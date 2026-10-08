import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, describe, expect, it } from 'vitest';
import { type Surface, trustClaims } from './trust-claims';
import { certifications, liveControls, subprocessors } from './trust-data';
import { namedDespiteExclusion, recipientsNotListed } from './trust-recipients';

/**
 * Drift tests for the Trust Center and the pages that repeat its claims.
 *
 * What they pin, and to what:
 *
 *  - trust-claims.ts is the machine-readable list of the evidence-backed claims (each tied to a row of
 *    docs/compliance/evidence/claims-vs-code.md). A narrowed sentence that is reworded away, a contradicted
 *    phrase that comes back, and an evidence anchor that disappears from the code each fail here.
 *  - The sub-processor table must account for every vendor the egress scan finds and every vendor the
 *    register (docs/compliance/evidence/subprocessors-from-code.md) proposes to list.
 *  - Levels never go up silently: a control or row cannot become "live" without changing this file.
 *
 * Needs `git` and `node` on the path and reads the repository's docs/compliance/evidence directory. Set
 * TRUST_REPO_ROOT to point at a checkout from elsewhere. Run from the repository root:
 *   pnpm --dir "apps/Frontend Plane/verevonv3" exec vitest run apps/verevon-web/src/components/trust
 */

/** The site files whose prose the claims are pinned to, relative to apps/verevon-web/src. */
const SITE_FILES: Record<string, string> = {
	'TrustCenter.tsx': 'components/trust/TrustCenter.tsx',
	'page.tsx': 'app/trust/page.tsx',
	'Footer.tsx': 'components/core/footer/Footer.tsx',
	'TechnologySection.tsx': 'components/home/extras/TechnologySection.tsx',
	'TrustBandSection.tsx': 'components/home/extras/TrustBandSection.tsx',
	'trust-scroll.tsx': 'components/home/extras/trust-scroll.tsx',
	'PromptSection.tsx': 'components/home/extras/PromptSection.tsx',
};

const PENDING_DOC = 'docs/compliance/evidence/trust-claims-pending-reverification.md';

/** The controls and rows that may carry the "live" level. Anything else must stay "progress" or "planned". */
const LIVE_CONTROLS = ['Godkjenning per handling', 'Norskforankret grunnlag', 'SSO, 2FA & passkey'];
const LIVE_CERTIFICATIONS = ['Offentlig Trust Center'];

const here = dirname(fileURLToPath(import.meta.url));
const siteSrc = resolve(here, '..', '..');

function repoRoot(): string {
	const fromEnv = process.env.TRUST_REPO_ROOT;
	if (fromEnv) return resolve(fromEnv);
	let dir = here;
	for (;;) {
		if (existsSync(join(dir, 'docs', 'compliance', 'evidence', 'tools', 'scan-egress.mjs'))) return dir;
		const parent = dirname(dir);
		if (parent === dir) throw new Error('repository root not found; set TRUST_REPO_ROOT');
		dir = parent;
	}
}

/** Soft hyphens removed, non-breaking spaces and whitespace runs collapsed: JSX line breaks do not matter. */
function normalise(text: string): string {
	return text
		.replace(/­/g, '')
		.replace(/&nbsp;/g, ' ')
		.replace(/ /g, ' ')
		.replace(/\s+/g, ' ')
		.trim();
}

function readSite(name: string): string {
	const relative = SITE_FILES[name];
	if (!relative) throw new Error(`unknown site file "${name}"; add it to SITE_FILES`);
	return normalise(readFileSync(join(siteSrc, relative), 'utf8'));
}

const textOf = (...values: string[]) => normalise(values.join(' '));

function surfaceText(surface: Surface): string {
	const separator = surface.indexOf(':');
	const kind = surface.slice(0, separator);
	const key = surface.slice(separator + 1);
	if (kind === 'file') return readSite(key);
	if (kind === 'control') {
		const control = liveControls.find((entry) => entry.title === key);
		if (!control) throw new Error(`no control titled "${key}"`);
		return textOf(control.title, control.body);
	}
	if (kind === 'subprocessor') {
		const row = subprocessors.find((entry) => entry.name === key);
		if (!row) throw new Error(`no sub-processor row named "${key}"`);
		return textOf(row.name, row.purpose, row.region, row.note);
	}
	if (kind === 'certification') {
		const item = certifications.find((entry) => entry.name === key);
		if (!item) throw new Error(`no certification named "${key}"`);
		return textOf(item.name, item.plan);
	}
	throw new Error(`unknown surface "${surface}"`);
}

/** Every piece of public wording the claims cover, lower-cased for phrase checks. */
function allWording(): string {
	const data = textOf(
		...liveControls.flatMap((entry) => [entry.title, entry.body]),
		...certifications.flatMap((entry) => [entry.name, entry.plan]),
		...subprocessors.flatMap((entry) => [entry.name, entry.purpose, entry.region, entry.note]),
	);
	return [data, ...Object.keys(SITE_FILES).map(readSite)].join(' ').toLowerCase();
}

function splitRow(line: string): string[] {
	let text = line.trim();
	if (text.startsWith('|')) text = text.slice(1);
	if (text.endsWith('|') && !text.endsWith('\\|')) text = text.slice(0, -1);
	const cells: string[] = [];
	let current = '';
	for (let index = 0; index < text.length; index += 1) {
		const char = text[index];
		if (char === '\\' && text[index + 1] === '|') {
			current += '|';
			index += 1;
		} else if (char === '|') {
			cells.push(current);
			current = '';
		} else {
			current += char;
		}
	}
	cells.push(current);
	return cells.map((cell) => cell.trim());
}

function scannedVendorIds(root: string): string[] {
	const script = join(root, 'docs', 'compliance', 'evidence', 'tools', 'scan-egress.mjs');
	const output = execFileSync(process.execPath, [script, '--json'], {
		cwd: root,
		encoding: 'utf8',
		maxBuffer: 256 * 1024 * 1024,
	});
	const parsed = JSON.parse(output) as { vendors: { id: string }[] };
	return parsed.vendors.map((vendor) => vendor.id).sort();
}

function registerDispositions(root: string): Map<string, 'LISTED' | 'EXCLUDED'> {
	const text = readFileSync(join(root, 'docs', 'compliance', 'evidence', 'subprocessors-from-code.md'), 'utf8');
	const found = new Map<string, 'LISTED' | 'EXCLUDED'>();
	for (const raw of text.split('\n')) {
		const line = raw.replace(/\r$/, '');
		if (!line.startsWith('| `')) continue;
		const cells = splitRow(line);
		if (cells.length !== 8) continue;
		const id = cells[0].replace(/`/g, '');
		const disposition = cells[6];
		if (disposition === 'LISTED') found.set(id, 'LISTED');
		else if (disposition.startsWith('EXCLUDED:')) found.set(id, 'EXCLUDED');
	}
	return found;
}

describe('trust claims: the wording the evidence supports', () => {
	const root = repoRoot();
	const claimsDoc = readFileSync(join(root, 'docs', 'compliance', 'evidence', 'claims-vs-code.md'), 'utf8');
	const pendingDoc = existsSync(join(root, PENDING_DOC)) ? normalise(readFileSync(join(root, PENDING_DOC), 'utf8')) : '';

	it('has unique claim ids', () => {
		const ids = trustClaims.map((claim) => claim.id);
		expect(ids.filter((id, index) => ids.indexOf(id) !== index)).toEqual([]);
	});

	describe.each(trustClaims.map((claim) => [claim.id, claim] as const))('%s', (_id, claim) => {
		it('answers rows that exist in claims-vs-code.md', () => {
			for (const row of claim.rows) {
				expect(claimsDoc.includes(`| ${row} |`), `claims-vs-code.md has no row ${row}`).toBe(true);
			}
		});

		it('keeps its sentences on the surfaces they are listed for', () => {
			for (const { in: surface, text } of claim.mustSay) {
				expect(
					surfaceText(surface).includes(normalise(text)),
					`${claim.id}: "${text}" is gone from ${surface}. Re-read the evidence rows ${claim.rows.join(', ')} before rewording it.`,
				).toBe(true);
			}
		});

		it('does not bring back a contradicted phrase', () => {
			const wording = allWording();
			for (const entry of claim.mustNotSay) {
				const phrase = normalise(typeof entry === 'string' ? entry : entry.text).toLowerCase();
				const searched = typeof entry === 'string' ? wording : surfaceText(entry.in).toLowerCase();
				const where = typeof entry === 'string' ? 'a page' : entry.in;
				expect(searched.includes(phrase), `${claim.id}: "${phrase}" is back on ${where}`).toBe(false);
			}
		});

		it('still has the code it rests on', () => {
			for (const { path, anchor } of claim.evidence) {
				const file = join(root, path);
				expect(existsSync(file), `${claim.id}: evidence file ${path} is gone`).toBe(true);
				const message = claim.pending
					? `${claim.id}: the code behind this claim changed ("${anchor}" is gone from ${path}). Build item "${claim.pending}" is the likely cause: re-check the sentences in ${PENDING_DOC}.`
					: `${claim.id}: "${anchor}" is gone from ${path}. The claim may no longer hold: re-read the code, then update the sentence or the anchor.`;
				expect(readFileSync(file, 'utf8').includes(anchor), message).toBe(true);
			}
		});

		it('is listed for the final pass when it depends on unbuilt work', () => {
			if (!claim.pending) return;
			expect(pendingDoc.includes(`\`${claim.id}\``), `${claim.id} is pending (${claim.pending}) but is not in ${PENDING_DOC}`).toBe(true);
			expect(pendingDoc.includes(normalise(claim.pending)), `${PENDING_DOC} does not name the build item "${claim.pending}"`).toBe(true);
		});
	});
});

describe('trust wording: rules that hold whatever the sentences say', () => {
	it('never states zero external traffic or zero egress', () => {
		const wording = allWording();
		const forbidden = [
			/null\s+ekstern/,
			/null\s+egress/,
			/ingen\s+ekstern\s+trafikk/,
			/zero\s+(external|egress)/,
			/no\s+external\s+(traffic|egress)/,
			/0-saas\s*\/\s*no-egress/,
		];
		for (const pattern of forbidden) expect(wording, String(pattern)).not.toMatch(pattern);
	});

	it('does not claim more about erasure than the evidence', () => {
		const control = liveControls.find((entry) => entry.title === 'Rett til sletting (erasure)');
		expect(control, 'erasure control').toBeDefined();
		const body = control?.body ?? '';
		// A positive claim of deleting conversations or memory would be the pack's contradicted sentence.
		const positive = body.replace(/sletter ikke samtaler/g, '');
		expect(positive).not.toMatch(/(sletter|fjerner|fjernes|slettes)\s+(alle\s+)?(samtaler|minne|dokumenter)/);
		expect(body).toMatch(/ikke kjørt ende til ende/);
		expect(body).toMatch(/ikke verifisert ende til ende/);
		expect(control?.level).toBe('progress');
	});

	it('discloses the ChatGPT-account route as global, without EU or ZDR, chosen by the user', () => {
		const rows = subprocessors.filter((row) => row.vendorIds.includes('openai-chatgpt-subscription'));
		expect(rows).toHaveLength(1);
		const [row] = rows;
		expect(row.region).toMatch(/global/i);
		expect(row.note).toMatch(/støtter ikke zdr/i);
		expect(row.purpose).toMatch(/kobler til sin egen chatgpt-konto/i);
		expect(row.level).not.toBe('live');
		expect(surfaceText('control:EU-region på standardmodellene')).toMatch(/global behandling uten eu-garanti/i);
		expect(readSite('TrustCenter.tsx')).toMatch(/uten EU- eller ZDR-garanti/);
	});

	it('keeps the levels it earned: nothing is live without a reason in this file', () => {
		const live = liveControls.filter((entry) => entry.level === 'live').map((entry) => entry.title);
		expect([...live].sort()).toEqual([...LIVE_CONTROLS].sort());
		expect(subprocessors.filter((row) => row.level === 'live').map((row) => row.name)).toEqual([]);
		const liveCerts = certifications.filter((entry) => entry.level === 'live').map((entry) => entry.name);
		expect(liveCerts).toEqual(LIVE_CERTIFICATIONS);
		// No third-party certification is held: every other certification entry is planned.
		for (const item of certifications.filter((entry) => !LIVE_CERTIFICATIONS.includes(entry.name) && !entry.name.startsWith('In-app'))) {
			expect(item.level, item.name).toBe('planned');
		}
	});

	it('does not offer the unreviewed drafts as if they were ready', () => {
		const contact = readSite('TrustCenter.tsx');
		expect(contact).not.toMatch(/Vi deler\s+databehandleravtale/);
		expect(contact).toMatch(/ikke gjennomgått av norsk personvernjurist/);
	});

	it('gives every control and row non-empty text and a unique name', () => {
		for (const entry of liveControls) expect(`${entry.title}${entry.body}`.trim().length, entry.title).toBeGreaterThan(20);
		const names = subprocessors.map((row) => row.name);
		expect(names.filter((name, index) => names.indexOf(name) !== index)).toEqual([]);
		const titles = liveControls.map((entry) => entry.title);
		expect(titles.filter((title, index) => titles.indexOf(title) !== index)).toEqual([]);
	});
});

describe('Trust Center sub-processor list against the egress scan and the register', () => {
	let scanned: string[] = [];
	let register = new Map<string, 'LISTED' | 'EXCLUDED'>();
	const pageIds = new Set(subprocessors.flatMap((row) => row.vendorIds));
	const notListedIds = recipientsNotListed.map((entry) => entry.id);
	const namedAnyway = namedDespiteExclusion.map((entry) => entry.id);

	beforeAll(() => {
		const root = repoRoot();
		scanned = scannedVendorIds(root);
		register = registerDispositions(root);
	}, 60_000);

	it('accounts for every vendor the scan finds', () => {
		const unaccounted = scanned.filter((id) => !pageIds.has(id) && !notListedIds.includes(id));
		expect(unaccounted, 'vendors with neither a page row nor an entry in recipientsNotListed').toEqual([]);
	});

	it('names no vendor the scan no longer finds', () => {
		const stale = [...pageIds, ...notListedIds].filter((id) => !scanned.includes(id));
		expect(stale).toEqual([]);
	});

	it('puts each vendor in one place only', () => {
		const both = notListedIds.filter((id) => pageIds.has(id));
		const duplicated = notListedIds.filter((id, index) => notListedIds.indexOf(id) !== index);
		expect(both).toEqual([]);
		expect(duplicated).toEqual([]);
	});

	it('gives every row a vendor id and every not-listed entry a reason', () => {
		for (const row of subprocessors) expect(row.vendorIds.length, row.name).toBeGreaterThan(0);
		for (const entry of recipientsNotListed) expect(entry.reason.trim().length, entry.id).toBeGreaterThan(0);
		for (const entry of namedDespiteExclusion) expect(entry.reason.trim().length, entry.id).toBeGreaterThan(0);
	});

	it('lists every vendor the register proposes to list', () => {
		const missing = [...register.entries()]
			.filter(([, disposition]) => disposition === 'LISTED')
			.map(([id]) => id)
			.filter((id) => !pageIds.has(id));
		expect(missing, 'vendors the register LISTS that no page row covers').toEqual([]);
	});

	it('keeps the not-listed entries in line with the register', () => {
		for (const entry of recipientsNotListed) {
			expect(register.get(entry.id), `${entry.id} is missing from the register or is LISTED there`).toBe('EXCLUDED');
		}
	});

	it('names an excluded vendor on the page only by a recorded decision', () => {
		const excludedOnPage = [...pageIds].filter((id) => register.get(id) === 'EXCLUDED');
		expect([...excludedOnPage].sort()).toEqual([...namedAnyway].sort());
	});
});
