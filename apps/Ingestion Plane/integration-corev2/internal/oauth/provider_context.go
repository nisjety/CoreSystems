package oauth

import (
	"fmt"
	"regexp"
	"strings"
)

var shopifyShopPattern = regexp.MustCompile(`^[a-z0-9][a-z0-9-]*\.myshopify\.com$`)

var serverDerivedProviderContextKeys = map[string]struct{}{
	"webhook_account_ids": {},
	"guild_id":            {},
	"team_id":             {},
	"tenant_id":           {},
}

func NormalizeProviderContext(providerKey string, input map[string]string) (map[string]string, error) {
	context := map[string]string{}
	for key, value := range input {
		key = strings.TrimSpace(strings.ToLower(key))
		value = strings.TrimSpace(value)
		if _, reserved := serverDerivedProviderContextKeys[key]; reserved {
			continue
		}
		if key != "" && value != "" {
			context[key] = value
		}
	}
	if providerKey == "shopify" {
		shop, err := normalizeShopifyShop(firstNonEmpty(context["shop"], context["shop_domain"], context["shopdomain"]))
		if err != nil {
			return nil, err
		}
		context["shop"] = shop
	}
	return context, nil
}

func ShopifyShop(providerContext map[string]string) (string, error) {
	shop, err := normalizeShopifyShop(providerContext["shop"])
	if err != nil {
		return "", err
	}
	return shop, nil
}

func normalizeShopifyShop(raw string) (string, error) {
	shop := strings.TrimSpace(strings.ToLower(raw))
	shop = strings.TrimPrefix(shop, "https://")
	shop = strings.TrimPrefix(shop, "http://")
	shop = strings.TrimSuffix(shop, "/")
	if shop != "" && !strings.Contains(shop, ".") {
		shop += ".myshopify.com"
	}
	if !shopifyShopPattern.MatchString(shop) {
		return "", fmt.Errorf("shopify providerContext.shop must be a valid *.myshopify.com domain")
	}
	return shop, nil
}
