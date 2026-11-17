package spaces

import (
	"crypto/ed25519"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

func validRunActionAuthority() RunActionAuthority {
	return RunActionAuthority{
		RunID:                      "run-1",
		OrgID:                      "org-1",
		SubjectID:                  "user-1",
		ThreadID:                   "thread-1",
		SpaceRef:                   "space-personal",
		RecipientAudienceRef:       "audience:space-personal:2",
		RecipientAudienceRevision:  2,
		RecipientAudienceHash:      "sha256:test-audience",
		PrivacyPolicyRef:           "privacy:org-1:5",
		RunContextAuthorizationRef: "control:space-personal:thread-create:7",
		AuthorityRevision:          7,
		RunStatus:                  "running",
	}
}

// validRunActionRequest carries the pinned tickets.create hash: decisions compare
// the request hash with the golden registry row, not only its form.
func validRunActionRequest() RunActionDecisionRequest {
	return RunActionDecisionRequest{
		ActionID:         "tickets.create",
		ActionSchemaHash: ticketsCreateActionSchemaSHA256,
		PayloadDigest:    "sha256:" + strings.Repeat("b", 64),
		IdempotencyKey:   "run-1:tickets.create:1",
		DecisionRef:      "decision-1",
		Nonce:            "nonce-1",
	}
}

func TestIssueRunActionDecisionIsTargetBoundWithoutWideningThreadAuthority(t *testing.T) {
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	authority := validRunActionAuthority()
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)

	decision, err := IssueRunActionDecision(evidence, authority, validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision: %v", err)
	}
	if decision.ServiceAudience != ticketsCreateServiceAudience ||
		decision.ActionID != "tickets.create" ||
		decision.RunID != authority.RunID ||
		decision.ThreadID != authority.ThreadID {
		t.Fatalf("unexpected target action decision: %#v", decision)
	}
	if decision.RunContextAuthorizationRef != authority.RunContextAuthorizationRef {
		t.Fatalf("run context authorization changed: %#v", decision)
	}
	if strings.Contains(strings.ToLower(strings.Join(decision.Permissions, ",")), "resource") {
		t.Fatalf("thread context must not grant a target resource: %#v", decision)
	}
	if err := decision.Validate(); err != nil {
		t.Fatalf("issued run action decision did not validate: %v", err)
	}
}

func TestIssueRunActionDecisionFailsClosedOnStaleOrIneligibleAuthority(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence, *RunActionAuthority, *RunActionDecisionRequest){
		"agent action entitlement disabled": func(e *PersonalThreadDecisionEvidence, _ *RunActionAuthority, _ *RunActionDecisionRequest) {
			e.AgentActionEntitled = false
		},
		"zero retention": func(e *PersonalThreadDecisionEvidence, _ *RunActionAuthority, _ *RunActionDecisionRequest) {
			e.Privacy.ZeroDataRetention = true
		},
		"stale authority revision": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority, _ *RunActionDecisionRequest) {
			a.AuthorityRevision++
		},
		"different recipient audience": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority, _ *RunActionDecisionRequest) {
			a.RecipientAudienceHash = "sha256:other-audience"
		},
		"thread authorization cannot be substituted": func(_ *PersonalThreadDecisionEvidence, a *RunActionAuthority, _ *RunActionDecisionRequest) {
			a.RunContextAuthorizationRef = "control:space-personal:retrieval-read:7"
		},
		"unknown action": func(_ *PersonalThreadDecisionEvidence, _ *RunActionAuthority, r *RunActionDecisionRequest) {
			r.ActionID = "users.delete"
		},
		"invalid action schema": func(_ *PersonalThreadDecisionEvidence, _ *RunActionAuthority, r *RunActionDecisionRequest) {
			r.ActionSchemaHash = "sha256:not-a-digest"
		},
		"oversized idempotency key": func(_ *PersonalThreadDecisionEvidence, _ *RunActionAuthority, r *RunActionDecisionRequest) {
			r.IdempotencyKey = strings.Repeat("a", 201)
		},
	} {
		t.Run(name, func(t *testing.T) {
			evidence := validPersonalThreadEvidence()
			evidence.AgentActionEntitled = true
			evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
			authority := validRunActionAuthority()
			request := validRunActionRequest()
			mutate(&evidence, &authority, &request)
			if _, err := IssueRunActionDecision(evidence, authority, request, now); err == nil {
				t.Fatal("ineligible or stale run authority unexpectedly issued a target decision")
			}
		})
	}
}

func TestValidateCurrentRunActionDecisionRejectsSupersededControlFacts(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision() error = %v", err)
	}
	if err := ValidateCurrentRunActionDecision(evidence, decision); err != nil {
		t.Fatalf("ValidateCurrentRunActionDecision() error = %v", err)
	}

	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence){
		"recipient audience changed": func(e *PersonalThreadDecisionEvidence) {
			e.RecipientAudienceHash = "sha256:changed-audience"
			e.Membership.Revisions.Authority++
			e.Membership.Revisions.RecipientAudience++
		},
		"privacy policy changed": func(e *PersonalThreadDecisionEvidence) {
			e.Privacy.PolicyRef = "privacy:org-1:changed"
			e.Membership.Revisions.Authority++
			e.Membership.Revisions.Privacy++
		},
		"agent action entitlement revoked": func(e *PersonalThreadDecisionEvidence) {
			e.AgentActionEntitled = false
			e.Membership.Revisions.Authority++
			e.Membership.Revisions.Entitlement++
		},
	} {
		t.Run(name, func(t *testing.T) {
			current := evidence
			mutate(&current)
			if err := ValidateCurrentRunActionDecision(current, decision); err == nil {
				t.Fatal("superseded Control facts unexpectedly authorized the old decision")
			}
		})
	}
}

// W3-FIX-12: the effect-time check repeats the canvas authority for a canvas
// decision. The revisions are left as issued, so only the canvas check can refuse:
// canvas_entitled turning off or a since_join floor appearing stops a canvas
// decision, while a tickets.create decision passes as before.
func TestValidateCurrentRunActionDecisionRejectsSupersededCanvasFacts(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionRoomEvidence()
	evidence.CanvasEntitled = true
	authority := runActionAuthorityFor(evidence)
	tickets, err := IssueRunActionDecision(evidence, authority, validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision tickets.create: %v", err)
	}
	canvasOff := evidence
	canvasOff.CanvasEntitled = false
	floored := evidence
	floored.HistoryFloorRevision = 3

	for _, actionID := range []string{canvasRunActionReadID, canvasRunActionWriteID} {
		t.Run(actionID, func(t *testing.T) {
			decision, err := IssueRunActionDecision(evidence, authority, goldenRunActionRequest(t, actionID), now)
			if err != nil {
				t.Fatalf("IssueRunActionDecision: %v", err)
			}
			if err := ValidateCurrentRunActionDecision(evidence, decision); err != nil {
				t.Fatalf("an unchanged canvas decision failed the effect-time check: %v", err)
			}
			if denial := wantDenial(t, ValidateCurrentRunActionDecision(canvasOff, decision), DenialEntitlementOff); denial.Entitlement != "canvas" {
				t.Fatalf("entitlement = %q, want canvas", denial.Entitlement)
			}
			if err := ValidateCurrentRunActionDecision(floored, decision); !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
				t.Fatalf("a floored room = %v, want ErrCanvasHistoryFloorUnsupported", err)
			}
		})
	}
	for name, current := range map[string]PersonalThreadDecisionEvidence{"unchanged": evidence, "canvas off": canvasOff, "floored": floored} {
		if err := ValidateCurrentRunActionDecision(current, tickets); err != nil {
			t.Errorf("tickets.create (%s) failed the effect-time check: %v", name, err)
		}
	}
}

// tickets.create decides exactly as it did before the registry existed: the decision
// for the shared fixture marshals to these bytes, with or without an allowlist.
func TestTicketsCreateDecisionUnchanged(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	want := `{"decision_ref":"decision-1","run_id":"run-1","thread_id":"thread-1","org_id":"org-1",` +
		`"space_ref":"space-personal","subject_id":"user-1","service_audience":"application-plane-conversation-core",` +
		`"action_id":"tickets.create","action_schema_hash":"sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df",` +
		`"payload_digest":"sha256:` + strings.Repeat("b", 64) + `","idempotency_key":"run-1:tickets.create:1",` +
		`"recipient_audience_ref":"audience:space-personal:2","recipient_audience_hash":"sha256:test-audience",` +
		`"recipient_audience_revision":2,"privacy_policy_ref":"privacy:org-1:5",` +
		`"run_context_authorization_ref":"control:space-personal:thread-create:7","authority_revision":7,` +
		`"permissions":["owner-action:execute"],"purpose":"assistant_collaboration","lawful_basis":"contract",` +
		`"privacy_class":"internal","third_party_processing_allowed":false,"retention_class":"standard",` +
		`"residency":"swedencentral","deletion_scope":"space","zero_data_retention":false,` +
		`"issued_at":"2026-08-15T12:00:00Z","expires_at":"2026-08-15T12:02:00Z","nonce":"nonce-1"}`
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
	for _, allowlist := range []string{"", "space.thread.rename,space.canvas.read"} {
		t.Setenv(spaceSelfAPIActionsEnv, allowlist)
		decision, err := IssueRunActionDecision(runActionPersonalEvidence(), validRunActionAuthority(), validRunActionRequest(), now)
		if err != nil {
			t.Fatalf("IssueRunActionDecision (allowlist %q): %v", allowlist, err)
		}
		got, err := json.Marshal(decision)
		if err != nil {
			t.Fatalf("marshal: %v", err)
		}
		if string(got) != want {
			t.Fatalf("tickets.create decision bytes changed (allowlist %q):\n got %s\nwant %s", allowlist, got, want)
		}
		token, err := SignRunActionDecision(key, decision)
		if err != nil {
			t.Fatalf("SignRunActionDecision: %v", err)
		}
		if verified, err := VerifyRunActionDecision(key, token, now); err != nil || !strings.HasPrefix(token, runActionDecisionVersion+".") ||
			verified.ActionSchemaHash != ticketsCreateActionSchemaSHA256 {
			t.Fatalf("VerifyRunActionDecision = %+v, %v", verified, err)
		}
	}
}

// The issued hash must be the golden hash of the requested row, not merely a
// well-formed commitment. tickets.create's parameter-bytes hash is refused too: its
// basis is the owner contract, not its parameters.
func TestRunActionDecisionRefusesSchemaHashMismatch(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionPersonalEvidence()
	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename")
	tickets, _ := runActionSpec(ticketsCreateActionID)
	rename, _ := runActionSpec("space.thread.rename")
	for name, test := range map[string]struct{ actionID, hash string }{
		"tickets.create with a foreign hash":           {ticketsCreateActionID, "sha256:" + strings.Repeat("a", 64)},
		"tickets.create with its parameter-bytes hash": {ticketsCreateActionID, runActionSchemaCommitment(tickets.ParametersJSON)},
		"tickets.create with a Space row's hash":       {ticketsCreateActionID, rename.SchemaSHA256},
		"a Space row with the tickets.create hash":     {"space.thread.rename", ticketsCreateActionSchemaSHA256},
		"a Space row with an upper-case hash":          {"space.thread.rename", "sha256:" + strings.ToUpper(strings.TrimPrefix(rename.SchemaSHA256, "sha256:"))},
	} {
		t.Run(name, func(t *testing.T) {
			request := goldenRunActionRequest(t, test.actionID)
			request.ActionSchemaHash = test.hash
			if err := request.Validate(); err == nil {
				t.Fatal("a request with a mismatched schema hash validated")
			}
			if _, err := IssueRunActionDecision(evidence, validRunActionAuthority(), request, now); err == nil {
				t.Fatal("a decision with a mismatched schema hash was issued")
			}
		})
	}

	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), goldenRunActionRequest(t, "space.thread.rename"), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision: %v", err)
	}
	tampered := decision
	tampered.ActionSchemaHash = ticketsCreateActionSchemaSHA256
	if err := tampered.Validate(); err == nil {
		t.Fatal("a decision whose schema hash changed after issuance validated")
	}
	if err := ValidateCurrentRunActionDecision(evidence, tampered); err == nil {
		t.Fatal("the effect-time check accepted a decision whose schema hash changed")
	}
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
	if _, err := SignRunActionDecision(key, tampered); err == nil {
		t.Fatal("a decision whose schema hash changed was signed")
	}
}

// The agent gate (ValidateForAgentAction) runs first for every row: a viewer is
// below it and Zero Data Retention refuses with the agent_action effect, at
// issuance and at effect time. Past the gate, each row's own min_role applies.
func TestRunActionRefusedForViewerAndZDR(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename")
	for _, actionID := range []string{ticketsCreateActionID, "space.thread.rename"} {
		t.Run(actionID, func(t *testing.T) {
			personalViewer := runActionPersonalEvidence()
			personalViewer.Membership.Role = RoleViewer
			roomViewer := runActionRoomEvidence()
			roomViewer.Membership.Role = RoleViewer
			for name, viewer := range map[string]PersonalThreadDecisionEvidence{"personal": personalViewer, "room": roomViewer} {
				_, err := IssueRunActionDecision(viewer, runActionAuthorityFor(viewer), goldenRunActionRequest(t, actionID), now)
				if err == nil {
					t.Fatalf("a %s viewer was issued a run action", name)
				}
				wantDenial(t, err, DenialRoleBelowFloor)
			}
			zdr := runActionPersonalEvidence()
			zdr.Privacy.ZeroDataRetention = true
			_, err := IssueRunActionDecision(zdr, validRunActionAuthority(), goldenRunActionRequest(t, actionID), now)
			if denial := wantDenial(t, err, DenialZDRForbidsEffect); denial.Effect != "agent_action" {
				t.Fatalf("ZDR effect = %q, want agent_action (the agent gate)", denial.Effect)
			}

			decision, err := IssueRunActionDecision(runActionPersonalEvidence(), validRunActionAuthority(), goldenRunActionRequest(t, actionID), now)
			if err != nil {
				t.Fatalf("IssueRunActionDecision: %v", err)
			}
			wantDenial(t, ValidateCurrentRunActionDecision(personalViewer, decision), DenialRoleBelowFloor)
			wantDenial(t, ValidateCurrentRunActionDecision(zdr, decision), DenialZDRForbidsEffect)
		})
	}

	withRunActionRegistryRow(t, "space.thread.rename", func(row *runActionSpecRow) { row.MinRole = RoleManager })
	editor := runActionRoomEvidence()
	if _, err := IssueRunActionDecision(editor, runActionAuthorityFor(editor), goldenRunActionRequest(t, "space.thread.rename"), now); err == nil {
		t.Fatal("an editor was issued a manager-floor row")
	}
	manager := runActionRoomEvidence()
	manager.Membership.Role = RoleManager
	if _, err := IssueRunActionDecision(manager, runActionAuthorityFor(manager), goldenRunActionRequest(t, "space.thread.rename"), now); err != nil {
		t.Fatalf("a manager was refused a manager-floor row: %v", err)
	}
}

func TestVerifyRunActionDecisionRejectsTamperingAndExpiry(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	decision, err := IssueRunActionDecision(evidence, validRunActionAuthority(), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision() error = %v", err)
	}
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
	token, err := SignRunActionDecision(key, decision)
	if err != nil {
		t.Fatalf("SignRunActionDecision() error = %v", err)
	}
	if _, err := VerifyRunActionDecision(key, token, now.Add(time.Minute)); err != nil {
		t.Fatalf("VerifyRunActionDecision() error = %v", err)
	}
	if _, err := VerifyRunActionDecision(key, token+"x", now.Add(time.Minute)); err == nil {
		t.Fatal("tampered decision token accepted")
	}
	if _, err := VerifyRunActionDecision(key, token, decision.ExpiresAt.Add(time.Second)); err == nil {
		t.Fatal("expired decision token accepted")
	}
}
