package handler

// WS14-S3 / ZDR: a room-file write that zero data retention refuses answers the
// Model Plane catalogue's typed envelope, {error:{code,message,retryable,action,
// details}}, instead of a bare string. These tests stop before the database: the
// handler has no repository here, so a write that got past the refusal would panic
// the test. The catalogue entry is read from the contract, so the message and the
// fixed retryable and action values are the contract's own.

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/go-chi/chi/v5"
)

// zdrContractEntry is one row of the Model Plane ZDR refusal catalogue
// (apps/Model Plane/api/contracts/zdr-refusals.json).
type zdrContractEntry struct {
	Action    string `json:"action"`
	Code      string `json:"code"`
	MessageEN string `json:"message_en"`
	Retryable bool   `json:"retryable"`
}

// durableMutationContractEntry returns the catalogue entry for
// zdr_durable_mutation_forbidden. The path reaches the Model Plane contract
// read-only, from this package.
func durableMutationContractEntry(t *testing.T) zdrContractEntry {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "Model Plane", "api", "contracts", "zdr-refusals.json"))
	if err != nil {
		t.Fatalf("read the Model Plane ZDR refusal catalogue: %v", err)
	}
	var catalogue struct {
		Codes []zdrContractEntry `json:"codes"`
	}
	if err := json.Unmarshal(raw, &catalogue); err != nil {
		t.Fatalf("decode the Model Plane ZDR refusal catalogue: %v", err)
	}
	for _, entry := range catalogue.Codes {
		if entry.Code == zdrDurableMutationCode {
			return entry
		}
	}
	t.Fatalf("the Model Plane catalogue has no %s entry", zdrDurableMutationCode)
	return zdrContractEntry{}
}

// assertZDRDurableMutationRefusal checks the 412 body against the catalogue entry
// and the fail-closed rule that applied (details.reason).
func assertZDRDurableMutationRefusal(t *testing.T, rec *httptest.ResponseRecorder, reason string) {
	t.Helper()
	entry := durableMutationContractEntry(t)
	if rec.Code != http.StatusPreconditionFailed {
		t.Fatalf("status = %d, want 412; body %s", rec.Code, rec.Body.String())
	}
	body := decodeError(t, rec)
	if _, flat := body["code"]; flat {
		t.Fatalf("refusal uses the flat coded shape, want the nested envelope: %s", rec.Body.String())
	}
	inner, ok := body["error"].(map[string]any)
	if !ok {
		t.Fatalf("error is not an object: %s", rec.Body.String())
	}
	if inner["code"] != entry.Code || inner["message"] != entry.MessageEN {
		t.Fatalf("code/message = %v / %v, want %s / %q", inner["code"], inner["message"], entry.Code, entry.MessageEN)
	}
	if inner["retryable"] != entry.Retryable || inner["action"] != entry.Action {
		t.Fatalf("retryable/action = %v / %v, want %v / %s", inner["retryable"], inner["action"], entry.Retryable, entry.Action)
	}
	details, _ := inner["details"].(map[string]any)
	if details["reason"] != reason {
		t.Fatalf("details.reason = %v, want %q", details["reason"], reason)
	}
	if rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("accepted header set on a ZDR refusal")
	}
}

func TestZDRTokenRefusalForARoomFileIsTheTypedEnvelope(t *testing.T) {
	stub := &stubGrants{}
	f := newUploadFixture(t, roomFilesOn(stub))
	rec := f.post(uploadRequest{
		decision: uploadDecision("user-1"),
		bearer:   f.auth.serviceTokenZDR(testOrg, "imports-core", true, "documents:write"),
	})
	assertZDRDurableMutationRefusal(t, rec, "verified token zdr=true forbids durable document persistence")
	if len(stub.attaches) != 0 {
		t.Fatal("Control was asked for a grant under a ZDR refusal")
	}
}

func TestZDRDecisionRefusalForARoomFileIsTheTypedEnvelope(t *testing.T) {
	stub := &stubGrants{}
	f := newUploadFixture(t, roomFilesOn(stub))
	zdr := uploadDecision("user-1")
	zdr.ZeroDataRetention = true
	rec := f.post(uploadRequest{decision: zdr})
	assertZDRDurableMutationRefusal(t, rec, errSpaceImportDecisionZDR.Error())
	if len(stub.attaches) != 0 {
		t.Fatal("Control was asked for a grant under a ZDR decision")
	}
}

func TestZDRRefusalWritesExactlyTheCatalogueFields(t *testing.T) {
	rec := httptest.NewRecorder()
	writeZDRDurableMutationRefusal(rec, "ingest_policy.zdr_mode=on or ephemeral_only=true forbids durable document persistence")
	if rec.Code != http.StatusPreconditionFailed {
		t.Fatalf("status = %d, want 412", rec.Code)
	}
	if got := rec.Header().Get("Content-Type"); got != "application/json" {
		t.Fatalf("content type = %q", got)
	}
	inner, ok := decodeError(t, rec)["error"].(map[string]any)
	if !ok {
		t.Fatalf("error is not an object: %s", rec.Body.String())
	}
	want := map[string]bool{"code": true, "message": true, "retryable": true, "action": true, "details": true}
	if len(inner) != len(want) {
		t.Fatalf("error keys = %v, want exactly %v", inner, want)
	}
	for key := range inner {
		if !want[key] {
			t.Fatalf("unexpected error key %q in %s", key, rec.Body.String())
		}
	}
}

// A verified zdr=true user cannot delete a document or a room file either: the
// catalogue's durable-mutation refusal covers delete. The handler has no repository
// here, so a delete that got past the refusal would panic the test.
func TestZDRDeleteIsTheTypedEnvelope(t *testing.T) {
	auth := newTestAuth(t)
	stub := &stubGrants{}
	h := NewDocumentHandler(nil, nil).WithSpaceFiles(roomFilesOn(stub))
	router := chi.NewRouter()
	router.Delete("/v1/documents/{documentID}", h.Delete)
	req := httptest.NewRequest(http.MethodDelete, "/v1/documents/doc-1", nil)
	req.Header.Set("Authorization", "Bearer "+auth.userToken(testOrg, "user-1", true))
	req.Header.Set("X-Org-ID", testOrg)
	rec := httptest.NewRecorder()
	auth.wrap(router).ServeHTTP(rec, req)
	assertZDRDurableMutationRefusal(t, rec, zdrDeleteReason)
	if len(stub.revokes) != 0 {
		t.Fatal("Control was asked to revoke under a ZDR refusal")
	}
}
