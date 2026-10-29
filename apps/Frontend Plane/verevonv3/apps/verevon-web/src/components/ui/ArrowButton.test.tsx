import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { ArrowButton } from "./ArrowButton";

let container: HTMLElement;
let root: Root;

beforeEach(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
});

describe("ArrowButton", () => {
	it("renders the shared Tyde-style CTA anatomy for links", () => {
		act(() => root.render(<ArrowButton href="/produkt/arbeidsflyten">Se arbeidsflyten</ArrowButton>));

		const cta = container.querySelector<HTMLAnchorElement>("a");
		expect(cta?.classList.contains("verevon-cta")).toBe(true);
		expect(cta?.dataset.ctaVariant).toBe("dark");
		expect(cta?.querySelector(".verevon-cta-label")?.textContent).toBe("Se arbeidsflyten");
		expect(cta?.querySelector(".verevon-cta-arrow")?.getAttribute("viewBox")).toBe("0 0 48 8");
	});

	it("keeps contextual colour variants while sharing the same CTA geometry", () => {
		act(() => root.render(<ArrowButton href="#kontakt" variant="light">Snakk med oss</ArrowButton>));

		const cta = container.querySelector<HTMLAnchorElement>("a");
		expect(cta?.classList.contains("verevon-cta")).toBe(true);
		expect(cta?.dataset.ctaVariant).toBe("light");
		expect(cta?.className).toContain("text-verevon-c-white");
	});
});
