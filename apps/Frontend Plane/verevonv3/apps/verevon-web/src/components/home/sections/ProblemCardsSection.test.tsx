import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("next/link", () => ({
	default: ({ children, ...props }: Record<string, unknown> & { children?: ReactNode }) => createElement("a", props, children),
}));

vi.mock("framer-motion", () => ({
	AnimatePresence: ({ children }: { children: ReactNode }) => children,
	motion: {
		span: ({ children, ...props }: Record<string, unknown> & { children?: ReactNode }) => createElement("span", props, children),
	},
	useReducedMotion: () => true,
}));

vi.mock("@/components/home/sections/EndToEndKnowledgeVisual", () => ({
	KnowledgeCardVisual: () => createElement("div"),
}));

vi.mock("@/components/home/sections/Reveal", () => ({
	Reveal: ({ children }: { children: ReactNode }) => children,
}));

vi.mock("@/components/ui/ArrowButton", () => ({
	ArrowButton: ({ children, ...props }: Record<string, unknown> & { children?: ReactNode }) => createElement("a", props, children),
	ArrowButtonLabel: ({ children }: { children?: ReactNode }) => createElement("span", { className: "verevon-cta-label" }, children),
}));

import { ProblemCardsSection } from "./ProblemCardsSection";

let container: HTMLElement;
let root: Root;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

beforeEach(() => {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
	act(() => root.render(createElement(ProblemCardsSection)));
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
});

describe("ProblemCardsSection navigation", () => {
	it("uses the Stingray-style navigation rail for the three card destinations", () => {
		const navigation = container.querySelector('[aria-label="Utforsk Verevon"]');
		expect(navigation?.getAttribute("data-navigation-style")).toBe("stingray");

		const links = [...(navigation?.querySelectorAll<HTMLAnchorElement>("a") ?? [])];
		expect(links).toHaveLength(3);
		expect(links.map((link) => link.getAttribute("data-navigation-link"))).toEqual([
			"stingray",
			"stingray",
			"stingray",
		]);
	});

	it("presents a governed, cross-system business environment rather than a standalone chat assistant", () => {
		expect(container.textContent).toContain("Koble virksomheten sammen igjen.");
		expect(container.textContent).toContain("systemer, kunnskap og arbeid i én felles arbeidsflate");
		expect(container.textContent).toContain("Fagsystemene blir stående.");
		expect(container.textContent).toContain("Kunnskapen blir tilgjengelig der arbeidet skjer.");
		expect(container.textContent).toContain("svar, planer og handlinger bygger på samme sporbare grunnlag");
		expect(container.textContent).toContain("Én arbeidsflate for mennesker og agenter.");
		expect(container.textContent).not.toContain("En AI-assistent med bedriftens kunnskap");
		expect(container.querySelector<HTMLAnchorElement>('a[href="/plattform/felles-kontekst#kilder"]')?.textContent).toContain("Utforsk kunnskapen");
	});

	it("makes the knowledge and workplace cards single, whole-card destinations", () => {
		const knowledgeCard = container.querySelector<HTMLAnchorElement>('[data-card-link="knowledge"]');
		const businessCard = container.querySelector<HTMLAnchorElement>('[data-card-link="business"]');

		expect(knowledgeCard?.getAttribute("href")).toBe("/plattform/felles-kontekst#kilder");
		expect(businessCard?.getAttribute("href")).toBe("/plattform/kontrollert-arbeid");
		expect(knowledgeCard?.querySelector("button")).toBeNull();
		expect(businessCard?.querySelector("button")).toBeNull();
	});
});
