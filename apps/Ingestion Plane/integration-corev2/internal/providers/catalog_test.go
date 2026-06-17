package providers

import (
	"slices"
	"testing"
)

func TestResolveScopesIncludesInboxMailPermissions(t *testing.T) {
	provider := Microsoft()
	capabilities := ResolveCapabilities(provider, nil, []string{"inbox"})
	scopes := ResolveScopes(provider, capabilities)

	for _, want := range []string{"Mail.Read", "Mail.Send", "offline_access", "User.Read"} {
		if !slices.Contains(scopes, want) {
			t.Fatalf("scopes = %#v, missing %s", scopes, want)
		}
	}
}

func TestDefaultBundleIsSafeOnboarding(t *testing.T) {
	provider := Microsoft()
	capabilities := ResolveCapabilities(provider, nil, nil)

	if slices.Contains(capabilities, "mail.read") || slices.Contains(capabilities, "mail.send") {
		t.Fatalf("default capabilities = %#v, should not include mail", capabilities)
	}
	if !slices.Contains(capabilities, "sharepoint.read") {
		t.Fatalf("default capabilities = %#v, want sharepoint.read", capabilities)
	}
}

func TestCatalogIncludesTargetFirstPartyProviders(t *testing.T) {
	catalog := Catalog()
	for _, want := range []string{"microsoft", "slack", "google", "notion", "github", "shopify"} {
		if !slices.ContainsFunc(catalog, func(provider Provider) bool { return provider.Key == want }) {
			t.Fatalf("catalog missing provider %q: %#v", want, catalog)
		}
	}
}

func TestCatalogProvidersExposeCategoryForUIGrouping(t *testing.T) {
	for _, provider := range Catalog() {
		if provider.Category == "" {
			t.Fatalf("%s category is empty", provider.Key)
		}
	}
}

func TestOAuthCatalogProvidersAreDirectOAuthReady(t *testing.T) {
	for _, provider := range OAuthCatalog() {
		if !provider.DirectOAuthReady {
			t.Fatalf("%s should be direct OAuth ready", provider.Key)
		}
	}
}

func TestCatalogIncludesAdminAndInboundProviders(t *testing.T) {
	for _, want := range []string{"okta", "scim", "stripe"} {
		if !slices.ContainsFunc(Catalog(), func(provider Provider) bool { return provider.Key == want }) {
			t.Fatalf("catalog missing provider %q", want)
		}
	}
	if _, ok := FindOAuth("scim"); ok {
		t.Fatalf("scim should not be available through OAuth connect sessions")
	}
	if _, ok := FindOAuth("okta"); ok {
		t.Fatalf("okta should not be available through user OAuth connect sessions")
	}
}

func TestCatalogIncludesSocialProvidersWithStagedOAuthReadiness(t *testing.T) {
	for _, want := range []string{"linkedin", "x", "instagram", "facebook", "snapchat"} {
		provider, ok := Find(want)
		if !ok {
			t.Fatalf("catalog missing social provider %q", want)
		}
		if provider.Category != "social" {
			t.Fatalf("%s category = %q, want social", want, provider.Category)
		}
		if !provider.DirectOAuthReady {
			t.Fatalf("%s should be direct OAuth ready once credentials are configured", want)
		}
		if _, ok := FindOAuth(want); !ok {
			t.Fatalf("%s should be available through OAuth connect sessions", want)
		}
	}
	snapchat, _ := Find("snapchat")
	if slices.Contains(ResolveCapabilities(snapchat, nil, []string{"ads"}), "social.post.write") {
		t.Fatalf("snapchat ads bundle should not imply organic post publishing")
	}
	tiktok, ok := Find("tiktok")
	if !ok {
		t.Fatalf("catalog missing social provider %q", "tiktok")
	}
	if tiktok.Category != "social" {
		t.Fatalf("tiktok category = %q, want social", tiktok.Category)
	}
	if tiktok.DirectOAuthReady {
		t.Fatalf("tiktok should stay catalog-only until its provider-specific token flow lands")
	}
	if _, ok := FindOAuth("tiktok"); ok {
		t.Fatalf("tiktok should not be available through OAuth connect sessions yet")
	}
}

func TestProviderReadinessMarksMissingCredentials(t *testing.T) {
	catalog := WithReadiness(Catalog(), map[string][]string{
		"stripe": {"STRIPE_CLIENT_ID"},
		"scim":   {"SCIM_BEARER_TOKEN"},
	})
	stripe, ok := findProvider(catalog, "stripe")
	if !ok {
		t.Fatalf("stripe missing from readiness catalog")
	}
	if stripe.Configured || stripe.Status != "missing_config" {
		t.Fatalf("stripe readiness = configured:%v status:%s, want missing_config", stripe.Configured, stripe.Status)
	}
	scim, ok := findProvider(catalog, "scim")
	if !ok {
		t.Fatalf("scim missing from readiness catalog")
	}
	if scim.Configured || scim.Status != "missing_config" {
		t.Fatalf("scim readiness = configured:%v status:%s, want missing_config", scim.Configured, scim.Status)
	}
}

func TestSafeOnboardingBundlesAvoidSensitiveCapabilities(t *testing.T) {
	for _, provider := range Catalog() {
		capabilities := ResolveCapabilities(provider, nil, []string{"onboarding"})
		capabilityByKey := map[string]Capability{}
		for _, capability := range provider.Capabilities {
			capabilityByKey[capability.Key] = capability
		}
		for _, key := range capabilities {
			if capabilityByKey[key].Sensitive {
				t.Fatalf("%s onboarding capability %q is sensitive", provider.Key, key)
			}
		}
	}
}

func findProvider(catalog []Provider, key string) (Provider, bool) {
	for _, provider := range catalog {
		if provider.Key == key {
			return provider, true
		}
	}
	return Provider{}, false
}

func TestGoogleKnowledgeBundleUsesDriveReadScope(t *testing.T) {
	provider := GoogleWorkspace()
	capabilities := ResolveCapabilities(provider, nil, []string{"knowledge"})
	scopes := ResolveScopes(provider, capabilities)

	if !slices.Contains(scopes, "https://www.googleapis.com/auth/drive.readonly") {
		t.Fatalf("scopes = %#v, want drive.readonly", scopes)
	}
	if slices.Contains(scopes, "https://www.googleapis.com/auth/gmail.send") {
		t.Fatalf("knowledge scopes = %#v, should not include gmail.send", scopes)
	}
}
