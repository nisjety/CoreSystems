import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { Footer } from "./Footer";

let container: HTMLElement;
let root: Root;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

beforeEach(() => {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
});

describe("Footer typography", () => {
	it("uses Verevon Sans only for the brand wordmark and Manrope for the footer system", () => {
		act(() => root.render(<Footer />));

		const footer = container.querySelector("footer");
		const wordmark = container.querySelector<HTMLAnchorElement>('a[aria-label="Verevon hjem"]');

		expect(footer?.classList.contains("font-manrope")).toBe(true);
		expect(wordmark?.classList.contains("verevon-wordmark")).toBe(true);
		expect(wordmark?.classList.contains("font-manrope")).toBe(false);
	});

	it("closes with the same cross-system workplace promise as the homepage", () => {
		act(() => root.render(<Footer />));

		expect(container.textContent).toContain("Én arbeidsflate på tvers av systemene dere bruker");
		expect(container.textContent).toContain("Snakk med oss");
		expect(container.textContent).toContain("Made in Norway.");
		expect(container.textContent).toContain("Private by design.");
		expect(container.textContent).not.toContain("Tidlig tilgang");
		expect(container.querySelector<HTMLAnchorElement>('a[href="/trust#dataflyt"]')?.textContent).toContain("Dataflyt og underleverandører");
		expect(container.querySelector<HTMLAnchorElement>('a[href="/trust#kontroller"]')?.textContent).toContain("Sikkerhet");
	});
});
