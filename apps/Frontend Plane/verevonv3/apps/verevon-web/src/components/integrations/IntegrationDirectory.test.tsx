import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { INTEGRATION_CATALOG } from "./integration-catalog";
import { IntegrationDirectory } from "./IntegrationDirectory";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function renderDirectory(): HTMLElement {
	const root = document.createElement("div");
	document.body.append(root);
	act(() => {
		createRoot(root).render(<IntegrationDirectory />);
	});
	return root;
}

describe("IntegrationDirectory", () => {
	it("renders the full catalogue with an accessible category filter", () => {
		const root = renderDirectory();
		expect(root.querySelectorAll("[data-integration-card]")).toHaveLength(INTEGRATION_CATALOG.length);
		expect(root.querySelector("fieldset")).not.toBeNull();
		expect(root.querySelector("input[type='radio']:checked")?.value).toBe("all");
		for (const integration of INTEGRATION_CATALOG) {
			expect(root.textContent).toContain(integration.name);
			expect(root.textContent).toContain(integration.provides);
		}
	});

	it("filters to public sources and lets people search by system name", () => {
		const root = renderDirectory();
		const publicDataRadio = root.querySelector<HTMLInputElement>("#integration-filter-public-data");
		expect(publicDataRadio).not.toBeNull();
		act(() => publicDataRadio?.click());
		expect(root.textContent).toContain("Lovdata");
		expect(root.textContent).toContain("Doffin");
		expect(root.textContent).not.toContain("Shopify");

		const search = root.querySelector<HTMLInputElement>("input[type='search']");
		expect(search).not.toBeNull();
		act(() => {
			if (!search) return;
			search.value = "SSB";
			search.dispatchEvent(new Event("input", { bubbles: true }));
		});
		expect(root.textContent).toContain("SSB");
		expect(root.textContent).not.toContain("Lovdata");
	});

	it("keeps the selected context panel in the catalogue column while a system is selected", () => {
		const root = renderDirectory();
		const contextPanel = root.querySelector("[data-integration-context]");
		expect(contextPanel).not.toBeNull();
		expect(contextPanel?.className).toContain("xl:sticky");
		expect(contextPanel?.textContent).toContain("Shopify");
		expect(contextPanel?.textContent).toContain("Slik bruker dere konteksten");
		expect(contextPanel?.textContent).toContain("tilgjengelighet og omfang avklares");
	});

	it("selects and filters the requested integration from the catalogue URL", () => {
		window.history.replaceState({}, "", "/integrasjoner?integration=helthjem#katalog");
		const root = renderDirectory();
		const selectedCard = root.querySelector<HTMLElement>("[data-integration-card][data-selected='true']");

		expect(selectedCard?.dataset.integrationId).toBe("helthjem");
		expect(root.querySelector("input[type='radio']:checked")?.getAttribute("value")).toBe("commerce");
		expect(root.querySelector("[data-integration-context]")?.textContent).toContain("Helthjem");
		window.history.replaceState({}, "", "/");
	});
});
