//go:build integration

package handler

// WS14-S3 through the real handler and a real database: the paths the unit
// tests in space_upload_test.go cannot reach because they need rows.

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/go-chi/chi/v5"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/spacegrant"
)

func newLiveUpload(t *testing.T, cfg SpaceFilesConfig) (*uploadFixture, *pgxpool.Pool) {
	t.Helper()
	r, pool := liveDB(t)
	auth := newTestAuth(t)
	signer := newDecisionSigner(t)
	h := NewDocumentHandler(r, nil).WithSpaceFiles(cfg)
	mux := chi.NewRouter()
	mux.Post("/v1/documents", h.Create)
	mux.Delete("/v1/documents/{documentID}", h.Delete)
	return &uploadFixture{t: t, auth: auth, signer: signer, handler: auth.wrap(mux)}, pool
}

func withSHA(sha, key string) map[string]any {
	return map[string]any{
		"idempotency_key": key,
		"metadata":        map[string]any{"filename": key + ".docx", "sha256": sha},
	}
}

func documentIDOf(t *testing.T, body []byte) string {
	t.Helper()
	var doc struct {
		DocumentID string `json:"document_id"`
	}
	if err := json.Unmarshal(body, &doc); err != nil || doc.DocumentID == "" {
		t.Fatalf("no document_id in %s (%v)", body, err)
	}
	return doc.DocumentID
}

func TestRoomFileCreateLiveOwnerVisibilityStampsAndHeaderAfterGrant(t *testing.T) {
	stub := &stubGrants{}
	f, pool := newLiveUpload(t, roomFilesOn(stub))

	rec := f.post(uploadRequest{decision: uploadDecision("user-1"), body: withSHA(goodSHA, "up-1")})
	if rec.Code != http.StatusCreated {
		t.Fatalf("status = %d %s", rec.Code, rec.Body.String())
	}
	if rec.Header().Get(spaceImportAcceptedHeader) != "true" {
		t.Fatal("accepted header missing after a recorded grant")
	}
	id := documentIDOf(t, rec.Body.Bytes())
	if len(stub.attaches) != 1 || stub.attaches[0][1] != id {
		t.Fatalf("attach calls = %v, want one for %s", stub.attaches, id)
	}

	var owner, visibility, space, kind, scan string
	var chars int
	err := pool.QueryRow(context.Background(), `
		SELECT owner_id, visibility, space_ref, metadata->>'created_by_kind', metadata->>'scan_state', (metadata->>'text_chars')::int
		FROM documents WHERE document_id = $1`, id).Scan(&owner, &visibility, &space, &kind, &scan, &chars)
	if err != nil {
		t.Fatal(err)
	}
	if owner != "user-1" || visibility != "shared" || space != testSpace || kind != "human" || scan != "not_scanned_text_only" || chars != len("hello room") {
		t.Fatalf("row = owner %q visibility %q space %q kind %q scan %q chars %d", owner, visibility, space, kind, scan, chars)
	}
}

func TestRoomFileCreateLiveDuplicateAndQuotaAre409WithoutTheAcceptedHeader(t *testing.T) {
	stub := &stubGrants{}
	cfg := roomFilesOn(stub)
	cfg.MaxPerRoom = 2
	f, _ := newLiveUpload(t, cfg)

	first := f.post(uploadRequest{decision: uploadDecision("user-1"), body: withSHA(sha64('1'), "k1")})
	if first.Code != http.StatusCreated {
		t.Fatalf("first: %d %s", first.Code, first.Body.String())
	}
	firstID := documentIDOf(t, first.Body.Bytes())

	dup := f.post(uploadRequest{decision: uploadDecision("user-2"), body: withSHA(sha64('1'), "k-other")})
	body := decodeError(t, dup)
	details, _ := body["details"].(map[string]any)
	if dup.Code != http.StatusConflict || body["code"] != "room_file_duplicate" || details["document_id"] != firstID {
		t.Fatalf("duplicate: %d %v", dup.Code, body)
	}
	if dup.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header on a duplicate")
	}

	if second := f.post(uploadRequest{decision: uploadDecision("user-1"), body: withSHA(sha64('2'), "k2")}); second.Code != http.StatusCreated {
		t.Fatalf("second: %d %s", second.Code, second.Body.String())
	}
	over := f.post(uploadRequest{decision: uploadDecision("user-1"), body: withSHA(sha64('3'), "k3")})
	body = decodeError(t, over)
	details, _ = body["details"].(map[string]any)
	if over.Code != http.StatusConflict || body["code"] != "room_files_quota_exceeded" || details["limit"] != "max_files" || details["max_files"] != float64(2) {
		t.Fatalf("quota: %d %v", over.Code, body)
	}
	if over.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header on a quota refusal")
	}
	if len(stub.attaches) != 2 {
		t.Fatalf("grant attached %d times, want only for the 2 accepted files", len(stub.attaches))
	}
}

func TestRoomFileCreateLiveGrantFailureThenRetryReusesTheRowAndReattempts(t *testing.T) {
	stub := &stubGrants{attachErr: &spacegrant.DeniedError{Code: "space_member_required"}}
	f, pool := newLiveUpload(t, roomFilesOn(stub))
	req := uploadRequest{decision: uploadDecision("user-1"), body: withSHA(goodSHA, "retry-1")}

	denied := f.post(req)
	if denied.Code != http.StatusForbidden || decodeError(t, denied)["code"] != "space_files_not_authorized" {
		t.Fatalf("denied: %d %s", denied.Code, denied.Body.String())
	}
	if denied.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header although the grant was denied")
	}

	stub.attachErr = errors.New("control down")
	unavailable := f.post(req)
	if unavailable.Code != http.StatusServiceUnavailable || unavailable.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatalf("unavailable: %d header %q", unavailable.Code, unavailable.Header().Get(spaceImportAcceptedHeader))
	}

	stub.attachErr = nil
	ok := f.post(req)
	if ok.Code != http.StatusOK || ok.Header().Get(spaceImportAcceptedHeader) != "true" {
		t.Fatalf("retry: %d header %q %s", ok.Code, ok.Header().Get(spaceImportAcceptedHeader), ok.Body.String())
	}
	if len(stub.attaches) != 3 {
		t.Fatalf("attach attempts = %d, want 3 (one per try)", len(stub.attaches))
	}
	var rows int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM documents WHERE org_id = $1 AND type = 'space_upload'`, testOrg).Scan(&rows); err != nil || rows != 1 {
		t.Fatalf("rows = %d err=%v, want the same row reused across retries", rows, err)
	}
}

func TestRoomFileDeleteLiveRevokesWithUploaderBearerAndSurvivesRevokeFailure(t *testing.T) {
	stub := &stubGrants{}
	f, pool := newLiveUpload(t, roomFilesOn(stub))
	uploader := f.auth.userToken(testOrg, "user-1", false)

	del := func(id string) *httptest.ResponseRecorder {
		req := httptest.NewRequest(http.MethodDelete, "/v1/documents/"+id, nil)
		req.Header.Set("Authorization", "Bearer "+uploader)
		req.Header.Set("X-Org-ID", testOrg)
		rec := httptest.NewRecorder()
		f.handler.ServeHTTP(rec, req)
		return rec
	}

	first := f.post(uploadRequest{decision: uploadDecision("user-1"), body: withSHA(sha64('a'), "d1")})
	firstID := documentIDOf(t, first.Body.Bytes())
	if rec := del(firstID); rec.Code != http.StatusOK {
		t.Fatalf("delete: %d %s", rec.Code, rec.Body.String())
	}
	want := revokeCall{org: testOrg, user: "user-1", authorization: "Bearer " + uploader, doc: firstID, space: testSpace}
	if len(stub.revokes) != 1 || stub.revokes[0] != want {
		t.Fatalf("revokes = %+v, want %+v", stub.revokes, want)
	}

	stub.revokeErr = errors.New("control down")
	second := f.post(uploadRequest{decision: uploadDecision("user-1"), body: withSHA(sha64('b'), "d2")})
	secondID := documentIDOf(t, second.Body.Bytes())
	if rec := del(secondID); rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), "true") {
		t.Fatalf("delete with a failing revoke: %d %s", rec.Code, rec.Body.String())
	}
	var deleted int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM documents WHERE document_id = $1 AND deleted_at IS NOT NULL`, secondID).Scan(&deleted); err != nil || deleted != 1 {
		t.Fatalf("deleted rows = %d err=%v", deleted, err)
	}
}

func sha64(c byte) string { return strings.Repeat(string(c), 64) }
