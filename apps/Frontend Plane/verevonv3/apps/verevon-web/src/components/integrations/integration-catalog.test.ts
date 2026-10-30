import { describe, expect, it } from "vitest";
import {
	DEFAULT_CARD_INTEGRATIONS,
	INTEGRATION_CATALOG,
	NORWEGIAN_CARD_INTEGRATIONS,
} from "./integration-catalog";

const userProvidedIntegrations = [
	"Shopify", "WooCommerce", "Meta", "Slack", "AfterShip", "Gorgias", "HubSpot", "Helthjem", "Mailchimp", "Notion",
	"PostNord", "Zendesk", "Tripletex", "Packiyo", "ShipHero", "Bring", "Azure", "Ingrid", "SuperOffice", "Microsoft Dynamics",
	"Microsoft SharePoint", "Microsoft Exchange", "Jira", "Microsoft Teams", "Lovdata", "Doffin", "SSB", "Brønnøysundregistrene",
	"Zapier", "Visma Net", "Huma", "Xledger", "PowerOffice", "Fiken", "Google Drive", "Visma Business", "Dropbox", "Flowcase",
	"Sveve", "SCB", "Xero", "Odoo", "Documaster", "Gmail", "Outlook", "GitLab", "Google Cloud Portal", "Trello", "Google Calendar",
];

describe("integration catalogue", () => {
	it("includes every user-provided integration exactly once with an explanation of the context it provides", () => {
		const names = INTEGRATION_CATALOG.map(({ name }) => name);
		expect(names).toEqual(expect.arrayContaining(userProvidedIntegrations));
		expect(new Set(INTEGRATION_CATALOG.map(({ id }) => id)).size).toBe(INTEGRATION_CATALOG.length);
		for (const integration of INTEGRATION_CATALOG) {
			expect(integration.provides.trim(), integration.name).not.toBe("");
			expect(Boolean(integration.logo) || Boolean(integration.mark), integration.name).toBe(true);
		}
	});

	it("covers the operational domains businesses need", () => {
		expect(new Set(INTEGRATION_CATALOG.map(({ category }) => category))).toEqual(new Set([
			"commerce", "collaboration", "customer", "microsoft", "public-data", "finance", "developer", "knowledge",
		]));
	});

	it("makes the maturity of each connection explicit", () => {
		const byId = new Map(INTEGRATION_CATALOG.map((integration) => [integration.id, integration]));
		expect(byId.get("slack")?.availability).toBe("available");
		expect(byId.get("enhetsregisteret")?.availability).toBe("public-source");
		expect(byId.get("bronnoysundregistrene")?.availability).toBe("system-example");
		expect(byId.get("ssb")?.availability).toBe("system-example");
		expect(byId.get("visma-net")?.availability).toBe("pilot");
		expect(byId.get("zendesk")?.availability).toBe("planned");
		expect(byId.get("shopify")?.availability).toBe("system-example");
	});

	it("keeps the compact homepage surface global and the expanded surface Norwegian", () => {
		expect(INTEGRATION_CATALOG.map(({ name }) => name)).toEqual(expect.arrayContaining(["Flowcase", "Documaster"]));
		expect(DEFAULT_CARD_INTEGRATIONS.map(({ name }) => name)).toEqual(expect.arrayContaining([
			"Slack", "Shopify", "HubSpot", "Google Drive", "Microsoft 365", "Jira",
		]));
		expect(DEFAULT_CARD_INTEGRATIONS.map(({ name }) => name)).not.toEqual(expect.arrayContaining([
			"Lovdata", "Doffin", "SSB", "Fiken", "Tripletex",
		]));

		expect(NORWEGIAN_CARD_INTEGRATIONS.map(({ name }) => name)).toEqual(expect.arrayContaining([
			"Lovdata", "Doffin", "Altinn", "SSB", "Brønnøysundregistrene", "Sveve", "Visma Net", "Visma Business",
			"Tripletex", "Huma", "Xledger", "PowerOffice", "Fiken", "SuperOffice",
			"Bring", "PostNord", "Helthjem", "Ingrid",
		]));
		expect(NORWEGIAN_CARD_INTEGRATIONS.map(({ name }) => name)).not.toEqual(expect.arrayContaining([
			"Slack", "Shopify", "Google Drive", "Jira",
		]));
		expect(NORWEGIAN_CARD_INTEGRATIONS.map(({ name }) => name)).not.toContain("Flowcase");
		expect(NORWEGIAN_CARD_INTEGRATIONS.map(({ name }) => name)).not.toContain("Documaster");
	});
});
