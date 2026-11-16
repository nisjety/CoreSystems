package spaces

import (
	"crypto/ed25519"
	"strings"
	"testing"
	"time"
)

// As an execution workload, I can obtain a short-lived model-action view for
// the one action Control resolved from my run; the view is never an owner-plane
// effect permission.
func TestIssueModelActionViewIsRunBoundAndViewOnly(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	authority := validRunActionAuthority()
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)

	view, err := IssueModelActionView(evidence, authority, ModelActionViewRequest{
		DecisionRef: "view-1",
		Nonce:       "nonce-1",
	}, now)
	if err != nil {
		t.Fatalf("IssueModelActionView: %v", err)
	}
	if view.ServiceAudience != modelActionViewServiceAudience ||
		view.ActionID != "tickets.create" ||
		view.ActionSchemaHash != ticketsCreateActionSchemaSHA256 ||
		view.RunID != authority.RunID ||
		view.ThreadID != authority.ThreadID {
		t.Fatalf("unexpected model action view: %#v", view)
	}
	if len(view.Permissions) != 1 || view.Permissions[0] != modelActionViewPermission {
		t.Fatalf("view permissions = %#v; must not contain an owner-effect permission", view.Permissions)
	}
	if strings.Contains(strings.Join(view.Permissions, ","), "owner-action:execute") {
		t.Fatalf("view widened into owner effect authority: %#v", view.Permissions)
	}
	if err := view.Validate(); err != nil {
		t.Fatalf("issued model action view did not validate: %v", err)
	}

	seed := make([]byte, ed25519.SeedSize)
	for index := range seed {
		seed[index] = byte(index + 1)
	}
	token, err := SignModelActionView(SigningKey{
		ID:         "control-test",
		PrivateKey: ed25519.NewKeyFromSeed(seed),
	}, view)
	if err != nil {
		t.Fatalf("SignModelActionView: %v", err)
	}
	if !strings.HasPrefix(token, modelActionViewVersion+".") {
		t.Fatalf("model action view token uses an ambiguous envelope: %q", token)
	}
}

func TestIssueModelActionViewFailsClosedOnCurrentAuthorityMismatch(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	authority := validRunActionAuthority()
	authority.RecipientAudienceHash = "sha256:changed-audience"

	_, err := IssueModelActionView(evidence, authority, ModelActionViewRequest{
		DecisionRef: "view-1",
		Nonce:       "nonce-1",
	}, time.Now().UTC())
	if err == nil {
		t.Fatal("stale recipient audience produced a model action view")
	}
}

// A v2 view names only the requested actions the target table admits, and an
// unadmitted or unentitled request yields no view at all.
func TestIssueModelActionViewIntersectsEntitlement(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	request := ModelActionViewRequest{
		DecisionRef: "view-1",
		Nonce:       "nonce-1",
	}

	view, err := IssueModelActionViewV2(evidence, validRunActionAuthority(), request, now)
	if err != nil {
		t.Fatalf("IssueModelActionViewV2: %v", err)
	}
	if len(view.Actions) != 1 || view.Actions[0].ActionID != "tickets.create" || view.Actions[0].ActionSchemaHash != ticketsCreateActionSchemaSHA256 {
		t.Fatalf("v2 view names %#v; want only tickets.create", view.Actions)
	}
	if err := view.Validate(); err != nil {
		t.Fatalf("issued v2 view did not validate: %v", err)
	}
	token, err := SignModelActionViewV2(SigningKey{
		ID:         "control-test",
		PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef")),
	}, view)
	if err != nil {
		t.Fatalf("SignModelActionViewV2: %v", err)
	}
	if !strings.HasPrefix(token, modelActionViewVersionV2+".") {
		t.Fatalf("v2 view token uses envelope %q", token)
	}

	unentitled := evidence
	unentitled.AgentActionEntitled = false
	if _, err := IssueModelActionViewV2(unentitled, validRunActionAuthority(), request, now); err == nil {
		t.Fatal("a v2 view was issued without the agent action entitlement")
	}

	// With no operator allowlist the server-owned v2 list is empty, so no view is issued.
	if _, err := IssueModelActionViewV2(evidence, validRunActionAuthority(), request, now); err == nil {
		t.Fatal("a v2 view admitting no requested action was issued")
	}
}

func TestIssueModelActionViewRefusesZDR(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.Privacy.ZeroDataRetention = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	authority := validRunActionAuthority()

	if _, err := IssueModelActionView(evidence, authority, ModelActionViewRequest{DecisionRef: "view-1", Nonce: "nonce-1"}, now); err == nil {
		t.Fatal("a ZDR run received a v1 model action view")
	}
	if _, err := IssueModelActionViewV2(evidence, authority, ModelActionViewRequest{
		DecisionRef: "view-1", Nonce: "nonce-1",
	}, now); err == nil {
		t.Fatal("a ZDR run received a v2 model action view")
	}
}
