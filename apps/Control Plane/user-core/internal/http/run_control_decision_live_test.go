package http

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS09-S7 live legs: the real handler against a real repository. Gated on
// TEST_DATABASE_URL via d12Pool.

func controlBody(room w2Room, operation string) string {
	b, _ := json.Marshal(map[string]string{
		"space_ref": room.Room, "run_id": "run-1", "operation": operation, "idempotency_key": "control-1",
	})
	return string(b)
}

func TestRunControlDecisionMatrixLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	public := d12SigningEnv(t)
	room := newW2Room("ws09s7")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.RunControlEntitled = true })
	s := &Server{}
	s.SetSpaceRepository(repo)
	control := func(user, body string) (int, string) {
		w := w2Post(s, (*Server).issueRunControlDecision, runControlPath, room.Org, user, body)
		return w.Code, w.Body.String()
	}

	for _, user := range []string{room.Manager, room.Owner} {
		code, body := control(user, controlBody(room, "cancel"))
		if code != http.StatusOK {
			t.Fatalf("%s: want 200, got %d (%s)", user, code, body)
		}
		var response d12DecisionBody
		if err := json.Unmarshal([]byte(body), &response); err != nil {
			t.Fatalf("decode: %v", err)
		}
		decision := d12VerifyToken(t, response.Data.Token, public)
		if decision.ActionID != "model.run.control" || decision.ActionSchemaHash != "sha256:run-control-v1" ||
			len(decision.Permissions) != 1 || decision.Permissions[0] != "run:cancel" {
			t.Fatalf("%s: signed decision = %+v", user, decision)
		}
	}

	// Editor, viewer, a member outside the current audience: fixed refusal.
	for name, user := range map[string]string{"editor": room.Editor, "viewer": room.Viewer, "outside the audience": room.Outside} {
		code, body := control(user, controlBody(room, "cancel"))
		if code != http.StatusForbidden || !strings.Contains(body, "Space run control is not authorized") {
			t.Fatalf("%s: want fixed 403, got %d (%s)", name, code, body)
		}
		if strings.Contains(body, `"token"`) {
			t.Fatalf("%s: a refusal must carry no token", name)
		}
	}
	if code, _ := control(room.Manager, controlBody(room, "pause")); code != http.StatusBadRequest {
		t.Fatalf("unknown operation: want 400, got %d", code)
	}
}

func TestRunControlDecisionRefusesWhenTheEntitlementIsOffLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room("ws09s7b")
	// thread_create and thread_read on: run control is a separate power.
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.ThreadCreateEntitled, p.ThreadReadEntitled = true, true })
	s := &Server{}
	s.SetSpaceRepository(repo)
	w := w2Post(s, (*Server).issueRunControlDecision, runControlPath, room.Org, room.Manager, controlBody(room, "cancel"))
	if w.Code != http.StatusForbidden || !strings.Contains(w.Body.String(), string(spaces.DenialEntitlementOff)) {
		t.Fatalf("entitlement off: %d %s", w.Code, w.Body.String())
	}
}

func TestRunControlDecisionServesAnArchivedRoomLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room("ws09s7c")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.RunControlEntitled = true })
	if _, err := pool.Exec(context.Background(), `UPDATE registered_spaces SET registration_state='archived' WHERE space_ref=$1`, room.Room); err != nil {
		t.Fatalf("archive: %v", err)
	}
	s := &Server{}
	s.SetSpaceRepository(repo)
	w := w2Post(s, (*Server).issueRunControlDecision, runControlPath, room.Org, room.Manager, controlBody(room, "cancel"))
	if w.Code != http.StatusOK {
		t.Fatalf("a run started before the archive must stay stoppable: %d %s", w.Code, w.Body.String())
	}
}
