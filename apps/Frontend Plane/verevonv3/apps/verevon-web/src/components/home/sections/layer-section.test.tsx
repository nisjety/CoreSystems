import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";

vi.mock("next/image", () => ({
	default: (props: Record<string, unknown>) => createElement("img", props),
}));

vi.mock("../../../shared/hooks/usePrefersReducedMotion", () => ({
	usePrefersReducedMotion: () => true,
}));

import { LayerSection } from "./layer-section";

let container: HTMLElement;
let root: Root;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	vi.stubGlobal(
		"IntersectionObserver",
		class {
			observe() {}
			unobserve() {}
			disconnect() {}
		},
	);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
});

const mount = () => {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
	act(() => root.render(createElement(LayerSection)));
};

const controls = () => [...container.querySelectorAll<HTMLButtonElement>('nav[aria-label="Velg lag i plattformen"] button[aria-expanded]')];
const press = (name: string) => {
	const control = controls().find(
		(button) => button.querySelector("[data-layer-title]")?.textContent === name,
	);
	expect(control, `Missing layer control: ${name}`).toBeDefined();
	act(() => control?.click());
};

describe("LayerSection", () => {
	it("keeps the four layers as one accessible, single-select stack", () => {
		mount();
		expect(controls().map((button) => button.textContent?.replace(/\s+/g, " ").trim())).toEqual([
			expect.stringContaining("Kontroll"),
			expect.stringContaining("Agentarbeid"),
			expect.stringContaining("Kunnskap"),
			expect.stringContaining("Kilder"),
		]);
		expect(controls().map((button) => button.getAttribute("aria-expanded"))).toEqual([
			"true",
			"false",
			"false",
			"false",
		]);
		for (const control of controls()) {
			const panelId = control.getAttribute("aria-controls");
			expect(panelId).toBeTruthy();
			expect(container.querySelector(`#${panelId}`)).not.toBeNull();
		}
		const stage = container.querySelector<HTMLElement>("[data-platform-stage]");
		expect(stage?.className).toContain("overflow-visible");
		expect(stage?.className).not.toContain("bg-background");
		expect(stage?.className).not.toContain("border-verevon-j-text");
		expect(stage?.className).not.toContain("shadow-[");
		expect(container.querySelectorAll('[data-framer-layer][data-layer-id][data-active="true"]')).toHaveLength(1);
		expect(container.querySelectorAll('[data-framer-layer][data-layer-id]')).toHaveLength(4);
		expect(
			[...container.querySelectorAll('[data-framer-layer][data-layer-id]')].map(
				(layer) => layer.getAttribute("data-stack-state"),
			),
		).toEqual(["front", "under-1", "under-2", "under-3"]);
		expect(
			[...container.querySelectorAll('[data-framer-layer][data-layer-id]')].map(
				(layer) => layer.getAttribute("data-stack-offset"),
			),
		).toEqual(["-220", "68", "112", "152"]);
		expect(container.querySelectorAll('[data-framer-layer][data-stack-visible="true"]')).toHaveLength(4);
	});

	it("updates the visual focal layer and the connected explanation together", () => {
		mount();
		press("Kilder");
		expect(controls().map((button) => button.getAttribute("aria-expanded"))).toEqual([
			"false",
			"false",
			"false",
			"true",
		]);
		expect(container.querySelector('[data-layer-id="infrastructure"][data-active="true"]')).not.toBeNull();
		expect(
			[...container.querySelectorAll('[data-framer-layer][data-layer-id]')].map(
				(layer) => layer.getAttribute("data-stack-state"),
			),
		).toEqual(["passed-3", "passed-2", "passed-1", "front"]);
		expect(
			[...container.querySelectorAll('[data-framer-layer][data-layer-id]')].map(
				(layer) => layer.getAttribute("data-stack-offset"),
			),
		).toEqual(["-218", "-168", "-122", "210"]);
		expect(container.querySelector('#platform-layer-panel-infrastructure')?.textContent).toContain("Fagsystemene blir stående");
		expect(container.querySelector('#platform-layer-panel-management')?.getAttribute("aria-hidden")).toBe("true");
		expect(container.querySelector('#platform-layer-panel-infrastructure')?.getAttribute("aria-hidden")).toBe("false");

		press("Agentarbeid");
		expect(container.querySelector('[data-layer-id="orchestration"][data-active="true"]')).not.toBeNull();
		expect(
			[...container.querySelectorAll('[data-framer-layer][data-layer-id]')].map(
				(layer) => layer.getAttribute("data-stack-state"),
			),
		).toEqual(["passed-1", "front", "under-1", "under-2"]);
		expect(
			[...container.querySelectorAll('[data-framer-layer][data-layer-id]')].map(
				(layer) => layer.getAttribute("data-stack-offset"),
			),
		).toEqual(["-300", "44", "330", "378"]);
		expect(container.querySelectorAll('[data-framer-layer][data-stack-visible="true"]')).toHaveLength(4);
		expect(container.querySelector('#platform-layer-panel-orchestration')?.textContent).toContain("Mennesker tar beslutningene");
	});

	it("retains the original editorial hierarchy and fuller layer narrative", () => {
		mount();
		expect(container.textContent).toContain("Alt i sammenheng. Kontroll hele veien.");
		expect(container.textContent).not.toContain("04 / Plattform");
		expect(container.textContent).toContain("fra første signal til oppfølging");
		expect(container.querySelector('[data-layer-disclosure="intric"]')).not.toBeNull();
		const expectedRows = [
			["Øverste lag", "Kontroll", "Regler, tilgang og godkjenning rundt arbeidet."],
			["Andre lag", "Agentarbeid", "AI-arbeid innenfor rammene dere setter."],
			["Tredje lag", "Kunnskap", "Virksomhetens kunnskap, samlet i kontekst."],
			["Nederste lag", "Kilder", "Systemene og kildene dere allerede bruker."],
		];
		for (const [index, expected] of expectedRows.entries()) {
			const row = controls()[index];
			expect(row.querySelector('[data-layer-toggle]')).not.toBeNull();
			expect(row.textContent).toContain(expected[0]);
			expect(row.textContent).toContain(expected[1]);
			const panelId = row.getAttribute("aria-controls");
			expect(container.querySelector(`#${panelId}`)?.textContent).toContain(expected[2]);
			expect(row.querySelector("[data-layer-position]")?.className).toContain("font-geist");
			expect(row.querySelector("[data-layer-title]")?.className).toContain("font-geist");
		}
		expect(container.querySelector("#platform-layer-panel-management")?.textContent).toContain("Dere bestemmer hva AI kan se");
		expect(container.querySelector("#platform-layer-panel-management")?.textContent).toContain("Alt kan følges og etterprøves");
		press("Kilder");
		expect(container.querySelector("#platform-layer-panel-infrastructure")?.textContent).toContain("Verevon kobler seg til avtalte systemer");
		expect(container.querySelector("#platform-layer-panel-infrastructure")?.textContent).toContain("informasjonen blir tilgjengelig der arbeidet skjer");
	});

	it("uses the supplied platform artwork without rebuilding labels over the imagery", () => {
		mount();
		expect(container.textContent).toContain("Regler, tilgang og godkjenning rundt arbeidet.");
		press("Kilder");
		expect(container.textContent).toContain("Systemene og kildene dere allerede bruker.");
		press("Kunnskap");
		expect(container.textContent).toContain("Virksomhetens kunnskap, samlet i kontekst.");
		press("Agentarbeid");
		expect(container.textContent).toContain("AI-arbeid innenfor rammene dere setter.");
		for (const [layerId, src] of [
			["management", "/verevon-layers/kontroll-transparent.png"],
			["orchestration", "/verevon-layers/agentarbeid-transparent.png"],
			["business-context", "/verevon-layers/kunnskap-transparent.png"],
			["infrastructure", "/verevon-layers/kilder-transparent.png"],
		]) {
			const layer = container.querySelector(`[data-layer-id="${layerId}"]`);
			expect(layer?.querySelector(`img[src="${src}"]`)).not.toBeNull();
		}
		expect(container.querySelectorAll("svg image")).toHaveLength(0);
	});
});
