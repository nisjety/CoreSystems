package spaces

import (
	"errors"
	"fmt"
	"reflect"
	"testing"
	"time"
)

// W3-FIX-12 (WS19-S9): a canvas run action (space.canvas.read, space.canvas.write)
// meets the canvas authority a person's canvas decision meets, plus the since_join
// refusal, before Control issues it. tickets.create is unaffected by every canvas fact.

// runActionRoomEvidence is a room editor's agent-action evidence: a current recipient
// audience, thread_create and agent_action on, canvas off, and the run's source
// context bound as ResolveAgentActionDecisionEvidence binds it.
func runActionRoomEvidence() PersonalThreadDecisionEvidence {
	evidence := validPersonalThreadEvidence()
	evidence.Membership.SpaceRef = "space-room"
	evidence.Membership.Kind = KindRoom
	evidence.Membership.Role = RoleEditor
	evidence.RecipientAudienceRef = "audience:space-room:2"
	evidence.RecipientAudienceHash = "sha256:room-audience"
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-room:thread-create:7"
	return evidence
}

// runActionAuthorityFor is the run binding Session Core reports for a run started
// under evidence.
func runActionAuthorityFor(evidence PersonalThreadDecisionEvidence) RunActionAuthority {
	authority := validRunActionAuthority()
	authority.OrgID = evidence.Membership.OrgID
	authority.SubjectID = evidence.Membership.SubjectID
	authority.SpaceRef = evidence.Membership.SpaceRef
	authority.RecipientAudienceRef = evidence.RecipientAudienceRef
	authority.RecipientAudienceRevision = evidence.Membership.Revisions.RecipientAudience
	authority.RecipientAudienceHash = evidence.RecipientAudienceHash
	authority.PrivacyPolicyRef = evidence.Privacy.PolicyRef
	authority.AuthorityRevision = evidence.Membership.Revisions.Authority
	authority.RunContextAuthorizationRef = fmt.Sprintf("control:%s:thread-create:%d", evidence.Membership.SpaceRef, evidence.Membership.Revisions.Authority)
	return authority
}

func TestRunActionCanvasRefusesWhatTheCanvasRefuses(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	entitled := runActionRoomEvidence()
	entitled.CanvasEntitled = true

	for name, test := range map[string]struct {
		mutate func(*PersonalThreadDecisionEvidence)
		check  func(*testing.T, error)
	}{
		"canvas_entitled off": {
			func(e *PersonalThreadDecisionEvidence) { e.CanvasEntitled = false },
			func(t *testing.T, err error) {
				if denial := wantDenial(t, err, DenialEntitlementOff); denial.Entitlement != "canvas" {
					t.Fatalf("entitlement = %q, want canvas", denial.Entitlement)
				}
			},
		},
		"dm": {
			func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindDM },
			func(t *testing.T, err error) {
				if denial := wantDenial(t, err, DenialKindUnsupported); denial.Kind != string(KindDM) {
					t.Fatalf("kind = %q, want dm", denial.Kind)
				}
			},
		},
		"group_dm": {
			func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindGroupDM },
			func(t *testing.T, err error) {
				if denial := wantDenial(t, err, DenialKindUnsupported); denial.Kind != string(KindGroupDM) {
					t.Fatalf("kind = %q, want group_dm", denial.Kind)
				}
			},
		},
		"zero data retention": {
			func(e *PersonalThreadDecisionEvidence) { e.Privacy.ZeroDataRetention = true },
			func(t *testing.T, err error) { wantDenial(t, err, DenialZDRForbidsEffect) },
		},
		// A viewer may read a canvas as a person, but the agent gate needs an editor.
		"viewer": {
			func(e *PersonalThreadDecisionEvidence) { e.Membership.Role = RoleViewer },
			func(t *testing.T, err error) { wantDenial(t, err, DenialRoleBelowFloor) },
		},
		"since_join history floor": {
			func(e *PersonalThreadDecisionEvidence) { e.HistoryFloorRevision = 3 },
			func(t *testing.T, err error) {
				if !errors.Is(err, ErrCanvasHistoryFloorUnsupported) {
					t.Fatalf("err = %v, want ErrCanvasHistoryFloorUnsupported", err)
				}
			},
		},
	} {
		for _, actionID := range []string{canvasRunActionReadID, canvasRunActionWriteID} {
			t.Run(name+"/"+actionID, func(t *testing.T) {
				evidence := entitled
				test.mutate(&evidence)
				decision, err := IssueRunActionDecision(evidence, runActionAuthorityFor(evidence), goldenRunActionRequest(t, actionID), now)
				if err == nil {
					t.Fatalf("a canvas run action was issued: %+v", decision)
				}
				test.check(t, err)
			})
		}
	}
}

func TestRunActionCanvasAcceptsAnEntitledEditorWithoutAFloor(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	personal := runActionPersonalEvidence()
	personal.Membership.Role = RoleEditor
	personal.CanvasEntitled = true
	room := runActionRoomEvidence()
	room.CanvasEntitled = true

	for kind, evidence := range map[string]PersonalThreadDecisionEvidence{"personal": personal, "room": room} {
		for _, actionID := range []string{canvasRunActionReadID, canvasRunActionWriteID} {
			t.Run(kind+"/"+actionID, func(t *testing.T) {
				spec, _ := runActionSpec(actionID)
				decision, err := IssueRunActionDecision(evidence, runActionAuthorityFor(evidence), goldenRunActionRequest(t, actionID), now)
				if err != nil {
					t.Fatalf("IssueRunActionDecision: %v", err)
				}
				if decision.ActionID != actionID || decision.ServiceAudience != spec.ServiceAudience || decision.ActionSchemaHash != spec.SchemaSHA256 ||
					len(decision.Permissions) != 1 || decision.Permissions[0] != spec.Permission || decision.SpaceRef != evidence.Membership.SpaceRef {
					t.Fatalf("canvas decision = %+v", decision)
				}
				if err := decision.Validate(); err != nil {
					t.Fatalf("issued canvas decision did not validate: %v", err)
				}
				if err := ValidateCurrentRunActionDecision(evidence, decision); err != nil {
					t.Fatalf("issued canvas decision failed the effect-time check: %v", err)
				}
			})
		}
	}
}

func TestRunActionCanvasFactsLeaveTicketsCreateUnchanged(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	base := runActionRoomEvidence()
	want, err := IssueRunActionDecision(base, runActionAuthorityFor(base), validRunActionRequest(), now)
	if err != nil {
		t.Fatalf("IssueRunActionDecision tickets.create: %v", err)
	}
	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence){
		"canvas entitled":          func(e *PersonalThreadDecisionEvidence) { e.CanvasEntitled = true },
		"since_join history floor": func(e *PersonalThreadDecisionEvidence) { e.CanvasEntitled, e.HistoryFloorRevision = true, 3 },
		"floor with canvas off":    func(e *PersonalThreadDecisionEvidence) { e.HistoryFloorRevision = 3 },
	} {
		t.Run(name, func(t *testing.T) {
			evidence := base
			mutate(&evidence)
			got, err := IssueRunActionDecision(evidence, runActionAuthorityFor(evidence), validRunActionRequest(), now)
			if err != nil {
				t.Fatalf("tickets.create refused: %v", err)
			}
			if !reflect.DeepEqual(got, want) {
				t.Fatalf("tickets.create decision changed:\n got %+v\nwant %+v", got, want)
			}
			if err := ValidateCurrentRunActionDecision(evidence, got); err != nil {
				t.Fatalf("tickets.create failed the effect-time check: %v", err)
			}
		})
	}
}
