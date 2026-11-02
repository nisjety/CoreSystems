import { NORWEGIAN_CARD_INTEGRATIONS } from "../../integrations/integration-catalog";

export type LiveConnector = {
	name: string;
	logo: string | null;
	kind: "connector" | "lookup";
};

export type SystemFieldItem = {
	id: string;
	name: string;
	logo: string | null;
	mark?: string;
	kind: "connector" | "lookup" | "system";
};

// Keep this list aligned with the verified live set in VEREVON.md. Other marks
// elsewhere on the site may illustrate use cases, not shipped integrations.
export const LIVE_CONNECTORS: readonly LiveConnector[] = [
	{ name: "Slack", logo: "slack.svg", kind: "connector" },
	{ name: "Gmail", logo: "gmail.svg", kind: "connector" },
	{ name: "Notion", logo: "notion.svg", kind: "connector" },
	{ name: "SharePoint", logo: "sharepoint.svg", kind: "connector" },
	{ name: "OneDrive", logo: "onedrive.svg", kind: "connector" },
	{ name: "Outlook", logo: "outlook.svg", kind: "connector" },
	{ name: "Enhetsregisteret", logo: null, kind: "lookup" },
];

// The expanded field presents familiar systems as context, while direct
// Verevon connections remain limited to LIVE_CONNECTORS above. Text marks are
// intentional for public services whose official logo artwork is not licensed
// for this marketing surface.
export const SYSTEM_FIELD_ITEMS: readonly SystemFieldItem[] = NORWEGIAN_CARD_INTEGRATIONS.map((integration) => ({
	id: integration.id,
	name: integration.name,
	logo: integration.logo ?? null,
	mark: integration.mark,
	kind: integration.availability === "public-source" ? "lookup" : "system",
}));

export function getConnectionSelection(name: string | null): string {
	if (!name) return "Velg et system for å se tilkoblingen";
	const connector = LIVE_CONNECTORS.find((item) => item.name === name);
	return connector ? `${connector.name} · ${connector.kind === "lookup" ? "oppslag" : "tilkobling"}` : "Velg et system for å se tilkoblingen";
}

export function getSystemFieldSelection(name: string | null): string {
	if (!name) return "Norske systemer og registre";
	const system = SYSTEM_FIELD_ITEMS.find((item) => item.name === name);
	if (!system) return "Norske systemer og registre";
	const context = system.kind === "connector" ? "tilkobling" : system.kind === "lookup" ? "oppslag" : "systemeksempel";
	return `${system.name} · ${context}`;
}

