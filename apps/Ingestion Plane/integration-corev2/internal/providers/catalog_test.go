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

func TestGitHubCatalogCoversRepositoryOperations(t *testing.T) {
	github := GitHub()
	fullScopes := ResolveScopes(github, ResolveCapabilities(github, nil, []string{"full"}))
	for _, want := range []string{"read:user", "user:email", "read:org", "public_repo", "repo"} {
		if !slices.Contains(fullScopes, want) {
			t.Fatalf("github full scopes = %v, missing %s", fullScopes, want)
		}
	}
	knowledgeCaps := ResolveCapabilities(github, nil, []string{"knowledge"})
	for _, want := range []string{"repo.contents.read", "commits.read", "pulls.read", "issues.read"} {
		if !slices.Contains(knowledgeCaps, want) {
			t.Fatalf("github knowledge capabilities = %v, missing %s", knowledgeCaps, want)
		}
	}
}

func TestLinkedInCatalogCoversProductScopedOperations(t *testing.T) {
	linkedin := LinkedIn()

	onboardingScopes := ResolveScopes(linkedin, ResolveCapabilities(linkedin, nil, []string{"onboarding"}))
	for _, want := range []string{"openid", "profile", "email"} {
		if !slices.Contains(onboardingScopes, want) {
			t.Fatalf("linkedin onboarding scopes = %v, missing %s", onboardingScopes, want)
		}
	}

	publishingScopes := ResolveScopes(linkedin, ResolveCapabilities(linkedin, nil, []string{"publishing"}))
	if !slices.Contains(publishingScopes, "w_member_social") {
		t.Fatalf("linkedin publishing scopes = %v, missing w_member_social", publishingScopes)
	}

	verificationScopes := ResolveScopes(linkedin, ResolveCapabilities(linkedin, nil, []string{"verification"}))
	for _, want := range []string{"r_profile_basicinfo", "r_verify"} {
		if !slices.Contains(verificationScopes, want) {
			t.Fatalf("linkedin verification scopes = %v, missing %s", verificationScopes, want)
		}
	}

	adsScopes := ResolveScopes(linkedin, ResolveCapabilities(linkedin, nil, []string{"ads"}))
	for _, want := range []string{"r_ads", "rw_ads"} {
		if !slices.Contains(adsScopes, want) {
			t.Fatalf("linkedin ads scopes = %v, missing %s", adsScopes, want)
		}
	}

	leadScopes := ResolveScopes(linkedin, ResolveCapabilities(linkedin, nil, []string{"lead_sync"}))
	if !slices.Contains(leadScopes, "r_marketing_leadgen_automation") {
		t.Fatalf("linkedin lead sync scopes = %v, missing r_marketing_leadgen_automation", leadScopes)
	}

	conversionScopes := ResolveScopes(linkedin, ResolveCapabilities(linkedin, nil, []string{"conversions"}))
	for _, want := range []string{"r_ads", "rw_conversions"} {
		if !slices.Contains(conversionScopes, want) {
			t.Fatalf("linkedin conversion scopes = %v, missing %s", conversionScopes, want)
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
	// TikTok's Login Kit v2 flow (client_key naming) is implemented — it is a
	// full OAuth provider now, gated only by TIKTOK_CLIENT_KEY/SECRET readiness.
	tiktok, ok := Find("tiktok")
	if !ok {
		t.Fatalf("catalog missing social provider %q", "tiktok")
	}
	if tiktok.Category != "social" {
		t.Fatalf("tiktok category = %q, want social", tiktok.Category)
	}
	if !tiktok.DirectOAuthReady {
		t.Fatalf("tiktok should be direct OAuth ready now that the Login Kit v2 flow is implemented")
	}
	if _, ok := FindOAuth("tiktok"); !ok {
		t.Fatalf("tiktok should be available through OAuth connect sessions")
	}
	tiktokOnboardingScopes := ResolveScopes(tiktok, ResolveCapabilities(tiktok, nil, []string{"onboarding"}))
	if !slices.Contains(tiktokOnboardingScopes, "user.info.basic") {
		t.Fatalf("tiktok onboarding scopes = %v, want user.info.basic", tiktokOnboardingScopes)
	}
	tiktokPublishingScopes := ResolveScopes(tiktok, ResolveCapabilities(tiktok, nil, []string{"publishing"}))
	for _, want := range []string{"video.publish", "video.upload"} {
		if !slices.Contains(tiktokPublishingScopes, want) {
			t.Fatalf("tiktok publishing scopes = %v, missing %s", tiktokPublishingScopes, want)
		}
	}

	// The unified Meta provider supersedes the four separate Meta-family
	// providers; superseded entries must stay resolvable for existing
	// connections but point at "meta".
	meta, ok := FindOAuth("meta")
	if !ok {
		t.Fatalf("catalog missing unified meta provider")
	}
	if meta.SupersededBy != "" {
		t.Fatalf("meta must not be superseded, got %q", meta.SupersededBy)
	}
	metaCaps := ResolveCapabilities(meta, nil, []string{"ads"})
	if !slices.Contains(metaCaps, "social.ads.manage") {
		t.Fatalf("meta ads bundle should resolve social.ads.manage, got %v", metaCaps)
	}
	metaFullScopes := ResolveScopes(meta, ResolveCapabilities(meta, nil, []string{"full"}))
	for _, want := range []string{
		"pages_messaging",
		"whatsapp_business_messaging",
		"ads_management",
		"catalog_management",
		"threads_basic",
		"threads_content_publish",
		"pages_manage_posts",
	} {
		if !slices.Contains(metaFullScopes, want) {
			t.Fatalf("meta full scopes = %v, missing %s", metaFullScopes, want)
		}
	}
	for _, superseded := range []string{"facebook", "instagram", "whatsapp", "meta-ads"} {
		provider, ok := Find(superseded)
		if !ok {
			t.Fatalf("superseded provider %q missing from catalog", superseded)
		}
		if provider.SupersededBy != "meta" {
			t.Fatalf("%s should be superseded by meta, got %q", superseded, provider.SupersededBy)
		}
	}

	// Discord is a plain OAuth provider (identity + guilds; no messaging
	// claims without a bot).
	discord, ok := FindOAuth("discord")
	if !ok {
		t.Fatalf("catalog missing discord provider")
	}
	if !discord.DirectOAuthReady {
		t.Fatalf("discord should be direct OAuth ready")
	}

	// Google gains an ads bundle carrying the adwords scope.
	google, _ := Find("google")
	googleAdsScopes := ResolveScopes(google, ResolveCapabilities(google, nil, []string{"ads"}))
	if !slices.Contains(googleAdsScopes, "https://www.googleapis.com/auth/adwords") {
		t.Fatalf("google ads bundle should resolve the adwords scope, got %v", googleAdsScopes)
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
