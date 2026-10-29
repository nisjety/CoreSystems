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

func TestPersistentZDRReasonIsMonotonic(t *testing.T) {
	restrictive := &authctx.Claims{
		UserID: "verified-user", PrincipalType: "user", OrgID: "org-a",
		ZDR: true, ZDRPresent: true, Verified: true,
	}
	durable := &authctx.Claims{
		UserID: "verified-user", PrincipalType: "user", OrgID: "org-a",
		ZDR: false, ZDRPresent: true, Verified: true,
	}
	missingPosture := &authctx.Claims{Verified: true}
	ordinary := &model.CreateDocumentInput{Content: "durable"}
	bodyRestricted := &model.CreateDocumentInput{
		Content:      "must remain ephemeral",
		IngestPolicy: &model.IngestPolicy{ZDRMode: "on"},
	}

	if got := persistentZDRReason(restrictive, ordinary); got != "verified token zdr=true forbids durable document persistence" {
		t.Fatalf("signed restrictive posture reason = %q", got)
	}
	if got := persistentZDRReason(durable, bodyRestricted); got != "ingest_policy.zdr_mode=on or ephemeral_only=true forbids durable document persistence" {
		t.Fatalf("body restrictive posture reason = %q", got)
	}
	if got := persistentZDRReason(durable, ordinary); got != "" {
		t.Fatalf("non-restrictive request was rejected: %q", got)
	}
	if got := persistentZDRReason(nil, ordinary); got != "verified retention posture is required for durable document persistence" {
		t.Fatalf("missing verified posture did not fail closed: %q", got)
	}
	if got := persistentZDRReason(missingPosture, ordinary); got != "verified retention posture is required for durable document persistence" {
		t.Fatalf("missing signed boolean posture did not fail closed: %q", got)
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

// A verified service principal (a connector) must be able to state the
// visibility it read off the source system. Before the service carve-out these
// callers looked identical to an interactive end user — `viewerID` only blanks
// out for `org:data:read_all` holders — so every connector document was forced
// to `private` and an explicit `org` was rejected outright.
func TestApplyVisibilityPolicyServicePrincipal(t *testing.T) {
	cases := []struct {
		name          string
		claims        *authctx.Claims
		requested     string
		wantForbidden bool
		wantResolved  string
	}{
		{
			name:         "service may set org explicitly",
			claims:       &authctx.Claims{ServiceID: "finspo-core", PrincipalType: "service", OrgID: "org-a", Verified: true},
			requested:    "org",
			wantResolved: "org",
		},
		{
			name:         "service may still set private",
			claims:       &authctx.Claims{ServiceID: "finspo-core", PrincipalType: "service", OrgID: "org-a", Verified: true},
			requested:    "private",
			wantResolved: "private",
		},
		{
			name:          "interactive user still cannot set org",
			claims:        &authctx.Claims{UserID: "u-1", PrincipalType: "user", OrgID: "org-a", Verified: true},
			requested:     "org",
			wantForbidden: true,
		},
		{
			name:         "interactive user still defaults to private",
			claims:       &authctx.Claims{UserID: "u-1", PrincipalType: "user", OrgID: "org-a", Verified: true},
			requested:    "",
			wantResolved: "private",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest("POST", "/v1/documents", nil)
			req = req.WithContext(authctx.IntoContext(req.Context(), tc.claims))
			input := model.CreateDocumentInput{Visibility: tc.requested}

			forbidden := applyVisibilityPolicy(req, &input)
			if forbidden != tc.wantForbidden {
				t.Fatalf("forbidden = %v, want %v", forbidden, tc.wantForbidden)
			}
			if !tc.wantForbidden && input.Visibility != tc.wantResolved {
				t.Fatalf("visibility = %q, want %q", input.Visibility, tc.wantResolved)
			}
		})
	}
}

// Locks the carve-out's precondition: only a VERIFIED service principal
// satisfies IsService(). An unverified claim set fails it regardless of the
// principal_type it asserts, so nothing a caller can forge reaches the
// carve-out. (An unverified request never reaches this handler at all — auth
// middleware rejects it upstream — but the predicate must hold on its own.)
func TestIsServiceRequiresVerifiedClaims(t *testing.T) {
	req := httptest.NewRequest("POST", "/v1/documents", nil)
	unverified := &authctx.Claims{ServiceID: "finspo-core", PrincipalType: "service", OrgID: "org-a", Verified: false}
	req = req.WithContext(authctx.IntoContext(req.Context(), unverified))

	if verifiedClaims(req).IsService() {
		t.Fatal("unverified service-shaped claims satisfied IsService()")
	}
}
