import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const navbar = vi.hoisted(() => vi.fn(({ isIntroActive }: { isIntroActive?: boolean }) => (
	<div data-navbar="" data-intro-active={String(isIntroActive)} />
)));

vi.mock("@/components/core/navbar/Navbar", () => ({ Navbar: navbar }));
vi.mock("@/components/ui/MenuModal", () => ({ MenuModal: () => null }));
vi.mock("./intro/HomepageIntro", () => ({
	HomepageIntro: ({ onComplete }: { onComplete: () => void }) => (
		<button onClick={onComplete} type="button">Ferdig med intro</button>
	),
}));

import { HomeClientShell } from "./HomeClientShell";

let container: HTMLElement;
let root: Root;

beforeEach(() => {
	(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
	vi.stubGlobal("matchMedia", () => ({ matches: false }));
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
	vi.restoreAllMocks();
	vi.unstubAllGlobals();
});

describe("HomeClientShell intro handoff", () => {
	it("keeps the page inert and the header brand concealed until the intro completes", () => {
		act(() => root.render(
			<HomeClientShell
				content={<div>Innhold</div>}
				footer={<div>Footer</div>}
				hero={<div>Hero</div>}
			/>,
		));
		const page = container.querySelector<HTMLElement>("[data-home-content-shell]");
		expect(page?.hasAttribute("inert")).toBe(true);
		expect(container.querySelector("[data-navbar]")?.getAttribute("data-intro-active")).toBe("true");

		act(() => container.querySelector<HTMLButtonElement>("button")?.click());
		expect(page?.hasAttribute("aria-hidden")).toBe(false);
		expect(container.querySelector("[data-navbar]")?.getAttribute("data-intro-active")).toBe("false");
	});
});
