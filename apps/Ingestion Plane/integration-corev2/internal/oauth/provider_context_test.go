package oauth

import "testing"

func TestNormalizeProviderContextShopifyShop(t *testing.T) {
	context, err := NormalizeProviderContext("shopify", map[string]string{"shop": "Velion-Test"})
	if err != nil {
		t.Fatalf("NormalizeProviderContext error: %v", err)
	}
	if context["shop"] != "velion-test.myshopify.com" {
		t.Fatalf("shop = %q, want velion-test.myshopify.com", context["shop"])
	}
}

func TestNormalizeProviderContextRejectsInvalidShopifyShop(t *testing.T) {
	_, err := NormalizeProviderContext("shopify", map[string]string{"shop": "https://evil.example.com"})
	if err == nil {
		t.Fatalf("expected invalid shopify shop error")
	}
}

func TestNormalizeProviderContextStripsServerDerivedTenantBindings(t *testing.T) {
	context, err := NormalizeProviderContext("meta", map[string]string{
		"webhook_account_ids": "victim-page", "guild_id": "victim-guild", "business_login_config": "default",
	})
	if err != nil {
		t.Fatalf("NormalizeProviderContext: %v", err)
	}
	if context["webhook_account_ids"] != "" || context["guild_id"] != "" {
		t.Fatalf("server-derived bindings survived normalization: %+v", context)
	}
	if context["business_login_config"] != "default" {
		t.Fatalf("legitimate Meta context was removed: %+v", context)
	}
}
