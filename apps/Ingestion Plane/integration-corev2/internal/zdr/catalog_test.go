package zdr

import (
	"testing"

	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/providers"
)

// A contract row that no real connection can match is dead data, and a model provider with no
// row silently falls to cannot_verify. Both are decisions someone must make on purpose, so the
// provider catalog and the contract are held to each other here.

func TestEveryContractRowDescribesARealProviderAndItsAuthType(t *testing.T) {
	catalog := map[string]providers.Provider{}
	for _, provider := range providers.Catalog() {
		catalog[provider.Key] = provider
	}
	for _, row := range mustDefault(t).Providers() {
		provider, ok := catalog[row.ProviderKey]
		if !ok {
			t.Errorf("contract row %s names provider_key %q, which is not in the provider catalog", row.BasisRef, row.ProviderKey)
			continue
		}
		if provider.AuthType != row.AuthMode {
			t.Errorf("contract row %s has auth_mode %q but the catalog's auth type for %q is %q, so no real connection would ever match it",
				row.BasisRef, row.AuthMode, row.ProviderKey, provider.AuthType)
		}
		if provider.Category != "ai" {
			t.Errorf("contract row %s is for provider %q of category %q; only model (ai) providers have a ZDR capability", row.BasisRef, row.ProviderKey, provider.Category)
		}
	}
}

func TestEveryModelProviderOfTheCatalogHasAnExplicitContractRow(t *testing.T) {
	assessor := mustDefault(t)
	for _, provider := range providers.Catalog() {
		if provider.Category != "ai" {
			continue
		}
		got, err := assessor.Assess(provider.Key, provider.AuthType)
		if err != nil {
			t.Fatalf("%s: %v", provider.Key, err)
		}
		if got.BasisRef == assessor.doc.Fallback.BasisRef {
			t.Errorf("model provider %q (%s) has no row in connection-zdr-capabilities.json and would be assessed as the fallback; add a row (or consciously accept cannot_verify there)", provider.Key, provider.AuthType)
		}
	}
}

func TestTheSubscriptionConnectionIsAssessedUnderTheKeysItIsStoredWith(t *testing.T) {
	provider, ok := providers.Find(codexsubscription.ProviderKey)
	if !ok {
		t.Fatalf("provider %q is not in the catalog", codexsubscription.ProviderKey)
	}
	got, err := mustDefault(t).Assess(codexsubscription.ProviderKey, provider.AuthType)
	if err != nil {
		t.Fatalf("Assess: %v", err)
	}
	if got.Capability != CapabilityNotSupported || got.BasisRef != "openai-codex-subscription@1" {
		t.Fatalf("the ChatGPT subscription is assessed as %+v under (%q, %q)", got, codexsubscription.ProviderKey, provider.AuthType)
	}
}
