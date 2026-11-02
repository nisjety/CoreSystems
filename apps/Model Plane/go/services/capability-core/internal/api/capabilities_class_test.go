package api

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// The class vocabulary is declared by the registry, never by a catalog write:
// the generic upsert refuses a body that carries a class or floor, and refuses
// the mcp_tool kind outright. Neither refusal reaches the store, so a nil store
// proves the request was stopped at the handler.
func TestUpsertRefusesClassFieldsAndTheMCPToolKind(t *testing.T) {
	t.Parallel()

	for name, body := range map[string]string{
		"mcp_tool kind":            `{"ID":"cap.x","Kind":"mcp_tool","Name":"x","RiskLevel":"low"}`,
		"side_effect_class":        `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","SideEffectClass":"read"}`,
		"approval_floor":           `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","ApprovalFloor":"none"}`,
		"class lowering a seed id": `{"ID":"cap.command.shell","Kind":"tool","Name":"shell","RiskLevel":"high","SideEffectClass":"read","ApprovalFloor":"none"}`,
	} {
		body := body
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/upsert", strings.NewReader(body))
			recorder := httptest.NewRecorder()
			NewCapabilitiesHandler(nil).upsert(recorder, request)
			if recorder.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400; body = %s", recorder.Code, recorder.Body.String())
			}
		})
	}
}

// registry.CapabilityRow has no json tags, so the documented snake_case keys
// match none of its fields. The refusal reads the raw keys instead, so every
// spelling is refused with the same 400 and none is silently dropped to a 201.
func TestUpsertRefusesTheClassKeysUnderEverySpelling(t *testing.T) {
	t.Parallel()

	for name, body := range map[string]string{
		"snake_case side_effect_class":    `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","side_effect_class":"read"}`,
		"snake_case approval_floor":       `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","approval_floor":"none"}`,
		"snake_case both, no other keys":  `{"side_effect_class":"read","approval_floor":"none"}`,
		"snake_case with snake risk":      `{"id":"cap.x","kind":"tool","name":"x","risk_level":"low","side_effect_class":"read","approval_floor":"none"}`,
		"Go field name":                   `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","SideEffectClass":"read"}`,
		"camelCase side effect class":     `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","sideEffectClass":"read"}`,
		"camelCase approval floor":        `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","approvalFloor":"none"}`,
		"upper snake":                     `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","SIDE_EFFECT_CLASS":"read"}`,
		"mixed underscores":               `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","Approval_Floor":"none"}`,
		"null value still names the key":  `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","side_effect_class":null}`,
		"empty value still names the key": `{"ID":"cap.x","Kind":"tool","Name":"x","RiskLevel":"low","approval_floor":""}`,
	} {
		body := body
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/upsert", strings.NewReader(body))
			recorder := httptest.NewRecorder()
			// The handler is built with a nil store: a request that is not refused
			// before the store would dereference it. Report that as the failure it
			// is (the class was dropped and the write went on) instead of a panic.
			defer func() {
				if recovered := recover(); recovered != nil {
					t.Fatalf("the body reached the store, so the class key was dropped instead of refused: %v", recovered)
				}
			}()
			NewCapabilitiesHandler(nil).upsert(recorder, request)
			if recorder.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (never a 201 that dropped the class); body = %s", recorder.Code, recorder.Body.String())
			}
			if !strings.Contains(recorder.Body.String(), "side_effect_class and approval_floor are read-only") {
				t.Fatalf("refusal is not the class refusal: %s", recorder.Body.String())
			}
		})
	}
}

func TestCarriesReadOnlyClassFieldDoesNotRefuseNeighbouringKeys(t *testing.T) {
	t.Parallel()

	for _, key := range []string{"id", "kind", "risk_level", "RiskLevel", "side_effect", "class", "approval", "floor", "side_effect_classes", "approval_floors", "tags"} {
		if carriesReadOnlyClassField(map[string]json.RawMessage{key: json.RawMessage(`"x"`)}) {
			t.Errorf("%q must not be taken for the class or floor", key)
		}
	}
	if carriesReadOnlyClassField(nil) {
		t.Error("an empty body names no class")
	}
}

func TestCapabilityViewReportsTheStrictestClassForAnUnsetOrUnknownValue(t *testing.T) {
	t.Parallel()

	if capabilityView(nil) != nil {
		t.Fatal("a nil row must produce a nil view")
	}
	for _, tc := range []struct {
		name                 string
		class, floor         string
		wantClass, wantFloor string
	}{
		// An unset floor is the strictest floor, as in the Rust twin; an
		// unclassified class never reads below an ask.
		{"unset", "", "", models.SideEffectUnclassified, models.ApprovalFloorDurableOwner},
		{"unclassified with no floor stored", models.SideEffectUnclassified, models.ApprovalFloorNone, models.SideEffectUnclassified, models.ApprovalFloorAsk},
		{"unknown class", "readonly", "none", models.SideEffectUnclassified, models.ApprovalFloorAsk},
		{"read with an unset floor", models.SideEffectRead, "", models.SideEffectRead, models.ApprovalFloorDurableOwner},
		{"read", models.SideEffectRead, models.ApprovalFloorNone, models.SideEffectRead, models.ApprovalFloorNone},
		{"read with ask floor", models.SideEffectRead, models.ApprovalFloorAsk, models.SideEffectRead, models.ApprovalFloorAsk},
		{"owner effect never reads below its floor", models.SideEffectOwnerEffect, models.ApprovalFloorNone, models.SideEffectOwnerEffect, models.ApprovalFloorDurableOwner},
		{"unknown floor", models.SideEffectRead, "required", models.SideEffectRead, models.ApprovalFloorDurableOwner},
	} {
		view := capabilityView(&registry.CapabilityRow{ID: "cap.x", RiskLevel: models.RiskLow, SideEffectClass: tc.class, ApprovalFloor: tc.floor})
		if view["side_effect_class"] != tc.wantClass || view["approval_floor"] != tc.wantFloor {
			t.Errorf("%s: view class/floor = %v/%v, want %s/%s", tc.name, view["side_effect_class"], view["approval_floor"], tc.wantClass, tc.wantFloor)
		}
	}
}

func TestListToolsIsReadOnly(t *testing.T) {
	t.Parallel()

	for _, method := range []string{http.MethodPost, http.MethodPut, http.MethodPatch, http.MethodDelete} {
		recorder := httptest.NewRecorder()
		NewCapabilitiesHandler(nil).listTools(recorder, httptest.NewRequest(method, "/api/v1/capabilities/tools", nil))
		if recorder.Code != http.StatusMethodNotAllowed {
			t.Errorf("%s: status = %d, want 405", method, recorder.Code)
		}
	}
}
