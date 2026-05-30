package resources

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

func newRestoreServer(t *testing.T) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountRestore(r, db)
	return r, db
}

func seedSnapshot(t *testing.T, db store.DB) store.Snapshot {
	t.Helper()
	snap := store.Snapshot{
		ID:        quarrycontracts.NewID(quarrycontracts.KindSnapshot),
		RunID:     quarrycontracts.NewID(quarrycontracts.KindRun),
		Bucket:    "s3://bucket/key",
		CreatedAt: time.Now().UnixMilli(),
	}
	if err := db.Snapshots().Create(snap); err != nil {
		t.Fatalf("seed snapshot: %v", err)
	}
	return snap
}

func postJSON(t *testing.T, h http.Handler, body string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, "/v1/restore", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	return w
}

func decodeEnv(t *testing.T, w *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var env map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &env); err != nil {
		t.Fatalf("decode: %v body=%s", err, w.Body.String())
	}
	return env
}

func TestRestore_HappyPath(t *testing.T) {
	t.Parallel()
	h, db := newRestoreServer(t)
	snap := seedSnapshot(t, db)

	body := `{"snapshot_id":"` + string(snap.ID) + `","target":"profile","name":"my-profile"}`
	w := postJSON(t, h, body)
	if w.Code != http.StatusCreated {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnv(t, w)
	data, _ := env["data"].(map[string]any)
	if got, _ := data["snapshot_uri"].(string); got != "s3://bucket/key" {
		t.Errorf("snapshot_uri = %q; want s3://bucket/key", got)
	}
	if got, _ := data["name"].(string); got != "my-profile" {
		t.Errorf("name = %q; want my-profile", got)
	}
	id, _ := data["id"].(string)
	if !strings.HasPrefix(id, string(quarrycontracts.KindProfile)) {
		t.Errorf("id = %q; want prefix %q", id, quarrycontracts.KindProfile)
	}
}

func TestRestore_DefaultName(t *testing.T) {
	t.Parallel()
	h, db := newRestoreServer(t)
	snap := seedSnapshot(t, db)

	body := `{"snapshot_id":"` + string(snap.ID) + `","target":"profile"}`
	w := postJSON(t, h, body)
	if w.Code != http.StatusCreated {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnv(t, w)
	data, _ := env["data"].(map[string]any)
	want := "restored-" + string(snap.ID)
	if got, _ := data["name"].(string); got != want {
		t.Errorf("name = %q; want %q", got, want)
	}
}

func TestRestore_SnapshotNotFound(t *testing.T) {
	t.Parallel()
	h, _ := newRestoreServer(t)
	missing := quarrycontracts.NewID(quarrycontracts.KindSnapshot)

	body := `{"snapshot_id":"` + string(missing) + `","target":"profile"}`
	w := postJSON(t, h, body)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnv(t, w)
	errObj, _ := env["error"].(map[string]any)
	if code, _ := errObj["code"].(string); code != string(quarrycontracts.CodeNotFound) {
		t.Errorf("error.code = %q; want %q", code, quarrycontracts.CodeNotFound)
	}
}

func TestRestore_WrongKind(t *testing.T) {
	t.Parallel()
	h, _ := newRestoreServer(t)
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)

	body := `{"snapshot_id":"` + string(runID) + `","target":"profile"}`
	w := postJSON(t, h, body)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnv(t, w)
	errObj, _ := env["error"].(map[string]any)
	if code, _ := errObj["code"].(string); code != string(quarrycontracts.CodeBadRequest) {
		t.Errorf("error.code = %q; want %q", code, quarrycontracts.CodeBadRequest)
	}
}

func TestRestore_InvalidTarget(t *testing.T) {
	t.Parallel()
	h, db := newRestoreServer(t)
	snap := seedSnapshot(t, db)

	body := `{"snapshot_id":"` + string(snap.ID) + `","target":"artifact"}`
	w := postJSON(t, h, body)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
	env := decodeEnv(t, w)
	errObj, _ := env["error"].(map[string]any)
	details, _ := errObj["details"].(map[string]any)
	if got, _ := details["target"].(string); got != "artifact" {
		t.Errorf("error.details.target = %q; want artifact", got)
	}
}

func TestRestore_BadJSON(t *testing.T) {
	t.Parallel()
	h, _ := newRestoreServer(t)
	w := postJSON(t, h, "{not json")
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d; body=%s", w.Code, w.Body.String())
	}
}
