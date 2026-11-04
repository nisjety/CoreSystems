package http

import (
	"encoding/json"
	"net/http"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS08-S1 live legs: the real handler against a real repository. Gated on
// TEST_DATABASE_URL via d12Pool.

func wakeBody(room w2Room, agent, origin string) string {
	b, _ := json.Marshal(map[string]string{
		"space_ref": room.Room, "thread_id": "thread-1", "trigger_message_id": "message-9",
		"content_digest": "sha256:" + strings.Repeat("a", 64), "agent_subject_id": agent,
		"turn_origin": origin, "idempotency_key": "wake-1",
	})
	return string(b)
}

func TestRunWakeDecisionMemberMatrixLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	public := d12SigningEnv(t)
	room := newW2Room("ws08s1")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.ThreadCreateEntitled = true })
	s := &Server{}
	s.SetSpaceRepository(repo)
	wake := func(user, body string) (int, string) {
		w := w2Post(s, (*Server).issueRunWakeDecision, runWakePath, room.Org, user, body)
		return w.Code, w.Body.String()
	}

	for _, user := range []string{room.Editor, room.Manager, room.Owner} {
		code, body := wake(user, wakeBody(room, room.Agent, "direct"))
		if code != http.StatusOK {
			t.Fatalf("%s: want 200, got %d (%s)", user, code, body)
		}
		var response d12DecisionBody
		if err := json.Unmarshal([]byte(body), &response); err != nil {
			t.Fatalf("decode: %v", err)
		}
		decision := d12VerifyToken(t, response.Data.Token, public)
		if decision.ActionID != "model.run.wake" || len(decision.Permissions) != 1 || decision.Permissions[0] != "run:wake" {
			t.Fatalf("%s: signed decision = %+v", user, decision)
		}
		if decision.SubjectID != user {
			t.Fatalf("%s: decision subject = %q", user, decision.SubjectID)
		}
	}

	for name, tc := range map[string]struct{ user, body string }{
		"viewer":               {room.Viewer, wakeBody(room, room.Agent, "direct")},
		"not in the audience":  {room.Outside, wakeBody(room, room.Agent, "direct")},
		"agent not on roster":  {room.Editor, wakeBody(room, "ws08s1-ghost", "direct")},
		"a human is not agent": {room.Editor, wakeBody(room, room.Viewer, "direct")},
	} {
		code, body := wake(tc.user, tc.body)
		if code != http.StatusForbidden {
			t.Fatalf("%s: want 403, got %d (%s)", name, code, body)
		}
		if !strings.Contains(body, "Space run wake is not authorized") && !strings.Contains(body, "not authorized") {
			t.Fatalf("%s: unexpected refusal body %s", name, body)
		}
		if strings.Contains(body, `"token"`) {
			t.Fatalf("%s: a refusal must carry no token", name)
		}
	}
	// The agent-not-on-roster refusal is the exact one-key body.
	const denied = `{"error":"Space run wake is not authorized"}`
	if _, body := wake(room.Editor, wakeBody(room, "ws08s1-ghost", "direct")); strings.TrimSpace(body) != denied {
		t.Fatalf("untyped refusal body = %s, want %s", body, denied)
	}
}

func TestRunWakeDecisionRefusesWithoutThreadCreateEntitlementLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room("ws08s1b")
	seedW2Room(t, pool, repo, room, nil) // every entitlement off
	s := &Server{}
	s.SetSpaceRepository(repo)
	w := w2Post(s, (*Server).issueRunWakeDecision, runWakePath, room.Org, room.Editor, wakeBody(room, room.Agent, "direct"))
	if w.Code != http.StatusForbidden || !strings.Contains(w.Body.String(), string(spaces.DenialEntitlementOff)) {
		t.Fatalf("entitlement off: %d %s", w.Code, w.Body.String())
	}
}
