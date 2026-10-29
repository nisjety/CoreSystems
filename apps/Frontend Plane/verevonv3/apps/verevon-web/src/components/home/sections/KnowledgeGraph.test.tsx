import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("next/link", () => ({
	default: ({ children, ...props }: Record<string, unknown>) => createElement("a", props, children),
}));

vi.mock("@/shared/hooks/usePrefersReducedMotion", () => ({
	usePrefersReducedMotion: () => false,
}));

import { KnowledgeGraph } from "./KnowledgeGraph";
import { knowledgeDepartments } from "./knowledgeGraphData";

let container: HTMLElement;
let root: Root;

beforeEach(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	globalThis.IntersectionObserver = class {
		disconnect() {}
		observe() {}
		unobserve() {}
	} as typeof IntersectionObserver;
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
});

describe("KnowledgeGraph", () => {
	it("makes every department node a link to the knowledge page", () => {
		act(() => root.render(<KnowledgeGraph expanded />));

		const nodes = [...container.querySelectorAll<HTMLAnchorElement>("[data-business-node]")];
		expect(nodes).toHaveLength(knowledgeDepartments.length);
		expect(nodes.every((node) => node.getAttribute("href") === "/plattform/felles-kontekst#kilder")).toBe(true);
	});
});
