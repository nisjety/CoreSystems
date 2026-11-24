package spaces

import (
	"crypto/ed25519"
	"encoding/json"
	"slices"
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

// A v2 view names only the rows the operator allowlist (CONTROL_SPACE_SELF_API_ACTIONS)
// names and the caller's entitlements admit; an unentitled caller or an empty
// allowlist yields no view at all.
func TestIssueModelActionViewIntersectsEntitlement(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	evidence.ResourceAuthorizationRef = "control:space-personal:thread-create:7"
	request := ModelActionViewRequest{
		DecisionRef: "view-1",
		Nonce:       "nonce-1",
	}
	// The allowlist names the canvas rows too; canvas_entitled is off, so only
	// tickets.create is listed.
	t.Setenv(spaceSelfAPIActionsEnv, "tickets.create,space.canvas.read,space.canvas.write")

	view, err := IssueModelActionViewV2(evidence, validRunActionAuthority(), request, now)
	if err != nil {
		t.Fatalf("IssueModelActionViewV2: %v", err)
	}
	if len(view.Actions) != 1 || view.Actions[0].ActionID != "tickets.create" || view.Actions[0].ActionSchemaHash != ticketsCreateActionSchemaSHA256 {
		t.Fatalf("v2 view names %#v; want only tickets.create", view.Actions)
	}
	canvas := evidence
	canvas.CanvasEntitled = true
	withCanvas, err := IssueModelActionViewV2(canvas, validRunActionAuthority(), request, now)
	if err != nil {
		t.Fatalf("IssueModelActionViewV2 with the canvas entitlement: %v", err)
	}
	if got := modelActionViewV2IDs(withCanvas); !slices.Equal(got, []string{"space.canvas.read", "space.canvas.write", "tickets.create"}) {
		t.Fatalf("v2 view with the canvas entitlement names %v", got)
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
	t.Setenv(spaceSelfAPIActionsEnv, "")
	if _, err := IssueModelActionViewV2(evidence, validRunActionAuthority(), request, now); err == nil {
		t.Fatal("a v2 view admitting no requested action was issued")
	}
}

// modelActionViewV2IDs lists the action ids a v2 view names, in its order.
func modelActionViewV2IDs(view ModelActionViewV2) []string {
	ids := make([]string, 0, len(view.Actions))
	for _, action := range view.Actions {
		ids = append(ids, action.ActionID)
	}
	return ids
}

// WS18-S2: the v2 list is server-owned. A row is listed only when the operator
// allowlist names it, the caller meets the row's role floor and the room's
// entitlements admit it, each with its golden hash and level. The schedule row
// (another decision type) never is, and an empty list issues no view.
func TestModelActionViewV2ListsOnlyEntitledEnabledRows(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	request := ModelActionViewRequest{DecisionRef: "view-1", Nonce: "nonce-1"}
	evidence := runActionRoomEvidence()
	listed := func(t *testing.T, evidence PersonalThreadDecisionEvidence) []string {
		t.Helper()
		view, err := IssueModelActionViewV2(evidence, runActionAuthorityFor(evidence), request, now)
		if err != nil {
			t.Fatalf("IssueModelActionViewV2: %v", err)
		}
		if err := view.Validate(); err != nil {
			t.Fatalf("issued v2 view did not validate: %v", err)
		}
		for _, action := range view.Actions {
			spec, _ := runActionSpec(action.ActionID)
			if action.ActionSchemaHash != spec.SchemaSHA256 || action.Level != spec.DefaultLevel {
				t.Fatalf("row %+v does not carry the golden hash and level", action)
			}
		}
		return modelActionViewV2IDs(view)
	}

	t.Setenv(spaceSelfAPIActionsEnv, strings.Join(runActionRegistryIDs(), ","))
	// A-REC-03-TOOL: the directory resolver is a run-action read at the viewer floor, so an editor's view lists it.
	want := []string{"space.agent.pause_self", "space.directory.resolve", "space.thread.pin", "space.thread.post_card", "space.thread.rename", "tickets.create"}
	if got := listed(t, evidence); !slices.Equal(got, want) {
		t.Fatalf("every golden id allowlisted lists %v, want %v", got, want)
	}

	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename, tickets.create")
	if got := listed(t, evidence); !slices.Equal(got, []string{"space.thread.rename", "tickets.create"}) {
		t.Fatalf("two allowlisted rows list %v", got)
	}

	withRunActionRegistryRow(t, "space.thread.rename", func(row *runActionSpecRow) { row.MinRole = RoleManager })
	if got := listed(t, evidence); !slices.Equal(got, []string{"tickets.create"}) {
		t.Fatalf("an editor below a manager-floor row lists %v", got)
	}
	manager := evidence
	manager.Membership.Role = RoleManager
	if got := listed(t, manager); !slices.Equal(got, []string{"space.thread.rename", "tickets.create"}) {
		t.Fatalf("a manager lists %v", got)
	}

	unentitled := evidence
	unentitled.AgentActionEntitled = false
	viewer := evidence
	viewer.Membership.Role = RoleViewer
	for name, refused := range map[string]PersonalThreadDecisionEvidence{"agent action off": unentitled, "viewer": viewer} {
		if _, err := IssueModelActionViewV2(refused, runActionAuthorityFor(refused), request, now); err == nil {
			t.Errorf("%s: a v2 view was issued", name)
		}
	}
	for _, allowlist := range []string{"", " , ", "space.schedule.follow_up", "users.delete"} {
		t.Setenv(spaceSelfAPIActionsEnv, allowlist)
		if _, err := IssueModelActionViewV2(evidence, runActionAuthorityFor(evidence), request, now); err == nil {
			t.Errorf("allowlist %q issued a v2 view", allowlist)
		}
	}
}

// W3-FIX-12: a canvas row is listed only where the canvas admits it. An unentitled
// room, a direct-message kind and a since_join floor omit both canvas rows and
// keep tickets.create; the v1 view is the same whatever the canvas facts.
func TestModelActionViewV2ListsCanvasRowsOnlyWhereTheCanvasAdmitsThem(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "tickets.create,space.canvas.read,space.canvas.write")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	request := ModelActionViewRequest{DecisionRef: "view-1", Nonce: "nonce-1"}
	room := runActionRoomEvidence()
	room.CanvasEntitled = true

	view, err := IssueModelActionViewV2(room, runActionAuthorityFor(room), request, now)
	if err != nil {
		t.Fatalf("IssueModelActionViewV2: %v", err)
	}
	if got := modelActionViewV2IDs(view); !slices.Equal(got, []string{"space.canvas.read", "space.canvas.write", "tickets.create"}) {
		t.Fatalf("an entitled editor room lists %v", got)
	}
	wantV1, err := IssueModelActionView(room, runActionAuthorityFor(room), request, now)
	if err != nil {
		t.Fatalf("IssueModelActionView: %v", err)
	}

	for name, mutate := range map[string]func(*PersonalThreadDecisionEvidence){
		"unentitled room": func(e *PersonalThreadDecisionEvidence) { e.CanvasEntitled = false },
		"dm room":         func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindDM },
		"group dm room":   func(e *PersonalThreadDecisionEvidence) { e.Membership.Kind = KindGroupDM },
		"floored room":    func(e *PersonalThreadDecisionEvidence) { e.HistoryFloorRevision = 3 },
	} {
		t.Run(name, func(t *testing.T) {
			evidence := room
			mutate(&evidence)
			view, err := IssueModelActionViewV2(evidence, runActionAuthorityFor(evidence), request, now)
			if err != nil {
				t.Fatalf("IssueModelActionViewV2: %v", err)
			}
			if got := modelActionViewV2IDs(view); !slices.Equal(got, []string{"tickets.create"}) {
				t.Fatalf("lists %v, want tickets.create only", got)
			}
			v1, err := IssueModelActionView(evidence, runActionAuthorityFor(evidence), request, now)
			if err != nil {
				t.Fatalf("IssueModelActionView: %v", err)
			}
			got, _ := json.Marshal(v1)
			want, _ := json.Marshal(wantV1)
			if string(got) != string(want) {
				t.Fatalf("the v1 view changed:\n got %s\nwant %s", got, want)
			}
		})
	}
}

// WS18-S2: the v1 view (view_version 0 and 1) is the tickets.create view byte for
// byte as before v2; the allowlist and the canvas facts do not touch it.
func TestModelActionViewV1StillByteIdentical(t *testing.T) {
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	const want = `{"decision_ref":"view-1","run_id":"run-1","thread_id":"thread-1","org_id":"org-1",` +
		`"space_ref":"space-personal","subject_id":"user-1","service_audience":"model-plane-capability-core",` +
		`"action_id":"tickets.create","action_schema_hash":"sha256:c3aa12ec85c2d79f08e5e8cc726fd75af10ddab0b29f2a6e0dddb0bd42bb56df",` +
		`"recipient_audience_ref":"audience:space-personal:2","recipient_audience_hash":"sha256:test-audience",` +
		`"recipient_audience_revision":2,"privacy_policy_ref":"privacy:org-1:5",` +
		`"run_context_authorization_ref":"control:space-personal:thread-create:7","authority_revision":7,` +
		`"permissions":["model-action:view"],"purpose":"assistant_collaboration","lawful_basis":"contract",` +
		`"privacy_class":"internal","third_party_processing_allowed":false,"retention_class":"standard",` +
		`"residency":"swedencentral","deletion_scope":"space","zero_data_retention":false,` +
		`"issued_at":"2026-08-15T12:00:00Z","expires_at":"2026-08-15T12:02:00Z","nonce":"nonce-1"}`
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
	evidence := validPersonalThreadEvidence()
	evidence.AgentActionEntitled = true
	canvas := evidence
	canvas.CanvasEntitled = true
	for _, allowlist := range []string{"", "tickets.create,space.thread.rename,space.canvas.read"} {
		t.Setenv(spaceSelfAPIActionsEnv, allowlist)
		for name, current := range map[string]PersonalThreadDecisionEvidence{"plain": evidence, "canvas entitled": canvas} {
			view, err := IssueModelActionView(current, validRunActionAuthority(), ModelActionViewRequest{DecisionRef: "view-1", Nonce: "nonce-1"}, now)
			if err != nil {
				t.Fatalf("IssueModelActionView (%s, allowlist %q): %v", name, allowlist, err)
			}
			got, err := json.Marshal(view)
			if err != nil {
				t.Fatalf("marshal: %v", err)
			}
			if string(got) != want {
				t.Fatalf("v1 view bytes changed (%s, allowlist %q):\n got %s\nwant %s", name, allowlist, got, want)
			}
			token, err := SignModelActionView(key, view)
			if err != nil || !strings.HasPrefix(token, modelActionViewVersion+".") {
				t.Fatalf("SignModelActionView = %q, %v", token, err)
			}
		}
	}
}

// WS18-S2: a v2 view names each golden run-action row at most once, with exactly
// the golden hash and level. Anything else is refused by Validate and by the signer.
func TestViewV2RejectsUnknownOrDuplicateAction(t *testing.T) {
	t.Setenv(spaceSelfAPIActionsEnv, "space.thread.rename,tickets.create")
	now := time.Date(2026, time.August, 15, 12, 0, 0, 0, time.UTC)
	evidence := runActionRoomEvidence()
	view, err := IssueModelActionViewV2(evidence, runActionAuthorityFor(evidence), ModelActionViewRequest{DecisionRef: "view-1", Nonce: "nonce-1"}, now)
	if err != nil {
		t.Fatalf("IssueModelActionViewV2: %v", err)
	}
	// Registry order: space.thread.rename (allow) then tickets.create (ask).
	if got := modelActionViewV2IDs(view); !slices.Equal(got, []string{"space.thread.rename", "tickets.create"}) {
		t.Fatalf("v2 view lists %v", got)
	}
	key := SigningKey{ID: "control-test", PrivateKey: ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))}
	if _, err := SignModelActionViewV2(key, view); err != nil {
		t.Fatalf("SignModelActionViewV2: %v", err)
	}
	schedule, _ := runActionSpec("space.schedule.follow_up")
	for name, mutate := range map[string]func(*ModelActionViewV2){
		"id outside the golden": func(v *ModelActionViewV2) {
			v.Actions[0] = ModelActionRowV2{ActionID: "users.delete", ActionSchemaHash: "sha256:" + strings.Repeat("d", 64), Level: "allow"}
		},
		"padded id":           func(v *ModelActionViewV2) { v.Actions[0].ActionID = " " + v.Actions[0].ActionID },
		"repeated id":         func(v *ModelActionViewV2) { v.Actions[1] = v.Actions[0] },
		"hash not the golden": func(v *ModelActionViewV2) { v.Actions[0].ActionSchemaHash = "sha256:" + strings.Repeat("e", 64) },
		"another row's hash": func(v *ModelActionViewV2) {
			v.Actions[0].ActionSchemaHash, v.Actions[1].ActionSchemaHash = v.Actions[1].ActionSchemaHash, v.Actions[0].ActionSchemaHash
		},
		"level not the golden": func(v *ModelActionViewV2) { v.Actions[1].Level = "allow" },
		"unknown level":        func(v *ModelActionViewV2) { v.Actions[0].Level = "deny" },
		"schedule row": func(v *ModelActionViewV2) {
			v.Actions[0] = ModelActionRowV2{ActionID: schedule.ActionID, ActionSchemaHash: schedule.SchemaSHA256, Level: schedule.DefaultLevel}
		},
		"no action": func(v *ModelActionViewV2) { v.Actions = nil },
		"over the cap": func(v *ModelActionViewV2) {
			for len(v.Actions) <= maxModelActionViewV2Actions {
				v.Actions = append(v.Actions, v.Actions[0])
			}
		},
	} {
		t.Run(name, func(t *testing.T) {
			mutated := view
			mutated.Actions = append([]ModelActionRowV2(nil), view.Actions...)
			mutate(&mutated)
			if err := mutated.Validate(); err == nil {
				t.Fatalf("a v2 view naming %+v validated", mutated.Actions)
			}
			if _, err := SignModelActionViewV2(key, mutated); err == nil {
				t.Fatal("a v2 view the registry refuses was signed")
			}
		})
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
