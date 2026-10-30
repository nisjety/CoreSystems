import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";
import { VerevonComposerPreview } from "./VerevonComposerPreview";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let container: HTMLElement;
let root: Root;

function mockMotionPreference(reduced: boolean) {
	let changeListener: ((event: MediaQueryListEvent) => void) | undefined;
	const mediaQuery = {
		addEventListener(_event: string, listener: (event: MediaQueryListEvent) => void) {
			changeListener = listener;
		},
		matches: reduced,
		removeEventListener(_event: string, listener: (event: MediaQueryListEvent) => void) {
			if (changeListener === listener) changeListener = undefined;
		},
	};
	vi.stubGlobal("matchMedia", () => mediaQuery);

	return {
		setReducedMotion(nextReduced: boolean) {
			mediaQuery.matches = nextReduced;
			changeListener?.({ matches: nextReduced } as MediaQueryListEvent);
		},
	};
}

function mountCrawlPreview({ ambientMotion = true }: { ambientMotion?: boolean } = {}) {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
	act(() => {
		root.render(
			<VerevonComposerPreview
				ambientMotion={ambientMotion}
				mode="crawl"
				prompt="Lim inn en nettside for å hente innhold."
			/>,
		);
	});
}

function expectCrawlSelection({
	icon,
	placeholder,
	source,
}: {
	icon: string;
	placeholder: string;
	source: "lenke" | "crawl" | "produkter";
}) {
	for (const controlSource of ["lenke", "crawl", "produkter"] as const) {
		const control = container.querySelector<HTMLButtonElement>(
			`[data-composer-crawl-control="${controlSource}"]`,
		);
		expect(control).not.toBeNull();
		expect(control?.getAttribute("aria-pressed")).toBe(
			String(controlSource === source),
		);
	}

	expect(
		container.querySelector("[data-composer-crawl-placeholder]")?.textContent,
	).toBe(placeholder);
	expect(
		container
			.querySelector("[data-composer-crawl-input-icon]")
			?.getAttribute("data-composer-crawl-input-icon"),
	).toBe(icon);
}

afterEach(() => {
	if (root) {
		act(() => root.unmount());
	}
	container?.remove();
	vi.unstubAllGlobals();
	vi.useRealTimers();
});

describe("VerevonComposerPreview ambient motion", () => {
	it("adds a decorative prompt and send affordance to the chat preview", () => {
		const html = renderToStaticMarkup(
			<VerevonComposerPreview
				ambientMotion
				mode="chat"
				prompt="Spør om hva som helst"
			/>,
		);

		expect(html).toContain("data-composer-idle-caret");
		expect(html).toContain("data-composer-idle-send");
		expect(html).toContain("motion-safe:animate-[verevon-composer-idle-caret");
		expect(html).toMatch(/class="[^"]*opacity-0[^"]*" data-composer-idle-send/);
	});

	it("adds the expanding search pill only to the search preview", () => {
		const html = renderToStaticMarkup(
			<VerevonComposerPreview ambientMotion mode="search" prompt="Søk eller spør" />,
		);

		expect(html).toContain("data-composer-search-pill");
		expect(html).toContain("data-composer-search-content");
		expect(html).toContain("motion-safe:animate-[verevon-composer-search-pill");
		expect(html).toContain('aria-label="Søk"');
		expect(html).not.toContain("data-composer-idle-caret");
	});

	it("adds a cursor pass over the Norwegian link option only to the crawl preview", () => {
		const html = renderToStaticMarkup(
			<VerevonComposerPreview ambientMotion mode="crawl" prompt="Lim inn en nettside" />,
		);

		expect(html).toContain("data-composer-crawl-cursor");
		expect(html).toContain("data-composer-crawl-link");
		expect(html).toContain("motion-safe:animate-[verevon-composer-crawl-cursor");
		expect(html).toMatch(/data-composer-crawl-link="true"[\s\S]*data-composer-crawl-cursor/);
		expect(html).toContain("z-20");
		expect(html).toMatch(/class="[^"]*opacity-0[^"]*"[^>]*data-composer-crawl-cursor/);
		expect(html).toContain("Hent</button>");
		expect(html).toContain("data-composer-crawl-submit");
		expect(html).not.toContain("data-composer-idle-send");
	});
});

describe("VerevonComposerPreview crawl controls", () => {
	it("switches the selected source, placeholder, and leading icon", () => {
		mountCrawlPreview();

		const controls = container.querySelector('[data-composer-crawl-controls]');
		expect(controls?.getAttribute("role")).toBe("group");
		expect(controls?.getAttribute("aria-label")).toBe("Velg innholdstype");
		expect(container.querySelector("[data-composer-crawl-cursor]")).not.toBeNull();
		expectCrawlSelection({
			icon: "globe",
			placeholder: "Lim inn en nettside for å hente innhold.",
			source: "crawl",
		});

		const lenke = container.querySelector<HTMLButtonElement>(
			'[data-composer-crawl-control="lenke"]',
		);
		const produkter = container.querySelector<HTMLButtonElement>(
			'[data-composer-crawl-control="produkter"]',
		);
		expect(lenke?.tagName).toBe("BUTTON");
		expect(lenke?.type).toBe("button");
		expect(produkter?.tagName).toBe("BUTTON");
		expect(produkter?.type).toBe("button");

		act(() => lenke?.click());
		expectCrawlSelection({
			icon: "link",
			placeholder: "Lim inn en lenke …",
			source: "lenke",
		});
		expect(container.querySelector("[data-composer-crawl-cursor]")).toBeNull();

		act(() => produkter?.click());
		expectCrawlSelection({
			icon: "shopping-bag",
			placeholder: "Lim inn en produktside …",
			source: "produkter",
		});
	});

	it("selects Lenke after the ambient cursor reaches its target", () => {
		vi.useFakeTimers();
		mockMotionPreference(false);
		mountCrawlPreview();

		act(() => vi.advanceTimersByTime(1_599));
		expectCrawlSelection({
			icon: "globe",
			placeholder: "Lim inn en nettside for å hente innhold.",
			source: "crawl",
		});

		act(() => vi.advanceTimersByTime(1));
		expectCrawlSelection({
			icon: "link",
			placeholder: "Lim inn en lenke …",
			source: "lenke",
		});
	});

	it("cancels the ambient Lenke selection after a manual source choice", () => {
		vi.useFakeTimers();
		mockMotionPreference(false);
		mountCrawlPreview();

		const produkter = container.querySelector<HTMLButtonElement>(
			'[data-composer-crawl-control="produkter"]',
		);
		act(() => produkter?.click());
		act(() => vi.advanceTimersByTime(2_000));

		expectCrawlSelection({
			icon: "shopping-bag",
			placeholder: "Lim inn en produktside …",
			source: "produkter",
		});
	});

	it("does not auto-select Lenke when reduced motion is preferred", () => {
		vi.useFakeTimers();
		mockMotionPreference(true);
		mountCrawlPreview();

		act(() => vi.advanceTimersByTime(2_000));
		expectCrawlSelection({
			icon: "globe",
			placeholder: "Lim inn en nettside for å hente innhold.",
			source: "crawl",
		});
	});

	it("cancels the ambient Lenke selection when reduced motion is enabled mid-animation", () => {
		vi.useFakeTimers();
		const motion = mockMotionPreference(false);
		mountCrawlPreview();

		act(() => vi.advanceTimersByTime(800));
		act(() => motion.setReducedMotion(true));
		act(() => vi.advanceTimersByTime(1_000));

		expectCrawlSelection({
			icon: "globe",
			placeholder: "Lim inn en nettside for å hente innhold.",
			source: "crawl",
		});
	});
});
