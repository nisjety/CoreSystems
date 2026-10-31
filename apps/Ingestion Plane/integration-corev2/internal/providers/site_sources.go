package providers

func Cookiebot() Provider {
	return Provider{Key: "cookiebot", Label: "Cookiebot", Category: "marketing", ConnectorType: "cookiebot", AuthType: "api_key", DirectOAuthReady: false, Capabilities: []Capability{{Key: "consent.analytics.read", Label: "Aggregate consent events", Description: "Read aggregate consent events for an explicitly mapped website."}, {Key: "cookie.inventory.read", Label: "Cookie scan summary", Description: "Read aggregate cookie category and scan counts; never cookie values or consent records."}}, Bundles: []Bundle{{Key: "reporting", Label: "Read-only website reports", Description: "Manual, organization/site-bound Data API key; no consent or installation changes.", Capabilities: []string{"consent.analytics.read", "cookie.inventory.read"}}}}
}
func Algolia() Provider {
	return Provider{Key: "algolia", Label: "Algolia", Category: "marketing", ConnectorType: "algolia", AuthType: "oauth2_authorization_code_pkce", DirectOAuthReady: true, Capabilities: []Capability{{Key: "search.analytics.read", Label: "Search aggregate analytics", Description: "Read aggregate analytics for an explicitly mapped existing index through user-scoped Productivity MCP.", Scopes: []string{"public"}}}, Bundles: []Bundle{{Key: "reporting", Label: "Read-only search analytics", Description: "User-authorized Productivity MCP; restricted analytics-key fallback remains separately site-bound.", Capabilities: []string{"search.analytics.read"}}}}
}
