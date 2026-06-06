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
