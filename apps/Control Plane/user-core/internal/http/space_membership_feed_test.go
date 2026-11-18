package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS15-S11: GET /spaces/:space_ref/membership-events (space_membership_feed.go).
// The pure tier covers the route's scope mapping and its gateway-delegation
// guard; the live tier (gated on TEST_DATABASE_URL via d12Pool, skipped
// without it) drives the real handler and repository to prove the per-role
// status matrix, including a removed member and an archived room.

const membershipEventsPathForScope = "/api/v1/internal/spaces/space-1/membership-events"

// TestMembershipEventsRouteScopeIsResolve pins serviceScopeForRequest's
// mapping directly, the way service_auth_test.go does for other routes, but
// in this slice's own file: service_auth_test.go is a shared golden table
// owned by a later consolidation pass.
func TestMembershipEventsRouteScopeIsResolve(t *testing.T) {
	req := httptest.NewRequest(http.MethodGet, membershipEventsPathForScope, nil)
	if got := serviceScopeForRequest(req); got != "spaces:resolve" {
		t.Fatalf("scope = %q, want spaces:resolve", got)
	}
}

// TestMembershipEventsRouteRequiresVerifiedGatewayDelegation needs no
// database: requireVerifiedSpaceResolver aborts on context fields alone,
// before the handler ever reaches the repository.
func TestMembershipEventsRouteRequiresVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"other service":       {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			req := httptest.NewRequest(http.MethodGet, "/api/v1/internal/spaces/room-1/membership-events", nil)
			rec := httptest.NewRecorder()
			engine.ServeHTTP(rec, req)
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

const (
	mfOrg      = "ws15s11-org"
	mfRoom     = "ws15s11-room"
	mfOwner    = "ws15s11-owner"
	mfEditor   = "ws15s11-editor"
	mfViewer   = "ws15s11-viewer"
	mfOutsider = "ws15s11-outsider"
	mfRemoved  = "ws15s11-removed"
)

// TestMembershipEventsRouteMatrix drives the real route (through the full
// Space route group, so requireVerifiedSpaceResolver and the registered
// scope are exercised too) against a real repository: owner, editor and
// viewer each get 200; an outsider and a removed member each get the typed
// membership-required 403; an archived room stays readable to a still-active
// member.
func TestMembershipEventsRouteMatrix(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	ctx := context.Background()

	cleanup := func() {
		for _, q := range []string{
			`DELETE FROM space_membership_events WHERE space_ref = '` + mfRoom + `'`,
			`DELETE FROM space_memberships WHERE space_ref = '` + mfRoom + `'`,
			`DELETE FROM space_authority_revisions WHERE space_ref = '` + mfRoom + `'`,
			`DELETE FROM registered_spaces WHERE space_ref = '` + mfRoom + `'`,
			`DELETE FROM user_org_memberships WHERE org_id = '` + mfOrg + `'`,
			`DELETE FROM users WHERE id LIKE 'ws15s11-%'`,
		} {
			_, _ = pool.Exec(ctx, q)
		}
	}
	cleanup()
	t.Cleanup(cleanup)

	exec := func(query string, args ...any) {
		t.Helper()
		if _, err := pool.Exec(ctx, query, args...); err != nil {
			t.Fatalf("%s: %v", query, err)
		}
	}
	exec(`INSERT INTO registered_spaces (space_ref, org_id, space_kind, owner_principal_id,
		application_lifecycle_revision, registration_state) VALUES ($1,$2,'room',$3,1,'active')`, mfRoom, mfOrg, mfOwner)
	exec(`INSERT INTO space_authority_revisions (space_ref) VALUES ($1)`, mfRoom)
	for _, user := range []string{mfOwner, mfEditor, mfViewer, mfOutsider, mfRemoved} {
		exec(`INSERT INTO users (id, email, name) VALUES ($1, $1 || '@ws15s11.invalid', $1)`, user)
		exec(`INSERT INTO user_org_memberships (user_id, org_id, role, status) VALUES ($1,$2,'member','active')`, user, mfOrg)
	}
	exec(`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'owner','ws15s11-test')`, mfRoom, mfOwner)
	exec(`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'editor','ws15s11-test')`, mfRoom, mfEditor)
	exec(`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by) VALUES ($1,'user',$2,'viewer','ws15s11-test')`, mfRoom, mfViewer)
	// Removed: was a member, now inactive -- distinct from the outsider, who
	// never had a membership row at all.
	exec(`INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, granted_by, active) VALUES ($1,'user',$2,'viewer','ws15s11-test',FALSE)`, mfRoom, mfRemoved)
	exec(`INSERT INTO space_membership_events
		(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id, created_at)
		VALUES ($1,$2,$3,1,'member_added','user','ws15s11-test',NOW())`, "sme_ws15s11_seed", mfRoom, mfOrg)

	get := func(user string) *httptest.ResponseRecorder {
		engine := spaceIssuerHarness(t, repo, map[string]any{
			"auth_method": "service_principal", "service_id": verevonGatewayPrincipal,
			"delegation_verified": true, "org_id": mfOrg, "user_id": user,
		})
		req := httptest.NewRequest(http.MethodGet, "/api/v1/internal/spaces/"+mfRoom+"/membership-events", nil)
		rec := httptest.NewRecorder()
		engine.ServeHTTP(rec, req)
		return rec
	}

	for _, user := range []string{mfOwner, mfEditor, mfViewer} {
		if w := get(user); w.Code != http.StatusOK {
			t.Fatalf("%s: want 200, got %d (%s)", user, w.Code, w.Body.String())
		}
	}

	var denialBody struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	if w := get(mfOutsider); w.Code != http.StatusForbidden {
		t.Fatalf("outsider: want 403, got %d (%s)", w.Code, w.Body.String())
	} else if err := json.Unmarshal(w.Body.Bytes(), &denialBody); err != nil || denialBody.Error.Code != string(spaces.DenialMembershipRequired) {
		t.Fatalf("outsider: want typed %s, got %s", spaces.DenialMembershipRequired, w.Body.String())
	}
	if w := get(mfRemoved); w.Code != http.StatusForbidden {
		t.Fatalf("removed member: want 403, got %d (%s)", w.Code, w.Body.String())
	} else if err := json.Unmarshal(w.Body.Bytes(), &denialBody); err != nil || denialBody.Error.Code != string(spaces.DenialMembershipRequired) {
		t.Fatalf("removed member: want typed %s, got %s", spaces.DenialMembershipRequired, w.Body.String())
	}

	// Archive the room: an active member can still read the feed.
	exec(`UPDATE registered_spaces SET registration_state='archived', archived_at=NOW(), archived_by=$2 WHERE space_ref=$1`, mfRoom, mfOwner)
	w := get(mfEditor)
	if w.Code != http.StatusOK {
		t.Fatalf("archived room, active member: want 200, got %d (%s)", w.Code, w.Body.String())
	}
	var archivedBody struct {
		Data spaces.MembershipFeed `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &archivedBody); err != nil {
		t.Fatalf("decode: %v (%s)", err, w.Body.String())
	}
	if len(archivedBody.Data.Events) == 0 {
		t.Fatalf("archived room: expected the seeded ledger row, got none: %s", w.Body.String())
	}
	// An outsider is still refused the same way once the room is archived.
	if w := get(mfOutsider); w.Code != http.StatusForbidden {
		t.Fatalf("outsider on archived room: want 403, got %d (%s)", w.Code, w.Body.String())
	}
}
