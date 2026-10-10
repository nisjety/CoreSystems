import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { TrustCenter } from './TrustCenter';
import { certifications, liveControls, subprocessors } from './trust-data';

// The page as a reader gets it: the component is rendered, so a sentence that exists in the data or in the JSX
// but never reaches the page (or reaches it twice) is caught here. The wording itself is pinned in
// trust-data.test.ts against trust-claims.ts; this file checks what the render does with it.

const normalise = (text: string) => text.replace(/­/g, '').replace(/ /g, ' ').replace(/\s+/g, ' ').trim();

function render(): { root: HTMLElement; text: string } {
	const root = document.createElement('div');
	root.innerHTML = renderToStaticMarkup(createElement(TrustCenter));
	return { root, text: normalise(root.textContent ?? '') };
}

describe('TrustCenter as rendered', () => {
	const { root, text } = render();

	it('shows every control with its title, body and a status badge', () => {
		const headings = [...root.querySelectorAll('#kontroller h3')].map((node) => normalise(node.textContent ?? ''));
		expect(headings).toEqual(liveControls.map((control) => normalise(control.title)));
		for (const control of liveControls) expect(text, control.title).toContain(normalise(control.body));
		const badges = [...root.querySelectorAll('#kontroller h3 + span')].map((node) => normalise(node.textContent ?? ''));
		expect(badges).toHaveLength(liveControls.length);
		for (const [index, control] of liveControls.entries()) {
			expect(badges[index], control.title).toBe(control.level === 'live' ? 'Live' : 'Under arbeid');
		}
	});

	it('shows one table row per sub-processor, with purpose, region and note', () => {
		const rows = [...root.querySelectorAll('#underleverandorer tbody tr')];
		expect(rows).toHaveLength(subprocessors.length);
		for (const [index, row] of rows.entries()) {
			const expected = subprocessors[index];
			const rendered = normalise(row.textContent ?? '');
			expect(rendered, expected.name).toContain(normalise(expected.name));
			expect(rendered, expected.name).toContain(normalise(expected.purpose));
			expect(rendered, expected.name).toContain(normalise(expected.region));
			expect(rendered, expected.name).toContain(normalise(expected.note));
		}
	});

	it('shows every certification entry as planned except the two Trust Centers', () => {
		const items = [...root.querySelectorAll('#sertifiseringer li')];
		expect(items).toHaveLength(certifications.length);
		for (const [index, item] of items.entries()) {
			const entry = certifications[index];
			const expected = entry.level === 'planned' ? 'Planlagt' : entry.level === 'live' ? 'Live' : 'Under arbeid';
			expect(normalise(item.textContent ?? ''), entry.name).toContain(expected);
		}
	});

	it('says where data goes and what is not promised, in the page text itself', () => {
		expect(text).toContain('Regionen er en etikett vi selv setter');
		expect(text).toContain('global behandling og uten EU- eller ZDR-garanti');
		expect(text).toContain('Nett-søk går via eksterne søkemotorer');
		expect(text).toContain('oppgir vi ikke her før vi kan dokumentere det');
		expect(text).toContain('ChatGPT-ruten støtter ikke ZDR');
		expect(text).toContain('og OpenAI på ChatGPT-ruten');
		expect(text).toContain('Utkastet er ikke gjennomgått av norsk personvernjurist');
	});

	it('does not offer unreviewed documents or a certificate it does not hold', () => {
		expect(text).not.toMatch(/Be om (sikkerhetspakken|pakken)/);
		expect(text).not.toMatch(/Vi deler databehandleravtale/);
		expect(text).not.toMatch(/null ekstern trafikk/i);
		expect(text).not.toMatch(/EU-residens som standard/);
		expect(text).toContain('ennå ingen tredjepartssertifiseringer');
	});

	it('links the contact address and nothing else as a way to get in touch', () => {
		const links = [...root.querySelectorAll('a[href^="mailto:"]')].map((node) => node.getAttribute('href'));
		expect(links.length).toBeGreaterThanOrEqual(2);
		for (const href of links) expect(href).toBe('mailto:sikkerhet@verevon.ai');
	});
});
