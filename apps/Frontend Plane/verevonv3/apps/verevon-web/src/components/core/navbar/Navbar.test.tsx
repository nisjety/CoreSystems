import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("next/navigation", () => ({
	usePathname: () => "/produkt/arbeidsflyten",
}));
import { Navbar } from "./Navbar";

let container: HTMLElement;
let root: Root;

beforeEach(() => {
	container = document.createElement("div");
	document.body.append(container);
	root = createRoot(container);
});

afterEach(() => {
	act(() => root.unmount());
	container.remove();
});

describe("Navbar intro landing target", () => {
	it("reserves one persistent VEREVON target while the intro owns the frame", () => {
		act(() => root.render(
			<Navbar
				isHidden={false}
				isIntroActive
				isMenuOpen={false}
				isOnDark
				isScrolled={false}
				menuButtonRef={{ current: null }}
				onOpen={() => {}}
			/>,
		));
		const brand = container.querySelector<HTMLAnchorElement>("[data-site-brand-target]");
		expect(brand?.getAttribute("href")).toBe("/");
		expect(brand?.getAttribute("aria-label")).toBe("Verevon hjem");
		expect(brand?.hasAttribute("data-intro-hidden")).toBe(true);
		expect(brand?.querySelector("svg")).toBeNull();
		expect(brand?.textContent).toContain("VEREVON");
		expect(brand?.classList.contains("verevon-wordmark")).toBe(true);
		expect(brand?.classList.contains("tracking-[0.08em]")).toBe(true);
	});

	it("uses a quiet Stingray-style nav and marks the actual route", () => {
		act(() => root.render(
			<Navbar
				isMenuOpen={false}
				isOnDark={false}
				isScrolled
				menuButtonRef={{ current: null }}
				onOpen={() => {}}
			/>,
		));
		const navigation = container.querySelector('[aria-label="Hovednavigasjon"]');
		expect(navigation?.getAttribute("data-navigation-style")).toBe("stingray");
		expect(navigation?.querySelectorAll('[data-navigation-link="stingray"]')).toHaveLength(4);
		expect(navigation?.querySelector('[aria-current="page"]')?.textContent).toBe("Produkt");
	});

	it("keeps the navigation surface transparent until the scroll fade is active", () => {
		act(() => root.render(
			<Navbar
				isMenuOpen={false}
				isOnDark={false}
				isScrolled={false}
				menuButtonRef={{ current: null }}
				onOpen={() => {}}
			/>,
		));

		const surface = container.querySelector("[data-site-nav-surface]");
		expect(surface?.getAttribute("data-scrolled")).toBe("false");

		act(() => root.render(
			<Navbar
				isMenuOpen={false}
				isOnDark={false}
				isScrolled
				menuButtonRef={{ current: null }}
				onOpen={() => {}}
			/>,
		));

		expect(surface?.getAttribute("data-scrolled")).toBe("true");
	});

	it("uses the hero's white, dimensional navigation treatment before the fade", () => {
		act(() => root.render(
			<Navbar
				isMenuOpen={false}
				isOnDark={false}
				isScrolled={false}
				menuButtonRef={{ current: null }}
				onOpen={() => {}}
			/>,
		));

		const header = container.querySelector("[data-site-nav]");
		const menuButton = container.querySelector<HTMLButtonElement>('button[aria-label="Åpne meny"]');

		expect(header?.classList.contains("text-verevon-c-white")).toBe(true);
		expect(header?.classList.contains("verevon-hero-nav-shadow")).toBe(true);
		expect(menuButton?.className).toContain("opacity-90");
	});

	it("switches the navigation state without a colour-transition delay and keeps Dashboard light", () => {
		act(() => root.render(
			<Navbar
				isMenuOpen={false}
				isOnDark={false}
				isScrolled
				menuButtonRef={{ current: null }}
				onOpen={() => {}}
			/>,
		));

		const header = container.querySelector("[data-site-nav]");
		const dashboard = container.querySelector<HTMLAnchorElement>('[data-dashboard-link]');

		expect(header?.className).not.toContain("transition-[color,opacity]");
		expect(dashboard?.classList.contains("bg-white")).toBe(false);
		expect(dashboard?.classList.contains("text-verevon-j-text")).toBe(true);
	});
});
