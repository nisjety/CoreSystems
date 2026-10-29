import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import {
	ProductLoopMediaContent,
	productLoopLayerShowing,
	productLoopPrefersReducedMotion,
	shouldPlayProductLoopVideo,
	type ProductLoopMedia,
} from './ProductLoopProductDemos';
import playerStyles from './ProductLoopProductDemos.module.css';

const film: ProductLoopMedia = {
	alt: 'Verevon svarer med kilder synlige.',
	kind: 'video',
	poster: '/product-scenes/02-plan-poster.webp',
	src: '/product-scenes/02-plan.mp4',
	webm: '/product-scenes/02-plan.webm',
};

const playable = { documentHidden: false, inView: true, layerVisible: true, reducedMotion: false };

describe('shouldPlayProductLoopVideo', () => {
	it('plays only while the film is in view, its stage layer shows and the page is visible', () => {
		expect(shouldPlayProductLoopVideo(playable)).toBe(true);
		expect(shouldPlayProductLoopVideo({ ...playable, inView: false })).toBe(false);
		expect(shouldPlayProductLoopVideo({ ...playable, layerVisible: false })).toBe(false);
		expect(shouldPlayProductLoopVideo({ ...playable, documentHidden: true })).toBe(false);
	});

	it('never plays under prefers-reduced-motion: reduce, whatever else is true', () => {
		expect(shouldPlayProductLoopVideo({ ...playable, reducedMotion: true })).toBe(false);
	});
});

// Mounted behaviour: the film is reported in view as soon as it is observed, and the
// reduced-motion query is controlled by the test so its change event can be fired.
let container: HTMLElement;
let root: Root | undefined;
let reduce = false;
const mediaListeners = new Set<() => void>();
let play: MockInstance<HTMLMediaElement['play']>;
let pause: MockInstance<HTMLMediaElement['pause']>;

class InViewObserver {
	constructor(private readonly callback: (entries: Array<{ isIntersecting: boolean }>) => void) {}
	observe() { this.callback([{ isIntersecting: true }]); }
	unobserve() {}
	disconnect() {}
}

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

beforeEach(() => {
	reduce = false;
	mediaListeners.clear();
	vi.stubGlobal('IntersectionObserver', InViewObserver);
	Object.defineProperty(window, 'matchMedia', {
		configurable: true,
		value: () => ({
			get matches() { return reduce; },
			media: '(prefers-reduced-motion: reduce)',
			addEventListener: (_type: string, listener: () => void) => { mediaListeners.add(listener); },
			removeEventListener: (_type: string, listener: () => void) => { mediaListeners.delete(listener); },
		}),
	});
	Object.defineProperty(document, 'hidden', { configurable: true, get: () => false });
	play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockImplementation(() => Promise.resolve());
	pause = vi.spyOn(HTMLMediaElement.prototype, 'pause').mockImplementation(() => {});
});

afterEach(() => {
	if (root) act(() => root?.unmount());
	root = undefined;
	container?.remove();
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
	delete (window as { matchMedia?: unknown }).matchMedia;
	delete (document as { hidden?: boolean }).hidden;
});

const mount = (media: ProductLoopMedia, { nativeReduce = false }: { nativeReduce?: boolean } = {}) => {
	container = document.createElement('div');
	// The site's layout shim hides the OS preference from matchMedia; this stands in for the native CSS state.
	if (nativeReduce) container.style.setProperty('--product-loop-motion', 'reduce');
	document.body.append(container);
	root = createRoot(container);
	act(() => root?.render(createElement(ProductLoopMediaContent, { media })));
	return container.querySelector('video')!;
};

describe('ProductLoopVideo playback', () => {
	it('pauses on click and resumes on the next click without losing the manual choice on visibility updates', () => {
		mount(film);
		const control = container.querySelector<HTMLButtonElement>('button[aria-label="Sett video på pause"]');
		expect(control).not.toBeNull();
		expect(control?.parentElement?.getAttribute('data-product-video-controls')).toBe('');
		expect(control?.parentElement?.className).toMatch(/bottom-.*right-|right-.*bottom-/);
		play.mockClear();
		pause.mockClear();

		act(() => control?.click());
		expect(pause).toHaveBeenCalled();
		expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Fortsett video');
		act(() => document.dispatchEvent(new Event('visibilitychange')));
		expect(play).not.toHaveBeenCalled();

		act(() => container.querySelector<HTMLButtonElement>('button')?.click());
		expect(play).toHaveBeenCalled();
		expect(container.querySelector('button')?.getAttribute('aria-label')).toBe('Sett video på pause');
	});

	it('provides a seekable timeline to the left of the pause button and updates elapsed time', () => {
		const video = mount(film);
		Object.defineProperty(video, 'duration', { configurable: true, value: 8 });
		Object.defineProperty(video, 'currentTime', { configurable: true, writable: true, value: 2 });
		act(() => video.dispatchEvent(new Event('loadedmetadata')));
		const timeline = container.querySelector<HTMLInputElement>('input[type="range"][aria-label="Spol i video"]')!;
		const control = container.querySelector<HTMLButtonElement>('button[aria-label="Sett video på pause"]')!;
		expect(timeline).not.toBeNull();
		expect(Boolean(timeline.compareDocumentPosition(control) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(true);
		expect(timeline.max).toBe('8');
		expect(timeline.value).toBe('2');
		expect(container.querySelector('[data-product-video-time]')?.textContent).toContain('0:02 / 0:08');
		act(() => {
			timeline.value = '5';
			timeline.dispatchEvent(new Event('input', { bubbles: true }));
		});
		expect(video.currentTime).toBe(5);
	});

	it('keeps the video unzoomed while the card owns the hover treatment and controls reveal', () => {
		const video = mount(film);
		expect(video.className).not.toContain('scale-110');
		expect(video.closest('[data-product-video-card]')).not.toBeNull();
		expect(container.querySelector('[data-product-video-controls]')?.classList.contains(playerStyles.controls)).toBe(true);
		expect(container.innerHTML).not.toContain('bg-black/15');
	});

	it('lets an explicit click play a reduced-motion poster, then pause it', () => {
		reduce = true;
		mount(film);
		expect(play).not.toHaveBeenCalled();
		const control = container.querySelector<HTMLButtonElement>('button[aria-label="Spill video"]');
		expect(control).not.toBeNull();
		act(() => control?.click());
		expect(play).toHaveBeenCalled();
		act(() => container.querySelector<HTMLButtonElement>('button')?.click());
		expect(pause).toHaveBeenCalled();
	});

	it('never calls play() under prefers-reduced-motion: reduce, and the poster stays', () => {
		reduce = true;
		const video = mount(film);
		expect(play).not.toHaveBeenCalled();
		expect(video.getAttribute('poster')).toBe(film.poster);
	});

	it('never calls play() when the native reduced-motion state is set, even where matchMedia reports full motion', () => {
		mount(film, { nativeReduce: true });
		expect(play).not.toHaveBeenCalled();
	});

	it('plays when reduced motion is off and the film is in view', () => {
		mount(film);
		expect(play).toHaveBeenCalled();
	});

	it('pauses when reduced motion is switched on, and resumes only when it is switched off', () => {
		mount(film);
		expect(play).toHaveBeenCalled();

		reduce = true;
		act(() => mediaListeners.forEach((listener) => listener()));
		expect(pause).toHaveBeenCalled();

		play.mockClear();
		act(() => mediaListeners.forEach((listener) => listener()));
		expect(play).not.toHaveBeenCalled();

		reduce = false;
		act(() => mediaListeners.forEach((listener) => listener()));
		expect(play).toHaveBeenCalled();
	});
});

describe('productLoopPrefersReducedMotion', () => {
	it('reads the native reduced-motion state that the film sets through the CSS cascade', () => {
		const plain = document.createElement('div');
		const reduced = document.createElement('div');
		reduced.style.setProperty('--product-loop-motion', 'reduce');
		expect(productLoopPrefersReducedMotion(plain)).toBe(false);
		expect(productLoopPrefersReducedMotion(reduced)).toBe(true);
	});
});

describe('productLoopLayerShowing', () => {
	const created: HTMLElement[] = [];
	const layer = (style: Partial<CSSStyleDeclaration>) => {
		const element = document.createElement('div');
		Object.assign(element.style, style);
		document.body.append(element);
		created.push(element);
		return element;
	};
	afterEach(() => created.splice(0).forEach((element) => element.remove()));

	it('counts a stage layer as showing from its first visible frame, so the poster never paints over the fade-in', () => {
		expect(productLoopLayerShowing(layer({ opacity: '0.02', visibility: 'visible' }))).toBe(true);
		expect(productLoopLayerShowing(layer({ opacity: '1', visibility: 'visible' }))).toBe(true);
	});

	it('treats a hidden or fully transparent layer as not showing', () => {
		expect(productLoopLayerShowing(layer({ opacity: '0', visibility: 'visible' }))).toBe(false);
		expect(productLoopLayerShowing(layer({ opacity: '1', visibility: 'hidden' }))).toBe(false);
	});

	it('treats a film outside the stage layers (the mobile tile) as showing', () => {
		expect(productLoopLayerShowing(null)).toBe(true);
	});
});

describe('ProductLoopVideo markup', () => {
	it('declares webm before mp4 and the silent, looping, non-interactive attributes', () => {
		const video = mount(film);
		const sources = [...video.querySelectorAll('source')].map((source) => [source.getAttribute('type'), source.getAttribute('src')]);
		expect(sources).toEqual([['video/webm', film.webm], ['video/mp4', film.src]]);
		expect(video.loop).toBe(true);
		expect(video.muted).toBe(true);
		expect(video.hasAttribute('playsinline')).toBe(true);
		expect(video.hasAttribute('disablepictureinpicture')).toBe(true);
		expect(video.hasAttribute('disableremoteplayback')).toBe(true);
		expect(video.getAttribute('preload')).toBe('metadata');
		expect(video.getAttribute('aria-label')).toBe(film.alt);
		expect(video.className).toContain('object-contain');
		expect(video.className).toContain('motion-reduce:[--product-loop-motion:reduce]');
	});

	it('declares only the mp4 source when no webm version is given', () => {
		const video = mount({ ...film, webm: undefined });
		const types = [...video.querySelectorAll('source')].map((source) => source.getAttribute('type'));
		expect(types).toEqual(['video/mp4']);
		expect(video.className).toContain('object-cover');
	});
});
