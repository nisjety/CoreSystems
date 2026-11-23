package handler

import (
	"net/http"
	"strings"
	"testing"
	"time"
)

// DEPTH-WS14-S3-1 step 3: a Control-signed content_digest binds the write to
// the posted body. These pin the documents-api half of the binding.

func TestSpaceImportContentDigestIsSha256OverTheContentString(t *testing.T) {
	// sha256("") is the well-known empty digest; the formula is the one
	// Control and Imports Core must reproduce.
	if got := spaceImportContentDigest(""); got != "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" {
		t.Fatalf("empty digest = %s", got)
	}
	if !spaceImportPayloadDigestPattern.MatchString(spaceImportContentDigest("hello room")) {
		t.Fatal("content digest is not the canonical sha256 commitment shape")
	}
}

func TestSpaceImportDecisionContentDigestShapeIsValidated(t *testing.T) {
	now := time.Now().UTC()
	d := baseDecision("org-1", "space:org-1:room-7", "user-1", "space_upload")
	d.ContentDigest = spaceImportContentDigest("x")
	if err := d.validate("org-1", now); err != nil {
		t.Fatalf("a canonical content digest was refused: %v", err)
	}
	for _, bad := range []string{"sha256:payload", "sha256:" + strings.Repeat("A", 64), strings.Repeat("a", 64)} {
		d.ContentDigest = bad
		if err := d.validate("org-1", now); err == nil {
			t.Errorf("content digest %q was accepted", bad)
		}
	}
}

func TestSpaceImportDecisionMatchesContent(t *testing.T) {
	d := baseDecision("org-1", "space:org-1:room-7", "user-1", "space_upload")
	if !d.matchesContent("anything") {
		t.Fatal("a legacy decision without a content digest must keep its old behaviour")
	}
	d.ContentDigest = spaceImportContentDigest("hello room")
	if !d.matchesContent("hello room") {
		t.Fatal("the signed body was refused")
	}
	if d.matchesContent("hello room ") || d.matchesContent("other body") {
		t.Fatal("a body other than the signed one was accepted")
	}
}

func TestCreateRefusesABodyThatDoesNotMatchTheSignedContentDigest(t *testing.T) {
	stub := &stubGrants{}
	f := newUploadFixture(t, roomFilesOn(stub))
	d := uploadDecision("user-1")
	d.ContentDigest = spaceImportContentDigest("the signed body")
	rec := f.post(uploadRequest{decision: d, body: map[string]any{"content": "a swapped body"}})
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "does not match the posted content") {
		t.Fatalf("swapped body: %d %s", rec.Code, rec.Body.String())
	}
	if len(stub.attaches) != 0 || rec.Header().Get(spaceImportAcceptedHeader) != "" {
		t.Fatal("a body mismatch must not reach the grant or the accepted header")
	}

	// The signed body passes the content gate and stops at the next gate (a
	// foreign owner_id), which proves the binding admitted it without a repo.
	rec = f.post(uploadRequest{decision: d, body: map[string]any{"content": "the signed body", "owner_id": "user-2"}})
	if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "owner_id must match the Space import decision subject") {
		t.Fatalf("signed body: %d %s", rec.Code, rec.Body.String())
	}
}
