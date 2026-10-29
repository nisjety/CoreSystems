import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { FeatureComposerCycle } from "./FeatureComposerCycle";

let container: HTMLElement;
let root: Root;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	vi.stubGlobal("IntersectionObserver", class {
		constructor(private readonly callback: IntersectionObserverCallback) {}
		disconnect() {}
		observe() {
			this.callback(
				[{ isIntersecting: true } as IntersectionObserverEntry],
				this as unknown as IntersectionObserver,
			);
		}
	});
	vi.stubGlobal("MutationObserver", class {
		disconnect() {}
		observe() {}
	});
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		value: () => ({
			addEventListener() {},
			matches: false,
			removeEventListener() {},
		}),
	});
});

afterEach(() => {
	if (root) act(() => root.unmount());
	container?.remove();
	vi.useRealTimers();
});

function mount(onModeChange = vi.fn()) {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
	act(() => root.render(<FeatureComposerCycle id="composer-cycle-test" onModeChange={onModeChange} />));
	return onModeChange;
}

describe("FeatureComposerCycle", () => {
	it("uses compact Norwegian labels for the three composer switches", () => {
		const html = renderToStaticMarkup(<FeatureComposerCycle id="composer-cycle-test" />);

		expect(html).toContain('role="group"');
		expect(html).toContain('data-composer-mode-switch="chat"');
		expect(html).toContain('data-composer-mode-switch="crawl"');
		expect(html).toContain('data-composer-mode-switch="search"');
		expect(html).toContain(">Spør<");
		expect(html).toContain(">Hent<");
		expect(html).toContain(">Søk<");
		expect(html).not.toContain(">Chat<");
		expect(html).not.toContain(">Crawl<");
	});

	it("uses the Norwegian open-search prompt", () => {
		const onModeChange = mount();
		const search = container.querySelector<HTMLButtonElement>(
			'[data-composer-mode-switch="search"]',
		);

		act(() => search?.click());
		expect(container.querySelector('[data-feature-composer-mode="search"]')?.textContent).toContain(
			"Søk på hva som helst",
		);
		expect(container.textContent).not.toContain("Søk eller spør ...");
		expect(onModeChange).toHaveBeenLastCalledWith("search");
	});

	it("switches the preview immediately when a mode button is selected", () => {
		vi.useFakeTimers();
		const onModeChange = mount();
		const hent = container.querySelector<HTMLButtonElement>(
			'[data-composer-mode-switch="crawl"]',
		);

		expect(hent).not.toBeNull();
		expect(hent?.getAttribute("aria-pressed")).toBe("false");
		act(() => hent?.click());

		expect(container.querySelector('[data-feature-composer-mode="crawl"]')).not.toBeNull();
		expect(hent?.getAttribute("aria-pressed")).toBe("true");
		expect(onModeChange).toHaveBeenLastCalledWith("crawl");
		act(() => vi.advanceTimersByTime(5000));
		expect(container.querySelector('[data-feature-composer-mode="crawl"]')).not.toBeNull();
	});
});
