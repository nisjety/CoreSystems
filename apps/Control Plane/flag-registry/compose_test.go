package flagregistry

import (
	"slices"
	"strings"
	"testing"
)

const composeFixture = `name: example

x-defaults: &defaults
  restart: unless-stopped

services:
  model-gateway:
    image: example
    environment:
      AGENTIC_SUBSCRIPTION_ENABLED: ${AGENTIC_SUBSCRIPTION_ENABLED:-false}
      # DISABLED_ENABLED: ${DISABLED_ENABLED:-true}
      EXECUTION_LOOP_PARKING_ENABLED: ${MODEL_GATEWAY_LOOP_PARKING_ENABLED:-false}
      MP_COMPAT_MODE: v1_only
      CHAT_DO_ATTACHMENTS_ENABLED: ${CHAT_DO_ATTACHMENTS_ENABLED:-}
  execution-core:
    environment:
      WIDE_RESEARCH_FAN_OUT_ENABLED: ${WIDE_RESEARCH_FAN_OUT_ENABLED:-true} # kill switch

volumes:
  data:
    ignored: ${IGNORED_ENABLED:-false}
`

func TestScanComposeFindsServiceDeclarations(t *testing.T) {
	want := []ComposeDecl{
		{Service: "model-gateway", Env: "AGENTIC_SUBSCRIPTION_ENABLED", Host: "AGENTIC_SUBSCRIPTION_ENABLED", Default: "false"},
		{Service: "model-gateway", Env: "EXECUTION_LOOP_PARKING_ENABLED", Host: "MODEL_GATEWAY_LOOP_PARKING_ENABLED", Default: "false"},
		{Service: "model-gateway", Env: "CHAT_DO_ATTACHMENTS_ENABLED", Host: "CHAT_DO_ATTACHMENTS_ENABLED", Default: ""},
		{Service: "execution-core", Env: "WIDE_RESEARCH_FAN_OUT_ENABLED", Host: "WIDE_RESEARCH_FAN_OUT_ENABLED", Default: "true"},
	}
	docs := map[string]string{
		"lf":   composeFixture,
		"crlf": strings.ReplaceAll(composeFixture, "\n", "\r\n"),
	}
	for name, doc := range docs {
		t.Run(name, func(t *testing.T) {
			got, err := ScanCompose(strings.NewReader(doc))
			if err != nil {
				t.Fatalf("ScanCompose: %v", err)
			}
			if !slices.Equal(got, want) {
				t.Fatalf("ScanCompose = %+v, want %+v", got, want)
			}
		})
	}
}

// composeFourSpaceFixture indents services by four spaces, as the Control Plane
// compose does. A scanner that reads only the two-space layout names no service.
const composeFourSpaceFixture = `name: control-plane

x-core-security: &core-security
    read_only: true

services:
    auth-core:
        image: example
        environment:
            WEB_POLICY_CLAIM_ENABLED: ${WEB_POLICY_CLAIM_ENABLED:-0}
            MP_COMPAT_MODE: v1_only
    audit-siem-exporter:
        environment:
            AUDIT_SIEM_EXPORT_ENABLED: ${AUDIT_SIEM_EXPORT_ENABLED:-0}
            AUDIT_CHAIN_VERIFY_ENABLED: ${AUDIT_CHAIN_VERIFY_ENABLED:-}

networks:
    controlplane-net:
        name: example
`

func TestScanComposeNamesServicesAtFourSpaces(t *testing.T) {
	want := []ComposeDecl{
		{Service: "auth-core", Env: "WEB_POLICY_CLAIM_ENABLED", Host: "WEB_POLICY_CLAIM_ENABLED", Default: "0"},
		{Service: "audit-siem-exporter", Env: "AUDIT_SIEM_EXPORT_ENABLED", Host: "AUDIT_SIEM_EXPORT_ENABLED", Default: "0"},
		{Service: "audit-siem-exporter", Env: "AUDIT_CHAIN_VERIFY_ENABLED", Host: "AUDIT_CHAIN_VERIFY_ENABLED", Default: ""},
	}
	got, err := ScanCompose(strings.NewReader(composeFourSpaceFixture))
	if err != nil {
		t.Fatalf("ScanCompose: %v", err)
	}
	if !slices.Equal(got, want) {
		t.Fatalf("ScanCompose = %+v, want %+v", got, want)
	}
}

func TestIsConventionName(t *testing.T) {
	inside := []string{
		"AGENTIC_SUBSCRIPTION_ENABLED",
		"AGENT_MEMORY_AUTH_MODE",
		"CONTENT_SAFETY_SHADOW_SAMPLE_RATE",
		"EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK",
		"AZURE_OPENAI_ZDR_CONFIRMED",
	}
	outside := []string{
		"DREAMING_LLM_EXTRACTION",
		"MODEL_GATEWAY_COMPACTION_TOKEN_BUDGET",
		"enabled_x",
		"FOO_ENABLE",
		"FOO_ENABLED_X",
		"_ENABLED",
	}
	for _, name := range inside {
		if !IsConventionName(name) {
			t.Errorf("IsConventionName(%q) = false, want true", name)
		}
	}
	for _, name := range outside {
		if IsConventionName(name) {
			t.Errorf("IsConventionName(%q) = true, want false", name)
		}
	}
}
