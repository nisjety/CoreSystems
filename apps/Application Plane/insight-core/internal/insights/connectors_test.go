package insights

import (
	"slices"
	"strings"
	"testing"
)

func TestDefaultConnectorSlotsRegistersAllMarketingSources(t *testing.T) {
	connectors := DefaultConnectorSlots(ConnectorSlotOptions{TokenLeaseAudience: "insight-core"})
	want := map[string]string{
		"google_search_console": "Google Search Console",
		"google_analytics_4":    "Google Analytics 4",
		"google_ads":            "Google Ads",
		"meta_ads":              "Meta Ads",
		"semrush":               "SEMrush",
		"first_party_seo":       "First-party SEO evidence",
	}

	got := map[string]ConnectorSlot{}
	for _, connector := range connectors {
		if _, marketingSource := want[connector.Type]; marketingSource {
			got[connector.Type] = connector
		}
	}
	if len(got) != len(want) {
		t.Fatalf("marketing connector count = %d, want %d: %#v", len(got), len(want), got)
	}
	for connectorType, displayName := range want {
		connector, found := got[connectorType]
		if !found {
			t.Errorf("missing %s", connectorType)
			continue
		}
		if connector.DisplayName != displayName {
			t.Errorf("%s display name = %q, want %q", connectorType, connector.DisplayName, displayName)
		}
		if connector.Surface != SurfaceExternalAnalytics {
			t.Errorf("%s surface = %q, want %q", connectorType, connector.Surface, SurfaceExternalAnalytics)
		}
	}
}

func TestMarketingConnectorSlotsDeclareLeastPrivilegeCredentialBoundaries(t *testing.T) {
	connectors := connectorSlotsByType(DefaultConnectorSlots(ConnectorSlotOptions{TokenLeaseAudience: "insight-core"}))

	for _, connectorType := range []string{"google_search_console", "google_analytics_4", "google_ads", "meta_ads"} {
		connector := connectors[connectorType]
		if connector.Authorization != "integration_core_governed_action" && connector.Authorization != "social_core_metric_event" {
			t.Errorf("%s authorization = %q, want governed provider boundary", connectorType, connector.Authorization)
		}
		for _, requiredEnv := range connector.RequiredEnv {
			if containsCredentialName(requiredEnv) {
				t.Errorf("%s exposes credential-shaped env %q", connectorType, requiredEnv)
			}
		}
	}

	if got := connectors["google_search_console"].RequiredCapabilities; !slices.Contains(got, "search_console.read") {
		t.Errorf("GSC capabilities = %#v, want search_console.read", got)
	}
	if got := connectors["google_ads"].RequiredCapabilities; !slices.Contains(got, "ads.manage") {
		t.Errorf("Google Ads capabilities = %#v, want ads.manage", got)
	}
	if got := connectors["meta_ads"].RequiredCapabilities; !slices.Contains(got, "social.analytics.read") || !slices.Contains(got, "social.ads.manage") {
		t.Errorf("Meta Ads capabilities = %#v, want social.analytics.read and social.ads.manage", got)
	}
	metaContract := connectors["meta_ads"].Contracts[0]
	if !strings.Contains(metaContract.ResponseShape, "connector_type=meta") {
		t.Errorf("Meta Ads response shape = %q, want actual connector_type=meta", metaContract.ResponseShape)
	}
	if metaContract.Method != "NATS -> HTTP" {
		t.Errorf("Meta Ads transport = %q, want NATS -> HTTP", metaContract.Method)
	}
	for _, scope := range []string{"ads_management", "business_management", "ads_read", "read_insights"} {
		if !slices.Contains(metaContract.RequiredScopes, scope) {
			t.Errorf("Meta Ads scopes = %#v, missing %q", metaContract.RequiredScopes, scope)
		}
	}

	semrush := connectors["semrush"]
	if semrush.Status != ConnectorStatusRequiresCredentialBroker || semrush.Authorization != "integration_core_api_key_lease" {
		t.Errorf("SEMrush boundary = status:%q auth:%q, want broker-required API-key lease", semrush.Status, semrush.Authorization)
	}
	if len(semrush.RequiredEnv) != 0 {
		t.Errorf("SEMrush must not declare an application-facing API key env: %#v", semrush.RequiredEnv)
	}

	firstParty := connectors["first_party_seo"]
	if firstParty.Status != ConnectorStatusRequiresEvidenceAdapter || firstParty.Authorization != "ingestion_data_plane_evidence" {
		t.Errorf("first-party SEO boundary = status:%q auth:%q, want evidence adapter", firstParty.Status, firstParty.Authorization)
	}
	if len(firstParty.RequiredEnv) != 0 || len(firstParty.RequiredCapabilities) != 0 {
		t.Errorf("first-party SEO must not require provider credentials: %#v", firstParty)
	}
}

func TestMarketingConnectorSlotsDeclareTraceableModelGrounding(t *testing.T) {
	connectors := connectorSlotsByType(DefaultConnectorSlots(ConnectorSlotOptions{}))
	requiredFields := []string{"connector_type", "metric", "time_window", "observed_at", "source_trace_ref", "retention_class"}
	for _, connectorType := range []string{"google_search_console", "google_analytics_4", "google_ads", "meta_ads", "semrush", "first_party_seo"} {
		grounding := connectors[connectorType].ModelGrounding
		if grounding.Contract != "marketing_evidence.v1" {
			t.Errorf("%s grounding contract = %q", connectorType, grounding.Contract)
		}
		if !grounding.ExcludesZeroDataRetention {
			t.Errorf("%s must exclude zero-data-retention evidence", connectorType)
		}
		for _, requiredField := range requiredFields {
			if !slices.Contains(grounding.RequiredFields, requiredField) {
				t.Errorf("%s grounding fields = %#v, missing %q", connectorType, grounding.RequiredFields, requiredField)
			}
		}
	}
}

func TestMarketingConnectorSlotsDeclareReadinessStepsAndAdvisorySkillInstructions(t *testing.T) {
	connectors := connectorSlotsByType(DefaultConnectorSlots(ConnectorSlotOptions{}))

	google := connectors["google_search_console"]
	googleReadiness, found := integrationStepByID(google.IntegrationSteps, "test_read_only_access")
	if !found {
		t.Fatalf("Google Search Console is missing the read-only access test step: %#v", google.IntegrationSteps)
	}
	if googleReadiness.EndpointTemplate != "/api/v1/connections/{connectionId}/readiness" || googleReadiness.Method != "POST" || googleReadiness.FixedOperation != "gsc.sites.list" {
		t.Fatalf("Google readiness step = %#v, want fixed GSC readiness endpoint", googleReadiness)
	}
	if !slices.Contains(googleReadiness.RequiredCapabilities, "search_console.read") {
		t.Fatalf("Google readiness capabilities = %#v, want search_console.read", googleReadiness.RequiredCapabilities)
	}
	for _, scope := range google.Contracts[0].RequiredScopes {
		if scope == "https://www.googleapis.com/auth/webmasters" {
			t.Fatalf("Google Search Console registry must not advertise writable scope: %#v", google.Contracts[0].RequiredScopes)
		}
	}

	meta := connectors["meta_ads"]
	metaReadiness, found := integrationStepByID(meta.IntegrationSteps, "test_read_only_access")
	if !found {
		t.Fatalf("Meta Ads is missing the read-only access test step: %#v", meta.IntegrationSteps)
	}
	if metaReadiness.EndpointTemplate != "/api/v1/connections/{connectionId}/readiness" || metaReadiness.Method != "POST" || metaReadiness.FixedOperation != "ads.adaccounts" {
		t.Fatalf("Meta readiness step = %#v, want fixed Meta readiness endpoint", metaReadiness)
	}
	if !slices.Contains(metaReadiness.RequiredCapabilities, "social.ads.manage") {
		t.Fatalf("Meta readiness capabilities = %#v, want social.ads.manage", metaReadiness.RequiredCapabilities)
	}

	expectedSkills := []string{
		"seo-google", "ds-seo-weekly", "seo-site-audit-pro", "seo-sitemap", "seo-gsc-opportunities",
		"seo-gsc-content-decay", "google-search-console-tool", "blog-google", "seo-audit", "seo-ops",
		"seo-gsc-drops", "seo-gsc-brand-vs-nonbrand", "google-search-console", "conversion-optimization",
	}
	registeredSkills := map[string]bool{}
	for _, pack := range google.AdvisoryInstructionPacks {
		if pack.Mode != "advisory" || !pack.RequiresReadiness || len(pack.Instructions) == 0 {
			t.Errorf("unsafe or incomplete advisory instruction pack: %#v", pack)
		}
		for _, instruction := range pack.Instructions {
			lower := strings.ToLower(instruction)
			for _, forbidden := range []string{"npx", "mcp", "token", "secret", "http://", "https://"} {
				if strings.Contains(lower, forbidden) {
					t.Errorf("instruction %q exposes unsafe runtime detail %q", instruction, forbidden)
				}
			}
		}
		for _, skillID := range pack.SkillIDs {
			registeredSkills[skillID] = true
		}
	}
	for _, skillID := range expectedSkills {
		if !registeredSkills[skillID] {
			t.Errorf("advisory instruction packs are missing skill %q", skillID)
		}
	}
}

func TestCopyConnectorSlotsDefensivelyCopiesMarketingMetadata(t *testing.T) {
	repository := NewMemoryRepository(DefaultConnectorSlots(ConnectorSlotOptions{}))
	first, err := repository.ListConnectorSlots(t.Context(), "org-1")
	if err != nil {
		t.Fatalf("ListConnectorSlots error: %v", err)
	}
	google := connectorSlotsByType(first)["google_search_console"]
	google.RequiredCapabilities[0] = "mutated"
	google.Contracts[0].RequiredScopes[0] = "mutated"
	google.ModelGrounding.RequiredFields[0] = "mutated"
	google.IntegrationSteps[0].RequiredCapabilities[0] = "mutated"
	google.AdvisoryInstructionPacks[0].SkillIDs[0] = "mutated"
	google.AdvisoryInstructionPacks[0].Instructions[0] = "mutated"

	second, err := repository.ListConnectorSlots(t.Context(), "org-1")
	if err != nil {
		t.Fatalf("ListConnectorSlots error: %v", err)
	}
	stored := connectorSlotsByType(second)["google_search_console"]
	if stored.RequiredCapabilities[0] == "mutated" || stored.Contracts[0].RequiredScopes[0] == "mutated" || stored.ModelGrounding.RequiredFields[0] == "mutated" || stored.IntegrationSteps[0].RequiredCapabilities[0] == "mutated" || stored.AdvisoryInstructionPacks[0].SkillIDs[0] == "mutated" || stored.AdvisoryInstructionPacks[0].Instructions[0] == "mutated" {
		t.Fatalf("marketing connector metadata leaked mutable state: %#v", stored)
	}
}

func integrationStepByID(steps []IntegrationStep, id string) (IntegrationStep, bool) {
	for _, step := range steps {
		if step.ID == id {
			return step, true
		}
	}
	return IntegrationStep{}, false
}

func connectorSlotsByType(connectors []ConnectorSlot) map[string]ConnectorSlot {
	byType := make(map[string]ConnectorSlot, len(connectors))
	for _, connector := range connectors {
		byType[connector.Type] = connector
	}
	return byType
}

func containsCredentialName(value string) bool {
	value = strings.ToLower(value)
	for _, forbidden := range []string{"secret", "token", "api_key", "apikey", "password"} {
		if strings.Contains(value, forbidden) {
			return true
		}
	}
	return false
}
