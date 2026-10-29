import { describe, expect, it, vi } from 'vitest';

// GSAP's ScrollTrigger reads window.matchMedia when the section module loads, and jsdom has none.
// Hoisted above the imports so the module can load; the copy and media data never use it.
vi.hoisted(() => {
	Object.defineProperty(window, 'matchMedia', {
		configurable: true,
		value: () => ({
			addEventListener() {},
			addListener() {},
			dispatchEvent: () => false,
			matches: false,
			media: '',
			onchange: null,
			removeEventListener() {},
			removeListener() {},
		}),
	});
});

import { loopStages } from './ProductLoopSection';

const stage = (step: string) => {
	const found = loopStages.find((item) => item.step === step);
	if (!found) throw new Error(`no product loop stage ${step}`);
	return found;
};

describe('product loop film stages', () => {
	it('02 turns a business signal into shared context before a decision is made', () => {
		const item = stage('02');
		expect(`${item.step} / ${item.state}`).toBe('02 / Forstå');
		expect(item.kind).toBe('media');
		expect(item.title).toBe('Se hva som berøres.');
		expect(item.body).toBe('Når noe skjer i virksomheten, samler Verevon det som er relevant fra systemer, kunnskap og historikk. Dere ser sammenhengen før dere tar stilling.');
		expect(item.media).toEqual({
			alt: expect.stringMatching(/fagsystem.*sammenheng/),
			kind: 'video',
			poster: '/product-scenes/02-plan-poster.webp',
			src: '/product-scenes/02-plan.mp4',
			webm: '/product-scenes/02-plan.webm',
		});
	});

	it('03 gives people the relevant basis before work continues', () => {
		const item = stage('03');
		expect(`${item.step} / ${item.state}`).toBe('03 / Vurder');
		expect(item.kind).toBe('media');
		expect(item.title).toBe('Grunnlaget samlet. Valget deres.');
		expect(item.body).toBe('Når noe avviker, viser Verevon hva som har skjedd, hva som berøres og hvilke valg som finnes. Arbeidet går videre når dere har tatt stilling.');
		expect(item.media).toEqual({
			alt: expect.stringMatching(/avvik.*kildene.*alternativer/),
			kind: 'video',
			poster: '/product-scenes/03-undersok-poster.webp',
			src: '/product-scenes/03-undersok.mp4',
			webm: '/product-scenes/03-undersok.webm',
		});
	});

	it('04 Utforsk embeds the interactive dashboard', () => {
		expect(stage('04')).toMatchObject({
			kind: 'live',
			state: 'Utforsk',
			live: { src: '/product-demo/index.html' },
		});
	});
});
