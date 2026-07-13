package handler

import (
	"net/http/httptest"
	"testing"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

func TestViewerIDRejectsUnverifiedForwardedHeader(t *testing.T) {
	req := httptest.NewRequest("GET", "/v1/documents", nil)
	req.Header.Set("X-User-ID", "spoofed-user")

	if got := viewerID(req); got != "" {
		t.Fatalf("viewerID trusted unverified header: got %q", got)
	}
}

func TestViewerIDUsesOnlyVerifiedClaims(t *testing.T) {
	req := httptest.NewRequest("GET", "/v1/documents", nil)
	verified := &authctx.Claims{UserID: "verified-user", PrincipalType: "user", OrgID: "org-a", Verified: true}
	req = req.WithContext(authctx.IntoContext(req.Context(), verified))

	if got := viewerID(req); got != "verified-user" {
		t.Fatalf("viewerID = %q, want verified-user", got)
	}
}

func TestRejectPersistentZDRContent(t *testing.T) {
	cases := []struct {
		name  string
		input model.CreateDocumentInput
		want  bool
	}{
		{
			name: "zdr content is rejected",
			input: model.CreateDocumentInput{
				Content:      "must remain ephemeral",
				IngestPolicy: &model.IngestPolicy{ZDRMode: "on"},
			},
			want: true,
		},
		{
			name: "zdr metadata-only is rejected",
			input: model.CreateDocumentInput{
				IngestPolicy: &model.IngestPolicy{EphemeralOnly: true},
			},
			want: true,
		},
		{
			name:  "ordinary content is allowed",
			input: model.CreateDocumentInput{Content: "durable"},
			want:  false,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := rejectsPersistentZDRContent(&tc.input); got != tc.want {
				t.Fatalf("rejectsPersistentZDRContent() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestPinDocumentOwnerRejectsCallerSelectedIdentity(t *testing.T) {
	input := model.CreateDocumentInput{OwnerID: "another-user"}
	if !pinDocumentOwner(&input, "verified-user") {
		t.Fatal("expected conflicting owner_id to be rejected")
	}
	input = model.CreateDocumentInput{}
	if pinDocumentOwner(&input, "verified-user") {
		t.Fatal("expected empty owner_id to be claim-pinned")
	}
	if input.OwnerID != "verified-user" || input.CreatedBy != "verified-user" {
		t.Fatalf("owner fields were not claim-pinned: %+v", input)
	}
}

func TestCanDeleteDocumentRequiresOwnershipOrWriteAllScope(t *testing.T) {
	doc := &model.Document{OwnerID: "owner-user"}
	owner := &authctx.Claims{UserID: "owner-user", PrincipalType: "user", Verified: true}
	reader := &authctx.Claims{UserID: "reader-user", PrincipalType: "user", Verified: true}
	admin := &authctx.Claims{
		UserID: "admin-user", PrincipalType: "user", Scopes: []string{"org:data:write_all"}, Verified: true,
	}
	if !canDeleteDocument(doc, owner) {
		t.Fatal("owner should be allowed to delete")
	}
	if canDeleteDocument(doc, reader) {
		t.Fatal("granted or org-visible reader must not be allowed to delete")
	}
	if !canDeleteDocument(doc, admin) {
		t.Fatal("explicit org write-all principal should be allowed to delete")
	}
}
