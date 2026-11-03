package http

import (
	"context"
	"encoding/json"
	"net/http"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S5a live leg of tool-gate-view: the merged tools carry their source and
// the policy digest moves when a tool level changes. TEST_DATABASE_URL via d12Pool.

type gateViewTools struct {
	Data struct {
		Digest string                            `json:"policy_digest"`
		Agent  string                            `json:"agent_subject_id"`
		Tools  map[string]spaces.MergedToolLevel `json:"tools"`
	} `json:"data"`
}

func TestToolGateViewReturnsMergedToolsAndADigestThatMovesLive(t *testing.T) {
	room, repo, _ := seedApprovalRoom(t, "ws10s5g", nil)
	pool := d12Pool(t)
	t.Cleanup(func() {
		for _, table := range []string{"space_tool_policies", "space_tool_policy_sets"} {
			_, _ = pool.Exec(context.Background(), `DELETE FROM `+table+` WHERE space_ref = $1`, room.Room)
		}
	})
	base := "/api/v1/internal/spaces/" + room.Room
	manager := liveApprovalEngine(t, repo, nil, room.Org, room.Manager)
	authority := liveRunAuthority(t, repo, room, room.Editor)
	view := func() gateViewTools {
		t.Helper()
		engine := approvalHarness(t, repo, fakeRunAuthority{authority: authority}, executionCoreContext(spaceToolGateViewScope))
		rec := doJSON(engine, http.MethodPost, toolGateViewPath, gateBody(room))
		if rec.Code != http.StatusOK {
			t.Fatalf("gate view: %d %s", rec.Code, rec.Body.String())
		}
		var out gateViewTools
		if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
			t.Fatalf("decode: %v", err)
		}
		return out
	}
	put := func(engine *gin.Engine, path string, revision int64, entries ...[2]string) {
		t.Helper()
		if rec := doJSON(engine, http.MethodPut, path, policyPutBody(revision, entries...)); rec.Code != http.StatusOK {
			t.Fatalf("PUT %s: %d %s", path, rec.Code, rec.Body.String())
		}
	}

	empty := view()
	if len(empty.Data.Tools) != 0 || empty.Data.Digest == "" || empty.Data.Agent != room.Agent {
		t.Fatalf("no policy stored: %+v", empty.Data)
	}

	put(manager, base+"/tool-policy", 0, [2]string{"*", "ask"}, [2]string{"shell", "block"})
	withCeiling := view()
	if withCeiling.Data.Digest == empty.Data.Digest {
		t.Fatal("storing a ceiling must change the policy digest")
	}
	if got := withCeiling.Data.Tools["shell"]; got.Level != spaces.ToolLevelBlock || got.Source != spaces.ToolPolicyLayerSpaceCeiling {
		t.Fatalf("shell = %+v", got)
	}

	put(manager, base+"/agents/"+room.Agent+"/tool-policy", 0, [2]string{"send", "block"})
	withAgent := view()
	if got := withAgent.Data.Tools["send"]; got.Level != spaces.ToolLevelBlock || got.Source != spaces.ToolPolicyLayerAgent {
		t.Fatalf("send = %+v", got)
	}
	if withAgent.Data.Digest == withCeiling.Data.Digest {
		t.Fatal("an agent layer must change the policy digest")
	}

	// Changing one tool level moves the digest again; changing it back restores it.
	put(manager, base+"/agents/"+room.Agent+"/tool-policy", 1, [2]string{"send", "ask"})
	softer := view()
	if softer.Data.Digest == withAgent.Data.Digest {
		t.Fatal("a tool level change must change the policy digest")
	}
	if got := softer.Data.Tools["send"]; got.Level != spaces.ToolLevelAsk {
		t.Fatalf("send after change = %+v", got)
	}
}
