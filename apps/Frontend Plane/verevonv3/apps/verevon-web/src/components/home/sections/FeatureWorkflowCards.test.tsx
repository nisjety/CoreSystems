import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("next/image", () => ({
	default: (props: Record<string, unknown>) => <img {...props} />,
}));

vi.mock("./FeatureCardFilms", () => ({
	FeatureCardFilm: () => <div data-feature-card-film="" />,
}));

import { ModuleWorkflowCard, moduleCards } from "./FeatureWorkflowCards";

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

describe("ModuleWorkflowCard", () => {
	it("uses one accessible link for the complete card", () => {
		const card = moduleCards[2];

		act(() =>
			root.render(
				<ModuleWorkflowCard
					animated={false}
					card={card}
					index={2}
					total={moduleCards.length}
				/>,
			),
		);

		const link = container.querySelector<HTMLAnchorElement>("a[data-module-card-link]");
		expect(link?.getAttribute("href")).toBe(card.href);
		expect(link?.className).toContain("cursor-pointer");
		expect(link?.getAttribute("aria-labelledby")).toBe("module-card-2-title");
		expect(link?.querySelector(".verevon-cta-label")?.textContent).toBe(card.linkLabel);
		expect(link?.querySelector("a")).toBeNull();
	});
});
