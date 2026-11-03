package http

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS11-S1: the bulk shared-Space read decisions route. The pure tier covers
// registration, scope, the guard, request validation and the closed denial
// words; the per-row matrix against a real repository is at the end of the
// file (TEST_DATABASE_URL via d12Pool).

const threadReadDecisionsPath = "/api/v1/internal/spaces/thread-read-decisions"

func bulkItems(n int) []map[string]string {
	items := make([]map[string]string, n)
	for i := range items {
		items[i] = map[string]string{"space_ref": fmt.Sprintf("room-%d", i), "idempotency_key": fmt.Sprintf("read-%d", i)}
	}
	return items
}

func bulkBody(items any) map[string]any { return map[string]any{"items": items} }

func TestThreadReadDecisionsRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+threadReadDecisionsPath] != 1 {
		t.Fatalf("thread-read-decisions registered %d times, want 1", counts["POST "+threadReadDecisionsPath])
	}
	// The single route is still there, and is a different path.
	if counts["POST /api/v1/internal/spaces/thread-read-decision"] != 1 {
		t.Fatal("the single thread-read-decision route is gone")
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, threadReadDecisionsPath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	for _, rule := range spaceFeatureScopeRules {
		if _, ok := rule(http.MethodGet, threadReadDecisionsPath); ok {
			t.Error("a GET of thread-read-decisions must not be claimed by the issuer rule")
		}
	}
}

func TestThreadReadDecisionsRequiresAVerifiedGatewayCaller(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":             {},
		"other service":       {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true},
		"execution core":      {"auth_method": "service_principal", "service_id": executionCorePrincipal, "delegation_verified": true},
		"gateway, unverified": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			if rec := postJSON(engine, threadReadDecisionsPath, bulkBody(bulkItems(1))); rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestThreadReadDecisionsValidatesTheBatch(t *testing.T) {
	if rec := postJSON(spaceIssuerHarness(t, nil, gatewayContext()), threadReadDecisionsPath, bulkBody(bulkItems(1))); rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("no repository: status = %d, want 503", rec.Code)
	}
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	dup := []map[string]string{
		{"space_ref": "room-1", "idempotency_key": "a"}, {"space_ref": "room-1", "idempotency_key": "b"},
	}
	for name, body := range map[string]any{
		"no items":          bulkBody([]any{}),
		"missing items":     map[string]any{},
		"null items":        map[string]any{"items": nil},
		"sixty-one items":   bulkBody(bulkItems(maxThreadReadDecisionItems + 1)),
		"blank space_ref":   bulkBody([]map[string]string{{"space_ref": "  ", "idempotency_key": "a"}}),
		"missing space_ref": bulkBody([]map[string]string{{"idempotency_key": "a"}}),
		"blank key":         bulkBody([]map[string]string{{"space_ref": "room-1", "idempotency_key": ""}}),
		"duplicate space":   bulkBody(dup),
		"unknown audience":  map[string]any{"items": bulkItems(1), "service_audience": "billing"},
		"not an object":     []any{1},
		"items is a string": map[string]any{"items": "room-1"},
	} {
		t.Run(name, func(t *testing.T) {
			rec := postJSON(engine, threadReadDecisionsPath, body)
			if rec.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400 (%s)", rec.Code, rec.Body.String())
			}
			if strings.Contains(rec.Body.String(), `"token"`) {
				t.Fatalf("a 400 carries no token: %s", rec.Body.String())
			}
		})
	}
	if maxThreadReadDecisionItems != 60 {
		t.Fatalf("batch ceiling = %d, want 60", maxThreadReadDecisionItems)
	}
	// The ceiling itself is accepted by validation (rows then fail at resolution).
	if rec := postJSON(engine, threadReadDecisionsPath, bulkBody(bulkItems(maxThreadReadDecisionItems))); rec.Code != http.StatusOK {
		t.Fatalf("sixty items: status = %d, want 200 (%s)", rec.Code, rec.Body.String())
	}
	for _, audience := range []string{"", spaces.ThreadReadAudienceSessionCore, spaces.ThreadReadAudienceSandboxManager, spaces.ThreadReadAudienceSpaceStream} {
		body := map[string]any{"items": bulkItems(1), "service_audience": audience}
		if rec := postJSON(engine, threadReadDecisionsPath, body); rec.Code != http.StatusOK {
			t.Errorf("audience %q: status = %d, want 200 (%s)", audience, rec.Code, rec.Body.String())
		}
	}
}

type bulkRow struct {
	SpaceRef string          `json:"space_ref"`
	Status   string          `json:"status"`
	Reason   string          `json:"reason"`
	Token    string          `json:"token"`
	Decision spaces.Decision `json:"decision"`
}

func decodeBulk(t *testing.T, rec *httptest.ResponseRecorder) []bulkRow {
	t.Helper()
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d (%s)", rec.Code, rec.Body.String())
	}
	var body struct {
		Data struct {
			Results []bulkRow `json:"results"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode: %v (%s)", err, rec.Body.String())
	}
	return body.Data.Results
}

// A resolver that cannot answer is a per-row `unavailable`, never a 5xx for the
// whole batch and never a token.
func TestThreadReadDecisionsReportsAResolverFailurePerRow(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	items := bulkItems(3)
	rows := decodeBulk(t, postJSON(engine, threadReadDecisionsPath, bulkBody(items)))
	if len(rows) != 3 {
		t.Fatalf("rows = %d, want one per item", len(rows))
	}
	for i, row := range rows {
		if row.SpaceRef != items[i]["space_ref"] {
			t.Errorf("row %d is for %q, want request order (%q)", i, row.SpaceRef, items[i]["space_ref"])
		}
		if row.Status != "denied" || row.Reason != "unavailable" || row.Token != "" {
			t.Errorf("row %d = %+v, want denied/unavailable with no token", i, row)
		}
	}
}

func TestThreadReadDenialReasonUsesOnlyTheClosedWords(t *testing.T) {
	for name, tc := range map[string]struct {
		err  error
		own  *spaces.Denial
		want string
	}{
		"no membership":         {spaces.ErrNoCurrentMembership, nil, "not_member"},
		"wrapped no membership": {fmt.Errorf("resolve: %w", spaces.ErrNoCurrentMembership), nil, "not_member"},
		"membership required":   {&spaces.Denial{Code: spaces.DenialMembershipRequired}, nil, "not_member"},
		"audience not current":  {&spaces.Denial{Code: spaces.DenialAudienceNotCurrent}, nil, "not_member"},
		"not active":            {&spaces.Denial{Code: spaces.DenialNotActive}, nil, "not_active"},
		"authority unavailable": {spaces.AuthorityUnavailableDenial("db down"), nil, "unavailable"},
		"entitlement off":       {&spaces.Denial{Code: spaces.DenialEntitlementOff}, nil, "not_entitled"},
		"role below floor":      {&spaces.Denial{Code: spaces.DenialRoleBelowFloor}, nil, "not_entitled"},
		"zdr":                   {&spaces.Denial{Code: spaces.DenialZDRForbidsEffect}, nil, "not_entitled"},
		"unclassified typed":    {&spaces.Denial{Code: spaces.DenialKindUnsupported}, nil, "not_entitled"},
		"untyped failure":       {errors.New("pq: connection reset"), nil, "unavailable"},
		// A typed own-scheduled refusal outranks the shared-path miss.
		"own denial wins": {spaces.ErrNoCurrentMembership, &spaces.Denial{Code: spaces.DenialEntitlementOff}, "not_entitled"},
	} {
		t.Run(name, func(t *testing.T) {
			if got := threadReadDenialReason(tc.err, tc.own); got != tc.want {
				t.Fatalf("reason = %q, want %q", got, tc.want)
			}
		})
	}
	// A denied row names a reason and nothing else about the Space.
	row := deniedThreadReadRow("room-9", "not_member")
	if len(row) != 3 || row["space_ref"] != "room-9" || row["status"] != "denied" || row["reason"] != "not_member" {
		t.Fatalf("denied row = %v", row)
	}
}

// The single route and the bulk route share one resolution, so a change to the
// shared path or the own-scheduled fallback applies to both.
func TestSingleAndBulkThreadReadRoutesShareOneResolution(t *testing.T) {
	raw, err := os.ReadFile("spaces.go")
	if err != nil {
		t.Fatal(err)
	}
	src := strings.ReplaceAll(string(raw), "\r\n", "\n")
	single := src[strings.Index(src, "func (s *Server) issueThreadReadDecision(c *gin.Context)"):]
	single = single[:strings.Index(single, "\n}\n")]
	if !strings.Contains(single, "s.resolveThreadReadEvidence(") {
		t.Error("the single route no longer uses resolveThreadReadEvidence")
	}
	if strings.Contains(single, "ResolveSharedThreadReadDecisionEvidence(") {
		t.Error("the single route resolves evidence itself instead of through the shared helper")
	}
	bulk, err := os.ReadFile("routes_space_thread_read_decisions.go")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(bulk), "s.resolveThreadReadEvidence(") {
		t.Error("the bulk route no longer uses resolveThreadReadEvidence")
	}
}

// Live matrix (TEST_DATABASE_URL via d12Pool).

func TestThreadReadDecisionsMixedBatchLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	public := d12SigningEnv(t)
	room := newW2Room("ws11s1")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.ThreadReadEntitled = true })
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = room.Org, room.Editor
	engine := spaceIssuerHarness(t, repo, ctx)

	items := []map[string]string{
		{"space_ref": room.Room, "idempotency_key": "read-1"},
		{"space_ref": "ws11s1-no-such-room", "idempotency_key": "read-2"},
	}
	rows := decodeBulk(t, postJSON(engine, threadReadDecisionsPath, bulkBody(items)))
	if len(rows) != 2 || rows[0].SpaceRef != room.Room || rows[1].SpaceRef != "ws11s1-no-such-room" {
		t.Fatalf("rows are not one per item in request order: %+v", rows)
	}
	issued, denied := rows[0], rows[1]
	if issued.Status != "issued" || issued.Token == "" {
		t.Fatalf("member row = %+v, want issued with a token", issued)
	}
	decision := d12VerifyToken(t, issued.Token, public)
	if decision.ActionID != "model.thread.read" || len(decision.Permissions) != 1 || decision.Permissions[0] != "thread:read" ||
		decision.ActionSchemaHash != "sha256:thread-read-v1" {
		t.Fatalf("signed decision = %+v", decision)
	}
	if denied.Status != "denied" || denied.Reason != "not_member" || denied.Token != "" {
		t.Fatalf("non-member row = %+v, want denied/not_member", denied)
	}

	// The digest equals the single route's for the same evidence: recompute it
	// with the single route's issuer from the decision's own ref and nonce.
	evidence, err := repo.ResolveSharedThreadReadDecisionEvidence(context.Background(), room.Room, room.Org, room.Editor)
	if err != nil {
		t.Fatalf("resolve evidence: %v", err)
	}
	again, err := spaces.IssueSharedThreadReadDecision(evidence, spaces.SharedThreadReadDecisionRequest{
		DecisionRef: issued.Decision.DecisionRef, IdempotencyKey: "read-1", Nonce: issued.Decision.Nonce,
	}, time.Now().UTC())
	if err != nil {
		t.Fatalf("single issuer: %v", err)
	}
	if again.PayloadDigest != issued.Decision.PayloadDigest {
		t.Fatalf("digest %s differs from the single route's %s", issued.Decision.PayloadDigest, again.PayloadDigest)
	}

	// A requested audience is carried on every issued row.
	body := map[string]any{"items": items[:1], "service_audience": spaces.ThreadReadAudienceSpaceStream}
	rows = decodeBulk(t, postJSON(engine, threadReadDecisionsPath, body))
	if rows[0].Status != "issued" || rows[0].Decision.ServiceAudience != spaces.ThreadReadAudienceSpaceStream {
		t.Fatalf("audience row = %+v", rows[0])
	}

	// Entitlement off turns the same row into not_entitled.
	if _, err := pool.Exec(context.Background(), `UPDATE space_effect_policies SET thread_read_entitled=FALSE WHERE org_id=$1`, room.Org); err != nil {
		t.Fatal(err)
	}
	rows = decodeBulk(t, postJSON(engine, threadReadDecisionsPath, bulkBody(items[:1])))
	if rows[0].Status != "denied" || rows[0].Reason != "not_entitled" || rows[0].Token != "" {
		t.Fatalf("entitlement-off row = %+v, want denied/not_entitled", rows[0])
	}
	// A member removed since stops resolving at once.
	if _, err := pool.Exec(context.Background(), `UPDATE space_effect_policies SET thread_read_entitled=TRUE WHERE org_id=$1`, room.Org); err != nil {
		t.Fatal(err)
	}
	if _, err := pool.Exec(context.Background(), `UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, room.Room, room.Editor); err != nil {
		t.Fatal(err)
	}
	rows = decodeBulk(t, postJSON(engine, threadReadDecisionsPath, bulkBody(items[:1])))
	if rows[0].Status != "denied" || rows[0].Reason != "not_member" {
		t.Fatalf("removed-member row = %+v, want denied/not_member", rows[0])
	}
}

// The single route is unchanged by the extraction: a room member is issued, a
// stranger is refused with the membership message.
func TestSingleThreadReadDecisionUnchangedAfterTheExtractionLive(t *testing.T) {
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	public := d12SigningEnv(t)
	room := newW2Room("ws11s1s")
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) { p.ThreadReadEntitled = true })
	s := &Server{}
	s.SetSpaceRepository(repo)

	w := d12ReadDecision(t, s, room.Org, room.Viewer, room.Room)
	if w.Code != http.StatusOK {
		t.Fatalf("member: %d %s", w.Code, w.Body.String())
	}
	var issued d12DecisionBody
	_ = json.Unmarshal(w.Body.Bytes(), &issued)
	if decision := d12VerifyToken(t, issued.Data.Token, public); decision.ActionID != "model.thread.read" || decision.SpaceRef != room.Room {
		t.Fatalf("decision = %+v", decision)
	}
	w = d12ReadDecision(t, s, room.Org, "ws11s1s-stranger", room.Room)
	if w.Code != http.StatusForbidden || strings.Contains(w.Body.String(), `"token"`) {
		t.Fatalf("stranger: %d %s", w.Code, w.Body.String())
	}
}
