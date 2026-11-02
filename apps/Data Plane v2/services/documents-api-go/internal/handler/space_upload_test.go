package handler

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/spacegrant"
)

// WS14-S3: room files in documents-api. These tests stop before the database
// (the handler is built with a nil repository, so reaching it would panic and
// fail the test): everything that is decided from the verified decision, the
// flag, the metadata and Control's answers. The paths that need rows (dedupe,
// quota, idempotent retry, header-after-grant, delete) are in
// space_files_handler_live_test.go behind the `integration` tag.

const (
	testOrg   = "org-1"
	testSpace = "space:org-1:room-7"
	goodSHA   = "a3f1c2d4e5b60718293a4b5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f90"
)

type revokeCall struct{ org, user, authorization, doc, space string }

type stubGrants struct {
	attachErr  error
	revokeErr  error
	attaches   [][2]string
	revokes    []revokeCall
	attachHook func()
}

func (s *stubGrants) AttachFile(_ context.Context, decisionToken, documentID string) error {
	s.attaches = append(s.attaches, [2]string{decisionToken, documentID})
	if s.attachHook != nil {
		s.attachHook()
	}
	return s.attachErr
}

func (s *stubGrants) RevokeFile(_ context.Context, orgID, userID, authorization, documentID, spaceRef string) error {
	s.revokes = append(s.revokes, revokeCall{orgID, userID, authorization, documentID, spaceRef})
	return s.revokeErr
}

func roomFilesOn(g SpaceGrantClient) SpaceFilesConfig {
	return SpaceFilesConfig{Enabled: true, MaxPerRoom: 5, MaxTextBytesPerRoom: 1000, Grants: g}
}

type uploadFixture struct {
	t       *testing.T
	auth    *testAuth
	signer  *decisionSigner
	handler http.Handler
}

func newUploadFixture(t *testing.T, cfg SpaceFilesConfig) *uploadFixture {
	t.Helper()
	auth := newTestAuth(t)
	signer := newDecisionSigner(t)
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(cfg)
	return &uploadFixture{t: t, auth: auth, signer: signer, handler: auth.wrap(http.HandlerFunc(h.Create))}
}

type uploadRequest struct {
	decision    *spaceImportDecisionClaims // nil: send no decision header
	bearer      string                     // default: imports-core service token
	body        map[string]any
	omitSHA     bool
	extraHeader map[string]string
}

func (f *uploadFixture) post(r uploadRequest) *httptest.ResponseRecorder {
	f.t.Helper()
	body := map[string]any{
		"source": "space-upload", "type": "space_upload", "title": "plan.docx", "content": "hello room",
		"zdr_classification": "internal", "idempotency_key": "upload-1",
		"metadata": map[string]any{"filename": "plan.docx", "sha256": goodSHA},
	}
	for k, v := range r.body {
		body[k] = v
	}
	raw, err := json.Marshal(body)
	if err != nil {
		f.t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/v1/documents", bytes.NewReader(raw))
	bearer := r.bearer
	if bearer == "" {
		bearer = f.auth.serviceToken(testOrg, "imports-core", "documents:write")
	}
	req.Header.Set("Authorization", "Bearer "+bearer)
	req.Header.Set("X-Org-ID", testOrg)
	if r.decision != nil {
		req.Header.Set(spaceImportDecisionHeader, f.signer.sign(*r.decision))
	}
	for k, v := range r.extraHeader {
		req.Header.Set(k, v)
	}
	rec := httptest.NewRecorder()
	f.handler.ServeHTTP(rec, req)
	return rec
}

func uploadDecision(subject string) *spaceImportDecisionClaims {
	d := baseDecision(testOrg, testSpace, subject, "space_upload")
	return &d
}

func decodeError(t *testing.T, rec *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var out map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatalf("response is not JSON (%d): %q", rec.Code, rec.Body.String())
	}
	return out
}

func TestRoomFileFlagOffRefusesWith404AndNoAcceptedHeader(t *testing.T) {
	stub := &stubGrants{}
	f := newUploadFixture(t, SpaceFilesConfig{Grants: stub}) // Enabled false: the default
	rec := f.post(uploadRequest{decision: uploadDecision("user-1")})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404; body %s", rec.Code, rec.Body.String())
	}
	if got := decodeError(t, rec)["code"]; got != "room_files_disabled" {
		t.Fatalf("code = %v", got)
	}
	if rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("a refused room file must not carry the accepted header")
	}
	if len(stub.attaches) != 0 {
		t.Fatal("Control was asked for a grant while the flag is off")
	}
}

func TestSpaceUploadTypeWithoutADecisionIsRefused(t *testing.T) {
	f := newUploadFixture(t, roomFilesOn(&stubGrants{}))
	rec := f.post(uploadRequest{decision: nil})
	if rec.Code != http.StatusForbidden || decodeError(t, rec)["code"] != "space_files_not_authorized" {
		t.Fatalf("no decision: %d %s", rec.Code, rec.Body.String())
	}
	// A decision for another source type cannot mint a room file by naming the type.
	other := baseDecision(testOrg, testSpace, "user-1", "notion")
	rec = f.post(uploadRequest{decision: &other})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("notion decision, space_upload body: %d %s", rec.Code, rec.Body.String())
	}
	if rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header set on a refusal")
	}
}

func TestRoomFileOwnerIsTheSignedSubjectNeverTheBodyOwner(t *testing.T) {
	stub := &stubGrants{}
	f := newUploadFixture(t, roomFilesOn(stub))
	// A body owner_id naming another user is refused (the only accepted
	// owner_id is the decision's own subject).
	rec := f.post(uploadRequest{decision: uploadDecision("user-1"), body: map[string]any{"owner_id": "user-2"}})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403; body %s", rec.Code, rec.Body.String())
	}
	if !strings.Contains(rec.Body.String(), "owner_id must match the Space import decision subject") {
		t.Fatalf("body = %s", rec.Body.String())
	}
	if len(stub.attaches) != 0 || rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("a refused owner must not reach the grant or the accepted header")
	}
}

func TestRoomFileRefusesVerifiedZDRAndNonImportsCorePrincipal(t *testing.T) {
	f := newUploadFixture(t, roomFilesOn(&stubGrants{}))
	// The decision header is only honoured for the scoped imports-core principal.
	rec := f.post(uploadRequest{decision: uploadDecision("user-1"), bearer: f.auth.userToken(testOrg, "user-1", false)})
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "invalid Space import authority") {
		t.Fatalf("user bearer + decision: %d %s", rec.Code, rec.Body.String())
	}
	// A decision that itself says zero retention is not authority to persist.
	zdr := uploadDecision("user-1")
	zdr.ZeroDataRetention = true
	rec = f.post(uploadRequest{decision: zdr})
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "invalid Space import authority") {
		t.Fatalf("zdr decision: %d %s", rec.Code, rec.Body.String())
	}
	// A token whose signed posture is zdr=true cannot persist a durable file.
	rec = f.post(uploadRequest{decision: uploadDecision("user-1"), bearer: f.auth.serviceTokenZDR(testOrg, "imports-core", true, "documents:write")})
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "zdr=true forbids durable document persistence") {
		t.Fatalf("zdr token: %d %s", rec.Code, rec.Body.String())
	}
	if rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header set on a ZDR refusal")
	}
}

func TestPrepareSpaceFileStampsServerFieldsAndNeverTrustsTheCaller(t *testing.T) {
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(SpaceFilesConfig{Enabled: true, MaxPerRoom: 7, MaxTextBytesPerRoom: 4096})
	upper := strings.ToUpper(goodSHA)
	input := &model.CreateDocumentInput{
		Type: "space_upload", SpaceRef: testSpace, Content: "héllo wörld", // 11 runes, 13 bytes
		Metadata: json.RawMessage(`{"filename":"a.csv","sha256":"  ` + upper + ` ","text_chars":999999,"created_by_kind":"agent","scan_state":"clean"}`),
	}
	rec := httptest.NewRecorder()
	if !h.prepareSpaceFile(rec, input, uploadDecision("user-1")) {
		t.Fatalf("refused: %d %s", rec.Code, rec.Body.String())
	}
	var meta map[string]any
	if err := json.Unmarshal(input.Metadata, &meta); err != nil {
		t.Fatal(err)
	}
	if meta["sha256"] != goodSHA {
		t.Fatalf("sha256 = %v, want lowercased and trimmed", meta["sha256"])
	}
	if meta["text_chars"] != float64(11) {
		t.Fatalf("text_chars = %v, want the server's rune count (11)", meta["text_chars"])
	}
	if meta["created_by_kind"] != "human" || meta["scan_state"] != "not_scanned_text_only" {
		t.Fatalf("server-stamped fields were taken from the caller: %v", meta)
	}
	if meta["filename"] != "a.csv" {
		t.Fatal("caller metadata the server does not own must survive")
	}
	if input.SpaceFile == nil || input.SpaceFile.SHA256 != goodSHA || input.SpaceFile.MaxFiles != 7 || input.SpaceFile.MaxTextBytes != 4096 {
		t.Fatalf("constraints = %+v", input.SpaceFile)
	}
}

func TestPrepareSpaceFileMetadataMustCarryAValidSHA256(t *testing.T) {
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(SpaceFilesConfig{Enabled: true})
	cases := map[string]json.RawMessage{
		"no metadata":       nil,
		"null metadata":     json.RawMessage(`null`),
		"not an object":     json.RawMessage(`[1,2]`),
		"no sha256":         json.RawMessage(`{"filename":"a"}`),
		"empty sha256":      json.RawMessage(`{"sha256":""}`),
		"short sha256":      json.RawMessage(`{"sha256":"abc123"}`),
		"63 chars":          json.RawMessage(`{"sha256":"` + goodSHA[:63] + `"}`),
		"65 chars":          json.RawMessage(`{"sha256":"` + goodSHA + `0"}`),
		"non-hex":           json.RawMessage(`{"sha256":"` + strings.Repeat("z", 64) + `"}`),
		"number not string": json.RawMessage(`{"sha256":123}`),
	}
	for name, metadata := range cases {
		t.Run(name, func(t *testing.T) {
			rec := httptest.NewRecorder()
			input := &model.CreateDocumentInput{Type: "space_upload", SpaceRef: testSpace, Content: "x", Metadata: metadata}
			if h.prepareSpaceFile(rec, input, uploadDecision("user-1")) {
				t.Fatal("accepted")
			}
			if rec.Code != http.StatusBadRequest || decodeError(t, rec)["code"] != "room_file_metadata_invalid" {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
			if input.SpaceFile != nil {
				t.Fatal("constraints were attached to a refused input")
			}
		})
	}
}

func TestPrepareSpaceFileRefusesWrongTypeOrMissingSpaceAndFlagOff(t *testing.T) {
	on := NewDocumentHandler(nil, nil).WithSpaceFiles(SpaceFilesConfig{Enabled: true})
	for name, input := range map[string]*model.CreateDocumentInput{
		"wrong type":  {Type: "note", SpaceRef: testSpace},
		"no space":    {Type: "space_upload"},
		"blank space": {Type: "space_upload", SpaceRef: "  "},
		"blank type":  {Type: " ", SpaceRef: testSpace},
	} {
		rec := httptest.NewRecorder()
		if on.prepareSpaceFile(rec, input, uploadDecision("user-1")) || rec.Code != http.StatusForbidden || decodeError(t, rec)["code"] != "space_files_not_authorized" {
			t.Fatalf("%s: %d %s", name, rec.Code, rec.Body.String())
		}
	}
	off := NewDocumentHandler(nil, nil)
	rec := httptest.NewRecorder()
	if off.prepareSpaceFile(rec, &model.CreateDocumentInput{Type: "space_upload", SpaceRef: testSpace}, uploadDecision("user-1")) || rec.Code != http.StatusNotFound {
		t.Fatalf("flag off: %d", rec.Code)
	}
}

func TestWriteSpaceFileRepoErrorMapsDuplicateAndQuota(t *testing.T) {
	rec := httptest.NewRecorder()
	if !writeSpaceFileRepoError(rec, &repo.SpaceFileDuplicateError{DocumentID: "doc-9"}) {
		t.Fatal("duplicate not handled")
	}
	body := decodeError(t, rec)
	details, _ := body["details"].(map[string]any)
	if rec.Code != http.StatusConflict || body["code"] != "room_file_duplicate" || details["document_id"] != "doc-9" {
		t.Fatalf("duplicate: %d %v", rec.Code, body)
	}

	// The unique-index backstop reports a duplicate without an id.
	rec = httptest.NewRecorder()
	writeSpaceFileRepoError(rec, &repo.SpaceFileDuplicateError{})
	body = decodeError(t, rec)
	if rec.Code != http.StatusConflict || body["code"] != "room_file_duplicate" {
		t.Fatalf("backstop duplicate: %d %v", rec.Code, body)
	}
	if _, has := body["details"]; has {
		t.Fatalf("a duplicate without an id must not invent details: %v", body)
	}

	for _, limit := range []string{"max_files", "max_text_bytes"} {
		rec = httptest.NewRecorder()
		wrapped := errors.Join(errors.New("tx"), &repo.SpaceFileQuotaError{Limit: limit, MaxFiles: 5, MaxTextBytes: 1000})
		if !writeSpaceFileRepoError(rec, wrapped) {
			t.Fatalf("%s: quota not handled through a wrapped error", limit)
		}
		body = decodeError(t, rec)
		details, _ = body["details"].(map[string]any)
		if rec.Code != http.StatusConflict || body["code"] != "room_files_quota_exceeded" ||
			details["limit"] != limit || details["max_files"] != float64(5) || details["max_text_bytes"] != float64(1000) {
			t.Fatalf("%s: %d %v", limit, rec.Code, body)
		}
	}
	if writeSpaceFileRepoError(httptest.NewRecorder(), errors.New("connection reset")) {
		t.Fatal("an unrelated error was swallowed as a room-file error")
	}
}

func attachRequest(token string) *http.Request {
	req := httptest.NewRequest(http.MethodPost, "/v1/documents", nil)
	req.Header.Set(spaceImportDecisionHeader, token)
	return req
}

func TestAttachSpaceFileDeniedIs403WithReasonAndNeverTheAcceptedHeader(t *testing.T) {
	stub := &stubGrants{attachErr: &spacegrant.DeniedError{Code: "space_member_required"}}
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(stub))
	rec := httptest.NewRecorder()
	if h.attachSpaceFile(rec, attachRequest(" tok "), "doc-1") {
		t.Fatal("a denied grant reported success")
	}
	body := decodeError(t, rec)
	details, _ := body["details"].(map[string]any)
	if rec.Code != http.StatusForbidden || body["code"] != "space_files_not_authorized" || details["reason_code"] != "space_member_required" {
		t.Fatalf("%d %v", rec.Code, body)
	}
	if rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header set although Control denied the grant")
	}
	if len(stub.attaches) != 1 || stub.attaches[0] != [2]string{"tok", "doc-1"} {
		t.Fatalf("attach calls = %v, want the trimmed decision token and the document id", stub.attaches)
	}
}

func TestAttachSpaceFileUnavailableOrUnconfiguredIs503AndNeverTheAcceptedHeader(t *testing.T) {
	for name, cfg := range map[string]SpaceFilesConfig{
		"control unavailable": roomFilesOn(&stubGrants{attachErr: spacegrant.ErrUnavailable}),
		"transport failure":   roomFilesOn(&stubGrants{attachErr: errors.New("dial tcp: refused")}),
		"not configured":      roomFilesOn(nil),
	} {
		t.Run(name, func(t *testing.T) {
			h := NewDocumentHandler(nil, nil).WithSpaceFiles(cfg)
			rec := httptest.NewRecorder()
			if h.attachSpaceFile(rec, attachRequest("tok"), "doc-1") {
				t.Fatal("reported success")
			}
			if rec.Code != http.StatusServiceUnavailable || decodeError(t, rec)["code"] != "space_grants_unavailable" {
				t.Fatalf("%d %s", rec.Code, rec.Body.String())
			}
			if rec.Header().Get(spaceImportAcceptedHeader) != "" {
				t.Fatal("accepted header set although the grant is not recorded")
			}
		})
	}
	if cfg := (SpaceFilesConfig{}); cfg.attach(context.Background(), "t", "d") != spacegrant.ErrNotConfigured {
		t.Fatal("a nil Grants client must read as not configured")
	}
}

func TestAttachSpaceFileSuccessWritesNothingItself(t *testing.T) {
	// The accepted header is the caller's to set AFTER this returns true.
	stub := &stubGrants{}
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(stub))
	rec := httptest.NewRecorder()
	if !h.attachSpaceFile(rec, attachRequest("tok"), "doc-1") {
		t.Fatal("attach failed")
	}
	if rec.Body.Len() != 0 || rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("attachSpaceFile must not write a response or the accepted header on success")
	}
}

func TestBulkIngestRefusesSpaceUploadType(t *testing.T) {
	auth := newTestAuth(t)
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(&stubGrants{}))
	body, _ := json.Marshal(map[string]any{"documents": []map[string]any{
		{"source": "s", "type": "space_upload", "title": "t", "content": "c"},
		{"source": "s", "type": " space_upload ", "title": "t", "content": "c"},
	}})
	req := httptest.NewRequest(http.MethodPost, "/v1/documents/bulk", bytes.NewReader(body))
	req.Header.Set("Authorization", "Bearer "+auth.serviceToken(testOrg, "imports-core", "documents:write"))
	req.Header.Set("X-Org-ID", testOrg)
	rec := httptest.NewRecorder()
	auth.wrap(http.HandlerFunc(h.BulkIngest)).ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d %s", rec.Code, rec.Body.String())
	}
	var out struct {
		Accepted  int      `json:"accepted"`
		Rejected  int      `json:"rejected"`
		IDs       []string `json:"document_ids"`
		Rejection []string `json:"rejection_reasons"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &out); err != nil {
		t.Fatal(err)
	}
	if out.Accepted != 0 || out.Rejected != 2 || len(out.IDs) != 0 || len(out.Rejection) != 2 {
		t.Fatalf("bulk result = %+v", out)
	}
	for _, reason := range out.Rejection {
		if !strings.Contains(reason, "room files need a Space import decision") {
			t.Fatalf("reason = %q", reason)
		}
	}
}

func roomFileDoc(space string) *model.Document {
	return &model.Document{DocumentID: "doc-1", Type: "space_upload", SpaceRef: &space}
}

func TestDeleteOfARoomFileRevokesTheSpaceGrantWithTheUploaderBearer(t *testing.T) {
	auth := newTestAuth(t)
	stub := &stubGrants{}
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(stub))
	token := auth.userToken(testOrg, "user-1", false)
	call := func(token string, doc *model.Document) {
		req := httptest.NewRequest(http.MethodDelete, "/v1/documents/doc-1", nil)
		req.Header.Set("Authorization", "Bearer "+token)
		req.Header.Set("X-Org-ID", testOrg)
		auth.wrap(http.HandlerFunc(func(_ http.ResponseWriter, r *http.Request) {
			h.revokeSpaceFile(r, doc, testOrg)
		})).ServeHTTP(httptest.NewRecorder(), req)
	}

	call(token, roomFileDoc(testSpace))
	want := revokeCall{org: testOrg, user: "user-1", authorization: "Bearer " + token, doc: "doc-1", space: testSpace}
	if len(stub.revokes) != 1 || stub.revokes[0] != want {
		t.Fatalf("revoke calls = %+v, want exactly %+v", stub.revokes, want)
	}

	// A stub failure does not propagate: revokeSpaceFile has no error to fail
	// the delete with, and the grant is inert once the document is gone.
	stub.revokeErr = errors.New("control down")
	call(token, roomFileDoc(testSpace))
	if len(stub.revokes) != 2 {
		t.Fatalf("revoke calls after failure = %d", len(stub.revokes))
	}
}

func TestRevokeSpaceFileSkipsWhenThereIsNothingToRevoke(t *testing.T) {
	auth := newTestAuth(t)
	stub := &stubGrants{}
	on := NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(stub))
	off := NewDocumentHandler(nil, nil).WithSpaceFiles(SpaceFilesConfig{Grants: stub})
	blank := ""
	run := func(h *DocumentHandler, token string, doc *model.Document) {
		req := httptest.NewRequest(http.MethodDelete, "/v1/documents/doc-1", nil)
		req.Header.Set("Authorization", "Bearer "+token)
		req.Header.Set("X-Org-ID", testOrg)
		auth.wrap(http.HandlerFunc(func(_ http.ResponseWriter, r *http.Request) {
			h.revokeSpaceFile(r, doc, testOrg)
		})).ServeHTTP(httptest.NewRecorder(), req)
	}
	user := auth.userToken(testOrg, "user-1", false)
	run(on, user, nil)
	run(on, user, &model.Document{DocumentID: "d", Type: "note", SpaceRef: ptr(testSpace)})
	run(on, user, &model.Document{DocumentID: "d", Type: "space_upload"})
	run(on, user, &model.Document{DocumentID: "d", Type: "space_upload", SpaceRef: &blank})
	run(off, user, roomFileDoc(testSpace))
	// A service caller (org:data:write_all) has no user to delegate for.
	run(on, auth.serviceToken(testOrg, "ops", "org:data:write_all"), roomFileDoc(testSpace))
	if len(stub.revokes) != 0 {
		t.Fatalf("Control was asked to revoke %d times, want 0: %+v", len(stub.revokes), stub.revokes)
	}
	// Flag on but no client wired: no panic.
	run(NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(nil)), user, roomFileDoc(testSpace))
}

func ptr(s string) *string { return &s }
