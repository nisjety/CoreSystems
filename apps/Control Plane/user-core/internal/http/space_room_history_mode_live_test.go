package http

import (
	"encoding/json"
	"net/http"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS04-S11 live leg: PATCH settings accepts history_mode alone and echoes it.
// Gated on TEST_DATABASE_URL via d12Pool.
func TestSettingsEchoesHistoryModeLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	room := newW2Room("ws04s11")
	seedW2Room(t, pool, repo, room, nil)

	ctx := roomActorCtx()
	ctx["org_id"] = room.Org
	ctx["user_id"] = room.Owner
	engine := roomDirectoryEngine(t, repo, ctx)
	target := "/api/v1/internal/spaces/" + room.Room + "/settings"

	rec := roomDirectoryDo(engine, http.MethodPatch, target, `{"history_mode":"since_join"}`)
	if rec.Code != http.StatusOK {
		t.Fatalf("owner PATCH: %d %s", rec.Code, rec.Body.String())
	}
	var body struct {
		Data struct {
			HistoryMode string `json:"history_mode"`
			Changed     bool   `json:"changed"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if body.Data.HistoryMode != "since_join" || !body.Data.Changed {
		t.Fatalf("echo = %+v", body.Data)
	}

	// A manager is refused with the typed role code.
	ctx["user_id"] = room.Manager
	engine = roomDirectoryEngine(t, repo, ctx)
	rec = roomDirectoryDo(engine, http.MethodPatch, target, `{"history_mode":"full"}`)
	if rec.Code != http.StatusForbidden || errorCode(t, rec) != spaces.CodeRoomRoleForbidden {
		t.Fatalf("manager PATCH: %d %s", rec.Code, rec.Body.String())
	}
}
