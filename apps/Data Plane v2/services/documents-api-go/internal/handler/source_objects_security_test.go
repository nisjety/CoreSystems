package handler

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

func TestSourceObjectUpsertRejectsSignedZDRBeforePersistence(t *testing.T) {
	body := bytes.NewBufferString(`{
		"connector":"fixture",
		"source":"fixture-source",
		"external_id":"fixture-object",
		"name":"fixture.txt"
	}`)
	req := httptest.NewRequest(http.MethodPost, "/v1/source-objects", body)
	req.Header.Set("X-Org-ID", "org-a")
	req = req.WithContext(authctx.IntoContext(req.Context(), &authctx.Claims{
		UserID: "verified-user", PrincipalType: "user", OrgID: "org-a",
		ZDR: true, ZDRPresent: true, Verified: true,
	}))

	recorder := httptest.NewRecorder()
	handler := NewSourceObjectHandler(nil)
	OrgIDMiddleware(http.HandlerFunc(handler.Upsert)).ServeHTTP(recorder, req)

	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", recorder.Code, http.StatusForbidden)
	}
	if !strings.Contains(recorder.Body.String(), "verified token zdr=true forbids durable source-object persistence") {
		t.Fatalf("unexpected response body: %s", recorder.Body.String())
	}
}

func TestSourceObjectPersistenceZDRReasonFailsClosed(t *testing.T) {
	durable := &authctx.Claims{ZDR: false, ZDRPresent: true, Verified: true}
	restrictive := &authctx.Claims{ZDR: true, ZDRPresent: true, Verified: true}
	missingPosture := &authctx.Claims{ZDR: false, Verified: true}

	if got := sourceObjectPersistenceZDRReason(nil); got != "verified retention posture is required for durable source-object persistence" {
		t.Fatalf("missing posture reason = %q", got)
	}
	if got := sourceObjectPersistenceZDRReason(restrictive); got != "verified token zdr=true forbids durable source-object persistence" {
		t.Fatalf("restrictive posture reason = %q", got)
	}
	if got := sourceObjectPersistenceZDRReason(missingPosture); got != "verified retention posture is required for durable source-object persistence" {
		t.Fatalf("missing signed boolean posture reason = %q", got)
	}
	if got := sourceObjectPersistenceZDRReason(durable); got != "" {
		t.Fatalf("non-restrictive posture was rejected: %q", got)
	}
}
