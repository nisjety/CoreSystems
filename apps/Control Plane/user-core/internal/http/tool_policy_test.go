package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S5a: the Space tool policy routes. Pure tier: delegation guard, body
// shape and the fail-closed answers that need no database. The role, kind,
// archive and revision legs are live (TEST_DATABASE_URL) below.

const toolPolicyBase = "/api/v1/internal/spaces/room-1"

func TestToolPolicyRoutesNeedAVerifiedDelegation(t *testing.T) {
	ctx := gatewayContext()
	delete(ctx, "delegation_verified")
	engine := approvalHarness(t, &spaces.Repository{}, nil, ctx)
	for _, call := range []struct{ method, path string }{
		{http.MethodGet, toolPolicyBase + "/tool-policy"},
		{http.MethodPut, toolPolicyBase + "/tool-policy"},
		{http.MethodPut, toolPolicyBase + "/agents/agent-1/tool-policy"},
	} {
		rec := doJSON(engine, call.method, call.path, `{"entries":[],"expected_revision":0}`)
		if rec.Code != http.StatusForbidden && rec.Code != http.StatusUnauthorized {
			t.Errorf("%s %s without delegation: %d %s", call.method, call.path, rec.Code, rec.Body.String())
		}
	}
}

func TestToolPolicyWriteBodiesAreValidatedBeforeAnyLookup(t *testing.T) {
	// An empty repository has no database: a 400 here proves the body check
	// runs before membership or storage is touched.
	engine := approvalHarness(t, &spaces.Repository{}, nil, gatewayContext())
	long := make([]map[string]string, 0, spaces.MaxToolPolicyEntries+1)
	for i := 0; i <= spaces.MaxToolPolicyEntries; i++ {
		long = append(long, map[string]string{"tool_key": "t" + strings.Repeat("x", i%7) + string(rune('a'+i%26)) + strings.Repeat("z", i/26), "level": "ask"})
	}
	cases := map[string]any{
		"duplicate key":     map[string]any{"entries": []map[string]string{{"tool_key": "a", "level": "ask"}, {"tool_key": "a", "level": "block"}}, "expected_revision": 0},
		"unknown level":     map[string]any{"entries": []map[string]string{{"tool_key": "a", "level": "deny"}}, "expected_revision": 0},
		"whitespace key":    map[string]any{"entries": []map[string]string{{"tool_key": "a b", "level": "ask"}}, "expected_revision": 0},
		"empty key":         map[string]any{"entries": []map[string]string{{"tool_key": "", "level": "ask"}}, "expected_revision": 0},
		"negative revision": map[string]any{"entries": []map[string]string{}, "expected_revision": -1},
		"too many entries":  map[string]any{"entries": long, "expected_revision": 0},
	}
	for _, body := range cases {
		for _, path := range []string{toolPolicyBase + "/tool-policy", toolPolicyBase + "/agents/agent-1/tool-policy"} {
			rec := doJSON(engine, http.MethodPut, path, body)
			wantApprovalError(t, rec, http.StatusBadRequest, "tool_policy_invalid")
		}
	}
	// Unknown top-level fields are refused by the strict decoder, and an actor
	// can never be named in the body.
	for _, extra := range []string{"actor", "org_id", "role", "entitled"} {
		rec := doJSON(engine, http.MethodPut, toolPolicyBase+"/tool-policy", map[string]any{"entries": []any{}, "expected_revision": 0, extra: "x"})
		wantApprovalError(t, rec, http.StatusBadRequest, "invalid_request")
	}
	wantApprovalError(t, doJSON(engine, http.MethodPut, toolPolicyBase+"/tool-policy", "[1,2"), http.StatusBadRequest, "invalid_request")
}

func TestToolPolicyRoutesFailClosedWithoutARepository(t *testing.T) {
	engine := approvalHarness(t, nil, nil, gatewayContext())
	for _, call := range []struct{ method, path string }{
		{http.MethodGet, toolPolicyBase + "/tool-policy"},
		{http.MethodPut, toolPolicyBase + "/tool-policy"},
		{http.MethodPut, toolPolicyBase + "/agents/agent-1/tool-policy"},
	} {
		rec := doJSON(engine, call.method, call.path, `{"entries":[],"expected_revision":0}`)
		wantApprovalError(t, rec, http.StatusServiceUnavailable, "approval_authority_unavailable")
	}
}

func TestToolPolicyErrorMapping(t *testing.T) {
	gin.SetMode(gin.TestMode)
	for _, tc := range []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{"revision conflict", spaces.ErrToolPolicyConflict, http.StatusConflict, "tool_policy_revision_conflict"},
		{"agent not in space", spaces.ErrApprovalAgentNotInSpace, http.StatusConflict, "approval_agent_not_in_space"},
		{"no membership", spaces.ErrNoCurrentMembership, http.StatusNotFound, "space_not_found"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := httptest.NewRecorder()
			c, _ := gin.CreateTestContext(rec)
			c.Request = httptest.NewRequest(http.MethodGet, "/", nil)
			writeToolPolicyError(c, tc.err)
			wantApprovalError(t, rec, tc.status, tc.code)
		})
	}
}

// ---- live legs -----------------------------------------------------------

func policyPutBody(revision int64, entries ...[2]string) map[string]any {
	list := make([]map[string]string, 0, len(entries))
	for _, e := range entries {
		list = append(list, map[string]string{"tool_key": e[0], "level": e[1]})
	}
	return map[string]any{"entries": list, "expected_revision": revision}
}

func TestToolPolicyRoutesTypedRefusalsAndRoundTripLive(t *testing.T) {
	room, repo, exec := seedApprovalRoom(t, "ws10s5h", nil)
	pool := d12Pool(t)
	// Registered after the seed's cleanup, so it runs first (the policy tables
	// hold foreign keys to registered_spaces).
	t.Cleanup(func() {
		for _, table := range []string{"space_tool_policies", "space_tool_policy_sets"} {
			_, _ = pool.Exec(context.Background(), `DELETE FROM `+table+` WHERE space_ref = $1`, room.Room)
		}
	})
	base := "/api/v1/internal/spaces/" + room.Room
	as := func(user string) *gin.Engine { return liveApprovalEngine(t, repo, nil, room.Org, user) }

	// Editors and viewers are refused with the typed role code; readers are not.
	for _, user := range []string{room.Editor, room.Viewer} {
		body := wantApprovalError(t, doJSON(as(user), http.MethodPut, base+"/tool-policy", policyPutBody(0, [2]string{"shell", "block"})),
			http.StatusForbidden, "tool_policy_role_required")
		if body.Error.Details["required_roles"] == nil {
			t.Errorf("%s: the refusal should name the required roles: %v", user, body.Error.Details)
		}
		wantApprovalError(t, doJSON(as(user), http.MethodPut, base+"/agents/"+room.Agent+"/tool-policy", policyPutBody(0, [2]string{"shell", "block"})),
			http.StatusForbidden, "tool_policy_role_required")
	}
	// A non-member sees no Space.
	wantApprovalError(t, doJSON(as("ws10s5h-stranger"), http.MethodGet, base+"/tool-policy", nil), http.StatusNotFound, "space_not_found")

	// A manager writes the ceiling; the first write needs revision 0.
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPut, base+"/tool-policy", policyPutBody(1, [2]string{"shell", "block"})),
		http.StatusConflict, "tool_policy_revision_conflict")
	rec := doJSON(as(room.Manager), http.MethodPut, base+"/tool-policy", policyPutBody(0, [2]string{"*", "ask"}, [2]string{"shell", "block"}))
	if rec.Code != http.StatusOK {
		t.Fatalf("ceiling write: %d %s", rec.Code, rec.Body.String())
	}
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPut, base+"/tool-policy", policyPutBody(0, [2]string{"shell", "allow"})),
		http.StatusConflict, "tool_policy_revision_conflict")

	// The agent layer for an agent that is not in the Space is refused.
	wantApprovalError(t, doJSON(as(room.Owner), http.MethodPut, base+"/agents/not-in-this-space/tool-policy", policyPutBody(0, [2]string{"x", "ask"})),
		http.StatusConflict, "approval_agent_not_in_space")
	if rec := doJSON(as(room.Owner), http.MethodPut, base+"/agents/"+room.Agent+"/tool-policy", policyPutBody(0, [2]string{"send", "block"})); rec.Code != http.StatusOK {
		t.Fatalf("agent layer write: %d %s", rec.Code, rec.Body.String())
	}

	// Any member (even a viewer) can GET, with the merged view for a named agent.
	rec = doJSON(as(room.Viewer), http.MethodGet, base+"/tool-policy?agent_subject_id="+room.Agent, nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("member GET: %d %s", rec.Code, rec.Body.String())
	}
	var got struct {
		Data struct {
			Ceiling spaces.ToolPolicySet   `json:"ceiling"`
			Agents  []spaces.ToolPolicySet `json:"agents"`
			Merged  spaces.MergedToolPolicy `json:"merged"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.Data.Ceiling.Revision != 1 || len(got.Data.Agents) != 1 || got.Data.Merged.Tools["send"].Level != spaces.ToolLevelBlock ||
		got.Data.Merged.Tools["shell"].Source != spaces.ToolPolicyLayerSpaceCeiling {
		t.Fatalf("GET body = %+v", got.Data)
	}

	// An archived Space is read-only: writes are refused, reads still work.
	exec(`UPDATE registered_spaces SET registration_state='archived' WHERE space_ref=$1`, room.Room)
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPut, base+"/tool-policy", policyPutBody(1, [2]string{"shell", "allow"})),
		http.StatusNotFound, "space_not_found")
	if rec := doJSON(as(room.Viewer), http.MethodGet, base+"/tool-policy", nil); rec.Code != http.StatusOK {
		t.Fatalf("archived read: %d %s", rec.Code, rec.Body.String())
	}
}

func TestToolPolicyIsUnsupportedOnAPersonalSpaceLive(t *testing.T) {
	room, repo, exec := seedApprovalRoom(t, "ws10s5p", nil)
	pool := d12Pool(t)
	t.Cleanup(func() {
		for _, table := range []string{"space_tool_policies", "space_tool_policy_sets"} {
			_, _ = pool.Exec(context.Background(), `DELETE FROM `+table+` WHERE space_ref = $1`, room.Room)
		}
	})
	exec(`UPDATE registered_spaces SET space_kind='personal' WHERE space_ref=$1`, room.Room)
	base := "/api/v1/internal/spaces/" + room.Room
	engine := liveApprovalEngine(t, repo, nil, room.Org, room.Owner)
	wantApprovalError(t, doJSON(engine, http.MethodPut, base+"/tool-policy", policyPutBody(0, [2]string{"shell", "block"})),
		http.StatusForbidden, "space_kind_unsupported")
}
