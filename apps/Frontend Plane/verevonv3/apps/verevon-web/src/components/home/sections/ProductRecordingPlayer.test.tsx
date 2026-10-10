import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import type { ProductRecording } from '../../../lib/product-recording-contract';
import { ProductRecordingPlayer } from './ProductRecordingPlayer';

// The films are silent, so the transcript is the only equivalent a screen-reader user gets.
// The short film and the full task have different cue lists on purpose.
const recording = (id: string, label: string): ProductRecording => ({
	id, label, title: `${label}: tittel.`,
	video: `/verevon-product-recordings/${id}.mp4`, raw: `/verevon-product-recordings/${id}-raw.webm`, poster: `/verevon-product-recordings/${id}.jpg`,
	captions: `/verevon-product-recordings/${id}.vtt`, rawCaptions: `/verevon-product-recordings/${id}-raw.vtt`,
	transcript: [`${label}: åpner henvendelsen.`, `${label}: skriver svarutkastet.`],
	rawTranscript: [`${label}, hele: åpner henvendelsen.`, `${label}, hele: leser kildene.`, `${label}, hele: venter på svaret.`, `${label}, hele: skriver svarutkastet.`],
	editingNote: 'Ventetid er klippet.',
});
const recordings = [recording('01-kundesvar', 'Kundesvar'), recording('02-salgsrapport', 'Salgsanalyse')];

let container: HTMLElement;
let root: Root;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	vi.stubGlobal('IntersectionObserver', class { observe() {} unobserve() {} disconnect() {} });
	vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
});
afterEach(() => { act(() => root.unmount()); container.remove(); });

const mount = (items = recordings) => {
	container = document.createElement('div');
	document.body.append(container);
	root = createRoot(container);
	act(() => root.render(createElement(ProductRecordingPlayer, { recordings: items })));
};
const disclosure = () => container.querySelector<HTMLDetailsElement>('details[data-recording-transcript]')!;
const summary = () => disclosure().querySelector('summary')!;
const cues = () => [...disclosure().querySelectorAll('ol > li')].map(item => item.textContent);
const button = (name: string) => [...container.querySelectorAll('button')].find(item => item.textContent === name)!;
const press = (element: HTMLElement) => act(() => element.click());
const before = (first: Node, second: Node) => Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);

describe('ProductRecordingPlayer text alternative', () => {
	it('shows a collapsed "Tekstalternativ" disclosure under the video and the full/short toggle', () => {
		mount();
		expect(disclosure()).not.toBeNull();
		expect(disclosure().open).toBe(false);
		expect(summary().textContent).toBe('Tekstalternativ');
		expect(disclosure().querySelector('summary')).toBe(disclosure().firstElementChild);
		const video = container.querySelector('video')!;
		expect(before(video, disclosure())).toBe(true);
		expect(before(button('Se hele oppgaven'), disclosure())).toBe(true);
	});

	it('lists every cue of the short film, in order, as a numbered list', () => {
		mount();
		expect(cues()).toEqual(recordings[0]!.transcript);
		expect(disclosure().querySelectorAll('ol')).toHaveLength(1);
	});

	it('is operable from the keyboard: a focusable native summary that opens and closes on activation', () => {
		mount();
		// A real <summary> in a <details> is a tab stop and Enter or Space activates it as a click.
		expect(summary().tagName).toBe('SUMMARY');
		expect(summary().hasAttribute('tabindex')).toBe(false);
		expect(disclosure().hasAttribute('aria-hidden')).toBe(false);
		act(() => summary().focus());
		expect(document.activeElement).toBe(summary());
		press(summary());
		expect(disclosure().open).toBe(true);
		press(summary());
		expect(disclosure().open).toBe(false);
	});

	it('follows the full/short toggle and keeps its open state', () => {
		mount();
		press(summary());
		expect(cues()).toEqual(recordings[0]!.transcript);
		expect(disclosure().textContent).toContain('kortfilm');

		press(button('Se hele oppgaven'));
		expect(cues()).toEqual(recordings[0]!.rawTranscript);
		expect(disclosure().textContent).toContain('hele oppgaven');
		expect(container.querySelector('video')!.getAttribute('src')).toBe(recordings[0]!.raw);
		expect(disclosure().open).toBe(true);

		press(button('Vis kortfilmen'));
		expect(cues()).toEqual(recordings[0]!.transcript);
		expect(container.querySelector('video')!.getAttribute('src')).toBe(recordings[0]!.video);
	});

	it('follows the selected task and starts on the short film again', () => {
		mount();
		press(button('Se hele oppgaven'));
		press(button('Salgsanalyse'));
		expect(cues()).toEqual(recordings[1]!.transcript);
		expect(disclosure().textContent).toContain('Salgsanalyse');
		press(button('Se hele oppgaven'));
		expect(cues()).toEqual(recordings[1]!.rawTranscript);
	});

	it('names the film and the list for assistive technology and keeps every id reference valid', () => {
		mount();
		const list = disclosure().querySelector('ol')!;
		const intro = container.querySelector(`#${CSS.escape(list.getAttribute('aria-labelledby') ?? 'none')}`);
		expect(intro?.textContent).toMatch(/Kundesvar.*kortfilm.*ingen tale/);
		const ids = [...container.querySelectorAll('[id]')].map(item => item.id);
		expect(new Set(ids).size).toBe(ids.length);
		for (const attribute of ['aria-labelledby', 'aria-describedby']) {
			for (const item of container.querySelectorAll(`[${attribute}]`)) {
				for (const id of item.getAttribute(attribute)!.split(' ')) expect(container.querySelector(`#${CSS.escape(id)}`), `${attribute} -> ${id}`).not.toBeNull();
			}
		}
		for (const item of container.querySelectorAll('button, summary, video')) expect((item.getAttribute('aria-label') ?? item.textContent ?? '').trim(), item.outerHTML).not.toBe('');
	});

	it('renders cue text as text, never as markup', () => {
		mount([{ ...recordings[0]!, transcript: ['<img src=x onerror=alert(1)> & <b>fet</b>'] }, ...recordings.slice(1)]);
		expect(disclosure().querySelector('img, b')).toBeNull();
		expect(cues()).toEqual(['<img src=x onerror=alert(1)> & <b>fet</b>']);
	});

	it('keeps the captions track, no autoplay and no motion of its own', () => {
		mount();
		const video = container.querySelector('video')!;
		const track = video.querySelector('track')!;
		expect([track.kind, track.srclang, track.default, track.getAttribute('src')]).toEqual(['captions', 'nb', true, recordings[0]!.captions]);
		expect(video.autoplay).toBe(false);
		expect(video.getAttribute('preload')).toBe('none');
		expect(video.controls).toBe(true);
		expect(disclosure().outerHTML).not.toMatch(/animate|transition|motion/);
		press(button('Se hele oppgaven'));
		expect(container.querySelector('video track')!.getAttribute('src')).toBe(recordings[0]!.rawCaptions);
	});

	it('renders nothing without recordings', () => {
		mount([]);
		expect(container.innerHTML).toBe('');
	});
});
