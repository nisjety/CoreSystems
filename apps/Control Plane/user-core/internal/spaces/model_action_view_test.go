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
