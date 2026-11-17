package api

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// spaceAdmissionFromGolden builds the admission row migration 0045 seeds for a Space run action, from its golden registry
// row, so the row and the golden agree on capability, parameters, hash and approval. status is the row's state.
func spaceAdmissionFromGolden(t *testing.T, actionID, status string) *registry.AgentActionAdmission {
	t.Helper()
	golden, found := spaceSelfAPIRowFor(actionID)
	if !found {
		t.Fatalf("the golden registry has no %s", actionID)
	}
	return &registry.AgentActionAdmission{
		ActionID:         actionID,
		ToolName:         actionID,
		CapabilityID:     golden.CapabilityID,
		ModelAccess:      "write_approval",
		SideEffectClass:  "owner_effect",
		ApprovalFloor:    "durable_owner",
		Authority:        "control_run_decision",
		HashBasis:        "owner_contract",
		ActionSchemaHash: golden.SchemaSHA256,
		ParametersJSON:   golden.ParametersJSON,
		Description:      "Space action " + actionID,
		RequiresApproval: golden.Approval == "owner_approval",
		Status:           status,
	}
}

// perCapabilityAvailabilityStore answers the view's health reads from one row per capability id, so a single view can
// judge several Space actions on their own health. A capability with no row answers nil.
type perCapabilityAvailabilityStore struct {
	rows map[string]*registry.CapabilityRow
}

func (store perCapabilityAvailabilityStore) GetForOrg(_ context.Context, capabilityID, _ string) (*registry.CapabilityRow, error) {
	return store.rows[capabilityID], nil
}

func (store perCapabilityAvailabilityStore) GetGlobal(_ context.Context, capabilityID string) (*registry.CapabilityRow, error) {
	return store.rows[capabilityID], nil
}

// The model action view never attests health, so these two report nothing persisted.
func (store perCapabilityAvailabilityStore) AttestAvailabilityForOrg(context.Context, string, string, string, registry.AvailabilityUpdate) (bool, error) {
	return false, nil
}

func (store perCapabilityAvailabilityStore) AttestAvailabilityGlobal(context.Context, string, string, registry.AvailabilityUpdate) (bool, error) {
	return false, nil
}

// attestedSpaceCapability is a capability row a health report has reached: enabled, available, agentic and bounded.
func attestedSpaceCapability(capabilityID string, checkedAt time.Time) *registry.CapabilityRow {
	return &registry.CapabilityRow{
		ID: capabilityID, OrgID: "global", Version: "1", Enabled: true, RiskLevel: models.RiskLow,
		AvailabilityState: string(models.AvailabilityAvailable), ExecutionMode: models.ExecutionAgentic,
		CostClass: models.CostBounded, HealthCheckedAt: &checkedAt,
	}
}

// unattestedSpaceCapability is an enabled capability no health report has reached yet, so it is not available.
func unattestedSpaceCapability(capabilityID string) *registry.CapabilityRow {
	return &registry.CapabilityRow{ID: capabilityID, OrgID: "global", Version: "1", Enabled: true, RiskLevel: models.RiskLow}
}

// serveSpaceModelActionView signs a v2 view that admits the given action hashes, serves it through the handler with the
// admission rows and capability rows, and returns the response.
func serveSpaceModelActionView(t *testing.T, rows []*registry.AgentActionAdmission, capabilities map[string]*registry.CapabilityRow, hashes map[string]string) *httptest.ResponseRecorder {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	verifier.WithAgentActionAdmissions(&fakeAgentActionAdmissions{rows: rows}, true)
	handler := (&CapabilitiesHandler{availabilityStore: perCapabilityAvailabilityStore{rows: capabilities}}).WithModelActionViewVerifier(verifier)
	token := signControlModelActionViewV2(t, private, "control-key", validControlModelActionViewV2(time.Now().UTC(), hashes))
	return modelActionViewRecorder(t, handler, "run-1", token)
}

// decodeSpaceModelActionView requires a 200 answer and returns the decoded view.
func decodeSpaceModelActionView(t *testing.T, recorder *httptest.ResponseRecorder) modelActionViewResponse {
	t.Helper()
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	var view modelActionViewResponse
	if err := json.NewDecoder(recorder.Body).Decode(&view); err != nil {
		t.Fatal(err)
	}
	return view
}

func actionNames(view modelActionViewResponse) []string {
	names := make([]string, 0, len(view.Data.Actions))
	for _, action := range view.Data.Actions {
		names = append(names, action.Name)
	}
	return names
}

// WS18-S2: a v2 row is offered from the golden registry, with the level Control signed and the autonomy rung the golden
// names. The rename and pin rows keep the definition the golden hashes.
func TestModelActionViewOffersSpaceRowsWithTheGoldenAutonomyRung(t *testing.T) {
	now := time.Now().UTC()
	rows := []*registry.AgentActionAdmission{
		spaceAdmissionFromGolden(t, "space.thread.rename", "enabled"),
		spaceAdmissionFromGolden(t, "space.thread.pin", "enabled"),
	}
	capabilities := map[string]*registry.CapabilityRow{
		spaceThreadRenameCapabilityID: attestedSpaceCapability(spaceThreadRenameCapabilityID, now),
		spaceThreadPinCapabilityID:    attestedSpaceCapability(spaceThreadPinCapabilityID, now),
	}
	hashes := map[string]string{rows[0].ActionID: rows[0].ActionSchemaHash, rows[1].ActionID: rows[1].ActionSchemaHash}
	view := decodeSpaceModelActionView(t, serveSpaceModelActionView(t, rows, capabilities, hashes))

	// Candidates are offered in action-id order: pin sorts before rename.
	if got := actionNames(view); !slices.Equal(got, []string{"space.thread.pin", "space.thread.rename"}) {
		t.Fatalf("offered = %v, want pin then rename", got)
	}
	golden, _ := spaceSelfAPIRowFor("space.thread.rename")
	for _, action := range view.Data.Actions {
		if action.Level != "allow" {
			t.Fatalf("%s level = %q, want the signed allow", action.Name, action.Level)
		}
		if action.AutonomyRung != "workspace_write" {
			t.Fatalf("%s autonomy rung = %q, want the golden workspace_write", action.Name, action.AutonomyRung)
		}
		if action.RequiresApproval {
			t.Fatalf("%s requires approval, the golden approval is none", action.Name)
		}
	}
	if view.Data.Actions[1].ActionSchemaHash != golden.SchemaSHA256 || view.Data.Actions[1].ParametersJSON != golden.ParametersJSON {
		t.Fatalf("rename definition is not the golden one: %#v", view.Data.Actions[1])
	}
}

// WS18-S2: a golden mismatch on the schema hash, capability, parameters or approval refuses the whole view, as does an
// id the golden registry does not list and a row another decision type owns. Nothing is offered around the signal.
func TestModelActionViewRefusesTheWholeViewOnAGoldenMismatch(t *testing.T) {
	mismatches := []struct {
		name   string
		mutate func(row *registry.AgentActionAdmission)
	}{
		{"schema hash", func(row *registry.AgentActionAdmission) { row.ActionSchemaHash = "sha256:" + strings.Repeat("1", 64) }},
		{"capability", func(row *registry.AgentActionAdmission) { row.CapabilityID = spaceThreadPinCapabilityID }},
		{"parameters", func(row *registry.AgentActionAdmission) { row.ParametersJSON = `{"type":"object"}` }},
		{"approval", func(row *registry.AgentActionAdmission) { row.RequiresApproval = true }},
	}
	for _, tc := range mismatches {
		t.Run(tc.name, func(t *testing.T) {
			row := spaceAdmissionFromGolden(t, "space.thread.rename", "enabled")
			tc.mutate(row)
			capabilities := map[string]*registry.CapabilityRow{
				spaceThreadRenameCapabilityID: attestedSpaceCapability(spaceThreadRenameCapabilityID, time.Now().UTC()),
			}
			recorder := serveSpaceModelActionView(t, []*registry.AgentActionAdmission{row}, capabilities,
				map[string]string{row.ActionID: row.ActionSchemaHash})
			if recorder.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 for a golden %s mismatch, body = %s", recorder.Code, tc.name, recorder.Body.String())
			}
		})
	}

	unlisted := spaceAdmissionFromGolden(t, "space.thread.rename", "enabled")
	unlisted.ActionID = "space.thread.unlisted"
	scheduled := spaceAdmissionFromGolden(t, "space.schedule.follow_up", "enabled")
	for name, row := range map[string]*registry.AgentActionAdmission{"unlisted id": unlisted, "schedule decision": scheduled} {
		t.Run(name, func(t *testing.T) {
			recorder := serveSpaceModelActionView(t, []*registry.AgentActionAdmission{row},
				map[string]*registry.CapabilityRow{row.CapabilityID: attestedSpaceCapability(row.CapabilityID, time.Now().UTC())},
				map[string]string{row.ActionID: row.ActionSchemaHash})
			if recorder.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403, body = %s", recorder.Code, recorder.Body.String())
			}
		})
	}
}

// WS18-S2: an admitted row whose capability is not available is omitted, the first reason is reported once, and every
// later admitted row is still judged on its own health. Here pin has no health report, rename is disabled and post_card
// is available: only post_card is offered, and the reason is pin's, not rename's.
func TestModelActionViewOmitsAnUnavailableRowAndKeepsTheLaterOnes(t *testing.T) {
	now := time.Now().UTC()
	rows := []*registry.AgentActionAdmission{
		spaceAdmissionFromGolden(t, "space.thread.pin", "enabled"),
		spaceAdmissionFromGolden(t, "space.thread.post_card", "enabled"),
		spaceAdmissionFromGolden(t, "space.thread.rename", "enabled"),
	}
	disabledRename := attestedSpaceCapability(spaceThreadRenameCapabilityID, now)
	disabledRename.Enabled = false
	capabilities := map[string]*registry.CapabilityRow{
		spaceThreadPinCapabilityID:      unattestedSpaceCapability(spaceThreadPinCapabilityID),
		spaceThreadPostCardCapabilityID: attestedSpaceCapability(spaceThreadPostCardCapabilityID, now),
		spaceThreadRenameCapabilityID:   disabledRename,
	}
	hashes := map[string]string{}
	for _, row := range rows {
		hashes[row.ActionID] = row.ActionSchemaHash
	}
	view := decodeSpaceModelActionView(t, serveSpaceModelActionView(t, rows, capabilities, hashes))
	if got := actionNames(view); !slices.Equal(got, []string{"space.thread.post_card"}) {
		t.Fatalf("offered = %v, want only post_card", got)
	}
	if view.Data.ReasonCode != "health_not_attested" {
		t.Fatalf("reason = %q, want the first reason, health_not_attested", view.Data.ReasonCode)
	}
}

// W3-DEC-CARD: the v2 view offers space.thread.post_card only when its admission row is enabled and its capability row is
// available. rename and pin are offered in every case, with the same definitions.
func TestModelActionViewOffersPostCardOnlyWhenItsRowIsEnabledAndAvailable(t *testing.T) {
	now := time.Now().UTC()
	cases := []struct {
		name       string
		status     string
		postCard   *registry.CapabilityRow
		wantNames  []string
		wantReason string
	}{
		{
			name:      "a declared row is not offered even when its capability is available",
			status:    "declared",
			postCard:  attestedSpaceCapability(spaceThreadPostCardCapabilityID, now),
			wantNames: []string{"space.thread.pin", "space.thread.rename"},
		},
		{
			name:      "an enabled row with an available capability is offered",
			status:    "enabled",
			postCard:  attestedSpaceCapability(spaceThreadPostCardCapabilityID, now),
			wantNames: []string{"space.thread.pin", "space.thread.post_card", "space.thread.rename"},
		},
		{
			name:       "an enabled row with an unattested capability is omitted with its reason",
			status:     "enabled",
			postCard:   unattestedSpaceCapability(spaceThreadPostCardCapabilityID),
			wantNames:  []string{"space.thread.pin", "space.thread.rename"},
			wantReason: "health_not_attested",
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rows := []*registry.AgentActionAdmission{
				spaceAdmissionFromGolden(t, "space.thread.pin", "enabled"),
				spaceAdmissionFromGolden(t, "space.thread.rename", "enabled"),
				spaceAdmissionFromGolden(t, "space.thread.post_card", tc.status),
			}
			capabilities := map[string]*registry.CapabilityRow{
				spaceThreadPinCapabilityID:      attestedSpaceCapability(spaceThreadPinCapabilityID, now),
				spaceThreadRenameCapabilityID:   attestedSpaceCapability(spaceThreadRenameCapabilityID, now),
				spaceThreadPostCardCapabilityID: tc.postCard,
			}
			hashes := map[string]string{}
			for _, row := range rows {
				hashes[row.ActionID] = row.ActionSchemaHash
			}
			view := decodeSpaceModelActionView(t, serveSpaceModelActionView(t, rows, capabilities, hashes))
			if got := actionNames(view); !slices.Equal(got, tc.wantNames) {
				t.Fatalf("offered = %v, want %v", got, tc.wantNames)
			}
			if view.Data.ReasonCode != tc.wantReason {
				t.Fatalf("reason = %q, want %q", view.Data.ReasonCode, tc.wantReason)
			}
		})
	}
}
