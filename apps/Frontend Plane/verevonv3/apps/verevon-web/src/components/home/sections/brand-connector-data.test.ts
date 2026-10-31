import { describe, expect, it } from "vitest";
import {
	LIVE_CONNECTORS,
	SYSTEM_FIELD_ITEMS,
	getConnectionSelection,
	getSystemFieldSelection,
} from "./brand-connector-data";

describe("live connector presentation", () => {
	it("uses only the verified connector and lookup set in the interactive card", () => {
		expect(LIVE_CONNECTORS.map(({ name }) => name)).toEqual([
			"Slack", "Gmail", "Notion", "SharePoint", "OneDrive", "Outlook", "Enhetsregisteret",
		]);
	});

	it("returns a clear, non-claiming selection caption", () => {
		expect(getConnectionSelection("Slack")).toBe("Slack · tilkobling");
		expect(getConnectionSelection(null)).toBe("Velg et system for å se tilkoblingen");
	});

	it("keeps the Norwegian system landscape distinct from the live set", () => {
		expect(SYSTEM_FIELD_ITEMS.map(({ name }) => name)).toEqual(expect.arrayContaining([
			"Lovdata",
			"Doffin",
			"Brønnøysundregistrene",
			"Visma Net",
			"SuperOffice",
			"Tripletex",
		]));
		expect(getSystemFieldSelection("Tripletex")).toBe("Tripletex · systemeksempel");
		expect(getSystemFieldSelection("Brønnøysundregistrene")).toBe("Brønnøysundregistrene · systemeksempel");
		expect(getSystemFieldSelection(null)).toBe("Norske systemer og registre");
	});
});
