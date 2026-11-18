package http

import (
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// W3-DEC-KIND (decision D17): POST /api/v1/internal/spaces/space-kind-evidence.
// The pure tier below covers registration, scope, the gateway-delegation guard
// and request validation, none of which needs a database. The 403/200 legs
// against a real membership read are gated on TEST_DATABASE_URL at the end of
// the file, the same live pattern as thread_read_decision_live_test.go (D12)
// and thread_read_decisions_test.go (WS11-S1); they reuse that file's personal
// Space/room/owner/peer fixture (the d12* helpers and constants) rather than
// seeding a third copy of it.

const spaceKindEvidencePath = "/api/v1/internal/spaces/space-kind-evidence"

func TestSpaceKindEvidenceRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+spaceKindEvidencePath] != 1 {
		t.Fatalf("space-kind-evidence registered %d times, want 1", counts["POST "+spaceKindEvidencePath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, spaceKindEvidencePath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
}

func TestSpaceKindEvidenceRouteRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"other service":       {"auth_method": "service_principal", "service_id": "session-core", "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			rec := postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": "space-1"})
			if rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestSpaceKindEvidenceRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceIssuerHarness(t, nil, gatewayContext())
	rec := postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": "space-1"})
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

// Shape is refused with 400 before any membership read (the empty
// repository's nil db would error on a read, so a 400 proves none happened).
func TestSpaceKindEvidenceRouteRejectsBadShapesBeforeReadingAuthority(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	for name, body := range map[string]map[string]any{
		"no space_ref":    {},
		"blank space_ref": {"space_ref": "   "},
	} {
		t.Run(name, func(t *testing.T) {
			rec := postJSON(engine, spaceKindEvidencePath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), `"token"`) {
				t.Fatalf("a 400 carries no token: %s", rec.Body.String())
			}
		})
	}
}

// A resolver that cannot read authority at all (the empty repository's nil
// db) is a 503, never a 403 and never a token.
func TestSpaceKindEvidenceRouteReportsUnavailableWhenAuthorityCannotBeRead(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	rec := postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": "space-1"})
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
	if strings.Contains(rec.Body.String(), `"token"`) {
		t.Fatalf("a 503 carries no token: %s", rec.Body.String())
	}
}

type spaceKindEvidenceResponseBody struct {
	Data struct {
		Evidence spaces.SpaceKindEvidence `json:"evidence"`
		Token    string                   `json:"token"`
	} `json:"data"`
}

// verifySpaceKindEvidenceToken checks the signature the way Session Core's
// verifier would (version.key-id.payload.signature) and returns the decoded
// claim, so the live tests assert on what is actually signed.
func verifySpaceKindEvidenceToken(t *testing.T, token string, public ed25519.PublicKey) spaces.SpaceKindEvidence {
	t.Helper()
	parts := strings.Split(token, ".")
	if len(parts) != 4 {
		t.Fatalf("token has %d parts, want 4: %s", len(parts), token)
	}
	if parts[0] != spaces.SpaceKindEvidenceVersion {
		t.Fatalf("version tag = %q, want %q", parts[0], spaces.SpaceKindEvidenceVersion)
	}
	signature, err := base64.RawURLEncoding.DecodeString(parts[3])
	if err != nil {
		t.Fatalf("decode signature: %v", err)
	}
	if !ed25519.Verify(public, []byte(strings.Join(parts[:3], ".")), signature) {
		t.Fatal("the evidence's signature does not verify against the configured key")
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[2])
	if err != nil {
		t.Fatalf("decode payload: %v", err)
	}
	var evidence spaces.SpaceKindEvidence
	if err := json.Unmarshal(payload, &evidence); err != nil {
		t.Fatalf("decode evidence: %v", err)
	}
	return evidence
}

// Live legs (TEST_DATABASE_URL via d12Pool), reusing the D12 personal
// Space/room fixture from thread_read_decision_live_test.go: d12Personal is
// owned by d12Owner, d12Room is a room d12Owner also belongs to, and d12Peer
// is an ordinary org member with no membership in either.

func TestSpaceKindEvidenceRouteRefusesANonMemberLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, true)
	d12SigningEnv(t)
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = d12Org, d12Peer
	engine := spaceIssuerHarness(t, repo, ctx)

	rec := postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": d12Personal})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("non-member: status = %d, want 403 (%s)", rec.Code, rec.Body.String())
	}
	if strings.Contains(rec.Body.String(), `"token"`) {
		t.Fatalf("a refusal must carry no token: %s", rec.Body.String())
	}

	// A different org entirely, even for the owner's own subject id, is the
	// same refusal.
	ctx = gatewayContext()
	ctx["org_id"], ctx["user_id"] = "d12-other-org", d12Owner
	engine = spaceIssuerHarness(t, repo, ctx)
	rec = postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": d12Personal})
	if rec.Code != http.StatusForbidden {
		t.Fatalf("wrong org: status = %d, want 403 (%s)", rec.Code, rec.Body.String())
	}
}

func TestSpaceKindEvidenceRouteIssuesPersonalKindForAReadableMemberLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, true)
	public := d12SigningEnv(t)
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = d12Org, d12Owner
	engine := spaceIssuerHarness(t, repo, ctx)

	rec := postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": d12Personal})
	if rec.Code != http.StatusOK {
		t.Fatalf("personal owner: status = %d, want 200 (%s)", rec.Code, rec.Body.String())
	}
	var body spaceKindEvidenceResponseBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode: %v (%s)", err, rec.Body.String())
	}
	if body.Data.Evidence.Kind != "personal" {
		t.Fatalf(`kind = %q, want "personal"`, body.Data.Evidence.Kind)
	}
	if body.Data.Evidence.OrgID != d12Org || body.Data.Evidence.SpaceRef != d12Personal || body.Data.Evidence.SubjectID != d12Owner {
		t.Fatalf("evidence bound to (%s, %s, %s), want the owner in their own personal Space",
			body.Data.Evidence.OrgID, body.Data.Evidence.SpaceRef, body.Data.Evidence.SubjectID)
	}
	if body.Data.Token == "" {
		t.Fatal("a readable member got no token")
	}

	decoded := verifySpaceKindEvidenceToken(t, body.Data.Token, public)
	if decoded.Kind != "personal" || decoded.OrgID != d12Org || decoded.SpaceRef != d12Personal || decoded.SubjectID != d12Owner {
		t.Fatalf("signed evidence = %+v", decoded)
	}
	if !decoded.ExpiresAt.After(decoded.IssuedAt) {
		t.Fatalf("signed evidence does not expire after issuance: %+v", decoded)
	}
}

// A readable member of a shared room (not personal) gets the room's own kind
// back, proving the route resolves the real membership kind rather than
// hardcoding "personal".
func TestSpaceKindEvidenceRouteIssuesRoomKindForARoomMemberLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12Seed(t, pool, repo, true)
	public := d12SigningEnv(t)
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = d12Org, d12Owner
	engine := spaceIssuerHarness(t, repo, ctx)

	rec := postJSON(engine, spaceKindEvidencePath, map[string]any{"space_ref": d12Room})
	if rec.Code != http.StatusOK {
		t.Fatalf("room owner: status = %d, want 200 (%s)", rec.Code, rec.Body.String())
	}
	var body spaceKindEvidenceResponseBody
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode: %v (%s)", err, rec.Body.String())
	}
	if body.Data.Evidence.Kind != "room" {
		t.Fatalf(`kind = %q, want "room"`, body.Data.Evidence.Kind)
	}
	decoded := verifySpaceKindEvidenceToken(t, body.Data.Token, public)
	if decoded.Kind != "room" || decoded.SpaceRef != d12Room {
		t.Fatalf("signed evidence = %+v", decoded)
	}
}

// NOTE (per W3-DEC-KIND task scope): the shared golden scope tables in
// scopes_spaces_test.go / service_auth_test.go were intentionally left
// untouched. This file's own TestSpaceKindEvidenceRouteIsRegisteredOnceUnderTheIssueScope
// covers the spaces:issue mapping (it already passes: scopes_space_kind_evidence.go
// registers it through the per-feature seam, not the hand-maintained
// coreSpaceScope table). No further golden-table edit is owed for this scope.
