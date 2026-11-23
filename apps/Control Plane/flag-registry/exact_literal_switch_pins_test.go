package flagregistry

import (
	"slices"
	"strings"
	"testing"
)

// exactSwitchPin is one exact-literal switch. The registry declares it with this owner, source
// and default, and its purpose names the literal that turns it on. Most of these names sit
// outside the naming convention, so no ratchet asks for their entries. This table is what stops
// one from being deleted, re-typed or moved back to the unregistered baseline without a failing
// test.
type exactSwitchPin struct {
	name   string
	owner  Plane
	source Source
	def    string
}

var exactSwitchPins = []exactSwitchPin{
	{"BROWSER_BROKER_ASSISTANT_TASK_GRANTS_ENABLED", PlaneModel, SourceCompose, "0"},
	{"BROWSER_BROKER_FILL_OTP_ENABLED", PlaneModel, SourceCompose, "0"},
	{"BROWSER_BROKER_HUMAN_BROWSE_ENABLED", PlaneModel, SourceCompose, "true"},
	{"BROWSER_BROKER_KEY_GRANTS_ENABLED", PlaneModel, SourceCompose, "0"},
	{"BROWSER_BROKER_ORIGIN_EXTENSION_ENABLED", PlaneModel, SourceCompose, "0"},
	{"BROWSER_BROKER_WEB_POLICY_SNAPSHOT", PlaneModel, SourceCompose, "0"},
	{"CAPABILITY_CORE_ALLOW_UNSIGNED_DECISIONS", PlaneModel, SourceCode, "false"},
	{"COST_CORE_ALLOW_EPHEMERAL", PlaneModel, SourceCode, "false"},
	{"ENABLE_COMPAT_ADAPTER", PlaneModel, SourceCode, "false"},
	{"MODEL_GATEWAY_AI_LAYOUT_SERVICE", PlaneModel, SourceCompose, "0"},
	{"MODEL_GATEWAY_AI_PII_SERVICE", PlaneModel, SourceCompose, "0"},
	{"MODEL_GATEWAY_AUTH_DEV_BYPASS", PlaneModel, SourceCompose, "false"},
	{"MODEL_GATEWAY_BROWSER_ATTACH", PlaneModel, SourceCompose, "off"},
	{"MODEL_GATEWAY_BROWSER_ATTACH_POLICY_ENFORCED", PlaneModel, SourceCompose, "off"},
	{"MODEL_GATEWAY_BROWSER_BUDGET_CLAIM", PlaneModel, SourceCode, "false"},
	{"MODEL_GATEWAY_BROWSER_KEY_GRANTS", PlaneModel, SourceCompose, "0"},
	{"MODEL_GATEWAY_BROWSER_PLANNER_QUOTAS", PlaneModel, SourceCompose, "off"},
	{"MODEL_GATEWAY_BROWSER_POLICY_ASK", PlaneModel, SourceCompose, "off"},
	{"MODEL_GATEWAY_DATED_POST_SECTION_ONLY", PlaneModel, SourceCompose, "true"},
	{"MODEL_GATEWAY_EXTRACT_TOOL", PlaneModel, SourceCompose, "0"},
	{"MODEL_GATEWAY_FINER_REVIEW_BATCHES", PlaneModel, SourceCompose, "true"},
	{"MODEL_PLANE_ALLOW_NON_EU_TTS", PlaneModel, SourceCompose, "false"},
	{"QUARRY_BROWSER_AGENT_ENABLED", PlaneModel, SourceCompose, "true"},
	{"SANDBOX_MANAGER_REQUIRE_RETENTION_CLAIM", PlaneModel, SourceCompose, "off"},
	{"SESSION_AGENT_MEMORY_ENABLED", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_APPROVAL_MEMBER_DECIDE", PlaneModel, SourceCompose, "0"},
	{"SESSION_CORE_APPROVAL_NOTIFY", PlaneModel, SourceCode, "false"},
	{"SESSION_CORE_AUTHZ_DECISION_AUDIT", PlaneModel, SourceCode, "false"},
	{"SESSION_CORE_ROUTING_CANDIDATES_ENABLED", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_SPACE_HISTORY_FLOOR", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_SPACE_MEMORY_DELETE_AUDIT", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_TODO_RECITATION_ENABLED", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_TOOL_LEDGER_SWEEP_ENABLED", PlaneModel, SourceCompose, "false"},
	{"SESSION_CORE_TRACE_CONSUMER", PlaneModel, SourceCode, "false"},
	{"SESSION_SPACE_MEMORY_ENABLED", PlaneModel, SourceCompose, "false"},
	{"SPACE_ATTENTION_EMIT", PlaneModel, SourceCompose, "true"},
	{"SPACE_HUMAN_POSTS_API", PlaneFrontend, SourceCode, "false"},
	{"SPACE_HUMAN_POSTS_CORE", PlaneModel, SourceCompose, "0"},
	{"SPACE_MENTION_RECORDS_CORE", PlaneModel, SourceCompose, "0"},
	{"SPACE_MESSAGE_REVISE_API", PlaneFrontend, SourceCode, "false"},
	{"SPACE_MESSAGE_REVISE_CORE", PlaneModel, SourceCompose, "0"},
	{"SPACE_MESSAGE_SEARCH_API", PlaneFrontend, SourceCode, "false"},
	{"SPACE_MESSAGE_SEARCH_CORE", PlaneModel, SourceCompose, "0"},
	{"SPACE_REALTIME_EMIT", PlaneModel, SourceCompose, "true"},
	{"SPACE_ROOM_METADATA_LEGACY_PASSTHROUGH", PlaneModel, SourceCompose, "false"},
	{"SPACE_RUN_LIVE_TEXT", PlaneFrontend, SourceCode, "false"},
	{"SPACE_RUN_LIVE_VIEW", PlaneFrontend, SourceCode, "false"},
	{"SPACE_RUN_MANAGER_STOP", PlaneFrontend, SourceCode, "false"},
	{"SPACE_TIMELINE_API", PlaneFrontend, SourceCode, "false"},
	{"SPACE_TIMELINE_CORE", PlaneModel, SourceCompose, "0"},
	{"SPACE_WAKE_CORE", PlaneModel, SourceCompose, "0"},
	{"TASK_EXECUTOR_ACCEPT_PUBLISH_ONLY_DISPATCH", PlaneModel, SourceCompose, "false"},
	{"VEREVON_RELEVANCE_CROSS_ENCODER_SCORES", PlaneModel, SourceCode, "false"},
}

// TestExactLiteralSwitchesAreRegistered pins each exact-literal switch: a switch with the stated owner,
// source and default, whose purpose names the literal that turns it on, and which is no longer on the
// unregistered baseline.
func TestExactLiteralSwitchesAreRegistered(t *testing.T) {
	reg := loadShipped(t)
	unregistered := reg.Unregistered()
	for _, p := range exactSwitchPins {
		f, ok := reg.Flag(p.name)
		if !ok {
			t.Errorf("%s has no registry entry", p.name)
			continue
		}
		if f.Kind != KindSwitch || f.Owner != p.owner || f.Source != p.source || f.Default != p.def {
			t.Errorf("%s: kind %q owner %q source %q default %q, want switch %q %q %q",
				p.name, f.Kind, f.Owner, f.Source, f.Default, p.owner, p.source, p.def)
		}
		if !strings.Contains(f.Purpose, "literal true") {
			t.Errorf("%s: purpose must name the literal that turns it on: %q", p.name, f.Purpose)
		}
		if slices.Contains(unregistered, p.name) {
			t.Errorf("%s is on the unregistered baseline; its entry replaces that line", p.name)
		}
	}
}

// TestAllowInsecureDevDefaultsNamesItsLiteral keeps the gate that the BFF and Model Plane share on the one
// literal both read: only 1 opens it. Its purpose says so, and the BFF test pins the reader.
func TestAllowInsecureDevDefaultsNamesItsLiteral(t *testing.T) {
	f, ok := loadShipped(t).Flag("ALLOW_INSECURE_DEV_DEFAULTS")
	if !ok {
		t.Fatal("ALLOW_INSECURE_DEV_DEFAULTS is not registered")
	}
	if !strings.Contains(f.Purpose, "Only the literal 1 opens the gate") {
		t.Errorf("purpose must say that only the literal 1 opens the gate: %q", f.Purpose)
	}
}
