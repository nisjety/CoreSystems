package server

import (
	"context"
	"errors"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// F-f09-13: the org's web.mode withholds the public-web capabilities for every caller, and the browser capabilities
// too while the browser switch is on and the org does not opt out. The gate tightens a decision and reads no content.

var (
	orgWebPublicTestIDs  = []string{"cap.tool.http", "cap.web.crawl", "cap.web.map", "cap.web.batch"}
	orgWebBrowserTestIDs = []string{"cap.browser.open", "cap.browser.observe", "cap.browser.act"}
	// orgWebOtherTestID is a capability neither set names. The gate must never touch it.
	orgWebOtherTestID = "cap.memory.search"
)

// fakeOrgWebModes is the org's web keys as the gate reads them, with a count of the reads.
type fakeOrgWebModes struct {
	mode   string
	covers bool
	err    error
	calls  int
}

func (f *fakeOrgWebModes) WebMode(context.Context, string) (string, bool, error) {
	f.calls++
	if f.err != nil {
		return "", false, f.err
	}
	return f.mode, f.covers, nil
}

// orgWebGateServer is a server whose capabilities are available and low risk, so the policy decides allow for each
// before the gate runs, with the gate configured as the switches say.
func orgWebGateServer(t *testing.T, reader orgWebModeReader, web, browser bool) (*Server, *DecisionProofSigner) {
	t.Helper()
	ids := append(append(append([]string{}, orgWebPublicTestIDs...), orgWebBrowserTestIDs...), orgWebOtherTestID)
	rows := make([]*registry.CapabilityRow, 0, len(ids))
	for _, id := range ids {
		rows = append(rows, availableCapabilityRow(id, models.KindTool, models.RiskLow, models.SideEffectRead, models.ApprovalFloorNone))
	}
	server, signer := toolBindingServer(t, nil, rows...)
	server.WithOrgWebModeGate(reader, web, browser)
	return server, signer
}

func evaluateOrgWebCapability(t *testing.T, server *Server, capabilityID string) *mpv1.EvaluatePolicyResponse {
	t.Helper()
	response, err := server.EvaluatePolicy(verifiedGRPCContext(t, "org-a"), &mpv1.EvaluatePolicyRequest{
		CapabilityId: capabilityID, RunId: "run-a", AgentId: "execution-core", Scope: "global", UserId: "user-a",
	})
	if err != nil {
		t.Fatalf("EvaluatePolicy(%s): %v", capabilityID, err)
	}
	return response
}

func TestOrgWebModeGateOffChangesNothing(t *testing.T) {
	reader := &fakeOrgWebModes{mode: "off", covers: true}
	// Web off and the browser switch on: with the public-web switch off the browser switch has no effect either.
	server, _ := orgWebGateServer(t, reader, false, true)
	for _, id := range append(append([]string{}, orgWebPublicTestIDs...), orgWebBrowserTestIDs...) {
		response := evaluateOrgWebCapability(t, server, id)
		if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
			t.Fatalf("%s with the gate off: decision=%q source=%q, want the allow it had", id, response.Decision, response.PolicySource)
		}
	}
	if reader.calls != 0 {
		t.Fatalf("the org's web keys were read %d times with the gate off, want none", reader.calls)
	}
}

func TestOrgWebModeGateNeedsAReaderToBeOn(t *testing.T) {
	server, _ := orgWebGateServer(t, nil, true, true)
	if server.orgWebModeGate || server.orgWebModeBrowser {
		t.Fatalf("a gate with no reader is on: gate=%v browser=%v", server.orgWebModeGate, server.orgWebModeBrowser)
	}
	if response := evaluateOrgWebCapability(t, server, "cap.tool.http"); response.Decision != policy.DecisionAllow {
		t.Fatalf("a gate with no reader changed the decision: %+v", response)
	}
}

func TestWithOrgWebModeGateEnablesTheBrowserGateOnlyWithThePublicOne(t *testing.T) {
	reader := &fakeOrgWebModes{mode: "live", covers: true}
	for _, tc := range []struct {
		name                  string
		web, browser          bool
		wantGate, wantBrowser bool
	}{
		{"both off", false, false, false, false},
		{"browser alone does nothing", false, true, false, false},
		{"public alone", true, false, true, false},
		{"both on", true, true, true, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			server := &Server{}
			server.WithOrgWebModeGate(reader, tc.web, tc.browser)
			if server.orgWebModeGate != tc.wantGate || server.orgWebModeBrowser != tc.wantBrowser {
				t.Fatalf("gate=%v browser=%v, want gate=%v browser=%v", server.orgWebModeGate, server.orgWebModeBrowser, tc.wantGate, tc.wantBrowser)
			}
		})
	}
}

// Off, cached_only and a value this build does not know all withhold the public web, for each of its capabilities, and
// the signed evidence names the gate and the reason.
func TestOrgWebModeGateWithholdsThePublicWebWhenWebIsNotLive(t *testing.T) {
	for _, mode := range []string{"off", "cached_only", "a value this build does not know"} {
		t.Run(mode, func(t *testing.T) {
			reader := &fakeOrgWebModes{mode: mode, covers: true}
			server, signer := orgWebGateServer(t, reader, true, false)
			for _, id := range orgWebPublicTestIDs {
				response := evaluateOrgWebCapability(t, server, id)
				if response.Decision != policy.DecisionDeny || response.Reason != "org_web_off" || response.PolicySource != policy.PolicySourceOrgWebMode {
					t.Fatalf("%s under web.mode %q: decision=%q reason=%q source=%q", id, mode, response.Decision, response.Reason, response.PolicySource)
				}
				claims := evidenceClaims(t, signer, response.DecisionEvidence)
				if claims["decision"] != policy.DecisionDeny || claims["policy_source"] != policy.PolicySourceOrgWebMode {
					t.Fatalf("%s: evidence disagrees with the decision: %v", id, claims)
				}
			}
		})
	}
}

func TestOrgWebModeGateAllowsALiveOrgAndANonWebCapability(t *testing.T) {
	reader := &fakeOrgWebModes{mode: "live", covers: true}
	server, _ := orgWebGateServer(t, reader, true, true)
	for _, id := range append(append([]string{}, orgWebPublicTestIDs...), orgWebBrowserTestIDs...) {
		if response := evaluateOrgWebCapability(t, server, id); response.Decision != policy.DecisionAllow {
			t.Fatalf("%s under a live org: %+v, want allow", id, response)
		}
	}
	if reader.calls == 0 {
		t.Fatal("the gate never read the org's web keys")
	}

	// A capability neither set names is decided as before, and is not read for at all.
	reader = &fakeOrgWebModes{mode: "off", covers: true}
	server, _ = orgWebGateServer(t, reader, true, true)
	response := evaluateOrgWebCapability(t, server, orgWebOtherTestID)
	if response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("a non-web capability under web.mode off changed: %+v", response)
	}
	if reader.calls != 0 {
		t.Fatalf("a non-web capability read the org's web keys %d times, want none", reader.calls)
	}
}

// The browser capabilities need the second switch and an org that covers them: with the switch off, or with the org
// opted out, they keep the decision they had, and the public web stays withheld either way.
func TestOrgWebModeGateBrowserNeedsTheSecondSwitchAndCoverage(t *testing.T) {
	for _, tc := range []struct {
		name        string
		browser     bool
		covers      bool
		wantBrowser string
	}{
		{"the browser switch off", false, true, policy.DecisionAllow},
		{"the switch on and the org covered", true, true, policy.DecisionDeny},
		{"the switch on and the org opted out", true, false, policy.DecisionAllow},
	} {
		t.Run(tc.name, func(t *testing.T) {
			reader := &fakeOrgWebModes{mode: "off", covers: tc.covers}
			server, _ := orgWebGateServer(t, reader, true, tc.browser)
			for _, id := range orgWebBrowserTestIDs {
				response := evaluateOrgWebCapability(t, server, id)
				if response.Decision != tc.wantBrowser {
					t.Fatalf("%s: decision=%q, want %q", id, response.Decision, tc.wantBrowser)
				}
				if tc.wantBrowser == policy.DecisionDeny && (response.Reason != "org_web_off" || response.PolicySource != policy.PolicySourceOrgWebMode) {
					t.Fatalf("%s: reason=%q source=%q", id, response.Reason, response.PolicySource)
				}
			}
			for _, id := range orgWebPublicTestIDs {
				if response := evaluateOrgWebCapability(t, server, id); response.Decision != policy.DecisionDeny {
					t.Fatalf("%s with web.mode off and covers_browser %v: decision=%q, want the public web withheld", id, tc.covers, response.Decision)
				}
			}
		})
	}
}

// A failed read withholds the gated set with org_web_unavailable and nothing else. The browser set is gated only
// while its switch is on, so with the switch off a failed read does not reach it.
func TestOrgWebModeGateReadFailureWithholdsOnlyTheGatedSet(t *testing.T) {
	reader := &fakeOrgWebModes{err: errors.New("database unavailable")}
	server, _ := orgWebGateServer(t, reader, true, true)
	for _, id := range append(append([]string{}, orgWebPublicTestIDs...), orgWebBrowserTestIDs...) {
		response := evaluateOrgWebCapability(t, server, id)
		if response.Decision != policy.DecisionDeny || response.Reason != "org_web_unavailable" || response.PolicySource != policy.PolicySourceOrgWebMode {
			t.Fatalf("%s under a failed read: decision=%q reason=%q source=%q", id, response.Decision, response.Reason, response.PolicySource)
		}
	}
	if response := evaluateOrgWebCapability(t, server, orgWebOtherTestID); response.Decision != policy.DecisionAllow || response.PolicySource != policy.PolicySourceRiskLevel {
		t.Fatalf("a non-web capability changed under a failed read: %+v", response)
	}

	server, _ = orgWebGateServer(t, reader, true, false)
	if response := evaluateOrgWebCapability(t, server, "cap.browser.open"); response.Decision != policy.DecisionAllow {
		t.Fatalf("a browser capability with its switch off was withheld by a failed read: %+v", response)
	}
}

// The gate asks the store once per org per window, so the web calls of one run cost one read.
func TestOrgWebModeGateReadsTheStoreOncePerWindow(t *testing.T) {
	db := &fakeOrgWebDB{doc: map[string]any{"web.mode": "off"}}
	store := newOrgWebModeStore(db, func() time.Time { return time.Date(2026, 11, 24, 9, 0, 0, 0, time.UTC) })
	server, _ := orgWebGateServer(t, store, true, false)
	for range 3 {
		if response := evaluateOrgWebCapability(t, server, "cap.tool.http"); response.Decision != policy.DecisionDeny {
			t.Fatalf("decision = %q, want the public web withheld", response.Decision)
		}
	}
	if db.queries != 1 {
		t.Fatalf("the store was read %d times for three calls in one window, want 1", db.queries)
	}
}

// The gate only tightens: a decision that is already a deny stands and reads nothing, and an allow under a live org
// comes back as it went in.
func TestApplyOrgWebModeNeverLoosensADecision(t *testing.T) {
	reader := &fakeOrgWebModes{mode: "off", covers: true}
	server := &Server{orgWebModes: reader, orgWebModeGate: true, orgWebModeBrowser: true}
	request := &mpv1.EvaluatePolicyRequest{CapabilityId: "cap.tool.http"}
	ctx := context.Background()

	deny := &policy.Result{Decision: policy.DecisionDeny, Reason: "tool_policy_never", PolicySource: policy.PolicySourceToolPolicyNever}
	if got := server.applyOrgWebMode(ctx, "org-a", request, deny); got != deny {
		t.Fatalf("a deny was rewritten: %+v", got)
	}
	if reader.calls != 0 {
		t.Fatalf("a decision already denied read the org's keys %d times, want none", reader.calls)
	}

	ask := &policy.Result{Decision: policy.DecisionAsk, Reason: "ask", PolicySource: policy.PolicySourceRiskLevel}
	if got := server.applyOrgWebMode(ctx, "org-a", request, ask); got.Decision != policy.DecisionDeny || got.PolicySource != policy.PolicySourceOrgWebMode {
		t.Fatalf("an ask under web.mode off = %+v, want the web withheld", got)
	}

	reader.mode = "live"
	allow := &policy.Result{Decision: policy.DecisionAllow, Reason: "allow", PolicySource: policy.PolicySourceRiskLevel}
	if got := server.applyOrgWebMode(ctx, "org-a", request, allow); got != allow {
		t.Fatalf("an allow under a live org was changed: %+v", got)
	}
}

// The gate runs before the rule-use audit. A stored allow that would relax a high-risk web call is recorded only when
// the gate lets the call through: a call the org's web.mode withholds writes no tool_policy_rule_used row.
func TestOrgWebModeGateWritesNoRuleUseRowForACallItWithholds(t *testing.T) {
	for _, tc := range []struct {
		name         string
		gate         bool
		wantDecision string
		wantUses     int
	}{
		{"the gate off: the stored allow relaxes the call and is audited", false, policy.DecisionAllow, 1},
		{"the gate on: the call is withheld and nothing is audited", true, policy.DecisionDeny, 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			// A stored allow relaxes only a pinned MCP binding, so the call is an MCP tool, as in ruleUseServer. The gate keys
			// on the capability id, and this id is one of the public-web ids, so the ordering is exercised as it would be.
			capability := availableCapabilityRow("cap.web.crawl", models.KindMCPTool, models.RiskHigh, models.SideEffectExternalWrite, models.ApprovalFloorNone)
			db := &bindingDB{rows: []bindingRow{{
				capabilityID: capability.ID, toolName: "mcp__s__t", class: models.SideEffectExternalWrite, floor: models.ApprovalFloorNone,
				digest: testDigest, reviewState: policy.ReviewStateApproved, version: "1.0.0",
			}}}
			server, _ := toolBindingServer(t, db, capability)
			server.policy.WithToolPolicies(&toolPolicyStore{policies: []policy.ToolPolicy{userAllow("tp_user")}}, true)
			recorder := &ruleUseRecorder{}
			server.WithRuleAudit(recorder, true)
			server.WithOrgWebModeGate(&fakeOrgWebModes{mode: "off", covers: true}, tc.gate, false)

			response := evaluateRuleUse(t, verifiedGRPCContext(t, "org-a"), server, capability.ID, "run-a", "persistent")
			if response.Decision != tc.wantDecision {
				t.Fatalf("decision = %q, want %q (%+v)", response.Decision, tc.wantDecision, response)
			}
			if len(recorder.uses) != tc.wantUses {
				t.Fatalf("rule uses = %d, want %d", len(recorder.uses), tc.wantUses)
			}
		})
	}
}
