import { act, createElement, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("next/image", () => ({
	default: ({ alt, ...props }: Record<string, unknown> & { alt?: string }) => createElement("img", { alt, ...props }),
}));

vi.mock("next/link", () => ({
	default: ({ children, ...props }: Record<string, unknown> & { children?: ReactNode }) => createElement("a", props, children),
}));

vi.mock("./KnowledgeGraph", () => ({
	KnowledgeGraph: () => createElement("div"),
}));

import { ConnectVisual } from "./EndToEndKnowledgeVisual";
import { SYSTEM_FIELD_ITEMS } from "./brand-connector-data";

let container: HTMLElement;

beforeAll(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
});

beforeEach(() => {
	container = document.createElement("div");
	document.body.append(container);
	act(() => {
		createRoot(container).render(createElement(ConnectVisual, { expanded: true }));
	});
});

afterEach(() => {
	container.remove();
});

describe("ConnectVisual integration destinations", () => {
	it("links each familiar system tile to its selected integration in the catalogue", () => {
		const links = [...container.querySelectorAll<HTMLAnchorElement>("[data-system-link]")];

		expect(links).toHaveLength(SYSTEM_FIELD_ITEMS.length);
		for (const item of SYSTEM_FIELD_ITEMS) {
			expect(links.find((link) => link.dataset.integrationId === item.id)?.getAttribute("href")).toBe(
			`/integrasjoner?integration=${item.id}#katalog`,
		);
		}
	});
});
