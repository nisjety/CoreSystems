/**
 * Vendors and recipients that the egress scan finds and that have no row on the Trust Center
 * sub-processor table, each with the register's reason (docs/compliance/evidence/subprocessors-from-code.md,
 * disposition EXCLUDED). Every vendor the scan finds is either on a row of `subprocessors` (through
 * its `vendorIds`) or here, so a vendor cannot arrive in the code without a decision about the page.
 *
 * Not rendered anywhere. trust-data.test.ts checks it against the scan and the register: a vendor the
 * register proposes to LIST must be on the page, so this list holds only exclusions.
 */
export type RecipientNotListed = {
	id: string;
	status: "excluded";
	reason: string;
};

/**
 * Vendors the register excludes but the page names anyway, with the reason. The ChatGPT-account route is
 * the one case: the register classes it as user-authorised, but the whole prompt goes to OpenAI on it,
 * so the page discloses it (docs/compliance/evidence/honest-limits.md, L16 to L19).
 */
export const namedDespiteExclusion: { id: string; reason: string }[] = [
	{
		id: "openai-chatgpt-subscription",
		reason:
			"the register excludes it as user-authorised, but the whole prompt (history, memory, retrieved sources, tool results) is sent to OpenAI on this route, so the page names it and says it is global and has no ZDR",
	},
];

export const recipientsNotListed: RecipientNotListed[] = [
	{
		id: "abuse-ch-feeds",
		status: "excluded",
		reason: "public blocklist download, only the request leaves, no customer data",
	},
	{
		id: "apple-sign-in",
		status: "excluded",
		reason: "sign-in with the user's own Apple account; Apple is an independent controller of that identity",
	},
	{
		id: "browserless",
		status: "excluded",
		reason: "a driver in a library crate that the edge binary does not register; named only in comments",
	},
	{
		id: "brreg",
		status: "excluded",
		reason: "public register read, the request is the only thing that leaves; the trust page already cites it as a grounding source",
	},
	{
		id: "crm-erp-import-sources",
		status: "excluded",
		reason: "customer-directed import from the customer's own system, supplied per import; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "discord-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "eu-procurement",
		status: "excluded",
		reason: "public procurement data read; only the query leaves",
	},
	{
		id: "geofabrik",
		status: "excluded",
		reason: "bulk download of public map data, nothing sent",
	},
	{
		id: "github-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "google-accounts-apis",
		status: "excluded",
		reason: "sign-in and connectors on the user's own Google account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "hibp-pwned-passwords",
		status: "excluded",
		reason: "five-character hash prefix with padding, not linkable to a person without the password; counsel may still prefer to list it",
	},
	{
		id: "hugging-face-hub",
		status: "excluded",
		reason: "model weights downloaded at container start, nothing about customers sent",
	},
	{
		id: "linkedin-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "marketing-site-assets",
		status: "excluded",
		reason: "recipients of the marketing sites' own visitors (Verevon is controller there), for the privacy notice, not the Art. 28 list",
	},
	{
		id: "meta-platforms",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "microsoft-365-entra-graph",
		status: "excluded",
		reason: "sign-in and connectors on the user's own Microsoft account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "no-news-feeds",
		status: "excluded",
		reason: "public news feeds read, only the request leaves",
	},
	{
		id: "no-public-authorities",
		status: "excluded",
		reason: "public data APIs read, only the request leaves",
	},
	{
		id: "notion-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "okta-oidc",
		status: "excluded",
		reason: "sign-in with the customer's own identity provider; it is the customer's system",
	},
	{
		id: "osm-nominatim",
		status: "excluded",
		reason: "place-name geocoding with the query text only; counsel may prefer to list it because the public instance is an independent recipient",
	},
	{
		id: "s3-compatible-object-storage",
		status: "excluded",
		reason: "the S3 client libraries talk to MinIO in the compose files; a cloud object store in production would be a sub-processor and is UNVERIFIED",
	},
	{
		id: "shopify",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "slack-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "snapchat-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "stock-media",
		status: "excluded",
		reason: "recipients of the public sites' own visitors (Verevon is controller there), for the privacy notice, not the Art. 28 list",
	},
	{
		id: "stripe-connect",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "superoffice",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "tiktok-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "unpkg-cdn",
		status: "excluded",
		reason: "developer documentation pages, no customer content, visitor address only; self-hosting the assets would remove it",
	},
	{
		id: "vipps-login",
		status: "excluded",
		reason: "sign-in with the user's own Vipps account; Vipps is an independent controller of that identity",
	},
	{
		id: "visma-net",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
	{
		id: "x-api",
		status: "excluded",
		reason: "connector the customer authorises on their own account; disclose as a recipient class, not a sub-processor",
	},
];
