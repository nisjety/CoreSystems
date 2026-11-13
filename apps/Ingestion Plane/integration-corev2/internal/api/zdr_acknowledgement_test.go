package api

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/codexsubscription"
	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/oauth"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

const (
	ackOrg   = "org-1"
	ackOwner = "user-1"
)

// fakeOrgPosture stands in for org-core's retention posture read, the one external dependency of
// the acknowledgement. Its answer is what org-core would say for the organisation.
type fakeOrgPosture struct {
	on    bool
	err   error
	calls int
}

func (f *fakeOrgPosture) EffectiveZDR(context.Context, string) (bool, error) {
	f.calls++
	return f.on, f.err
}

// ackRig serves the real routes, middleware and store for one verified bearer principal.
type ackRig struct {
	t       *testing.T
	app     *fiber.App
	repo    *store.MemoryRepository
	events  *fakeEventsPublisher
	posture *fakeOrgPosture
}

func newAckRig(t *testing.T, principal auth.Principal, orgOn bool) *ackRig {
	t.Helper()
	cfg, repo, service := testOAuthStack(t)
	return buildAckRig(t, cfg, repo, service, principal, orgOn)
}

// newAckRigOn serves the routes over an existing repository, so several principals can look at
// the same ledger. Its OAuth service is not bound to that repository, which only the generic
// disconnect route would notice; the admin cases that use this rig never call it.
func newAckRigOn(t *testing.T, repo *store.MemoryRepository, principal auth.Principal, orgOn bool) *ackRig {
	t.Helper()
	cfg, _, service := testOAuthStack(t)
	return buildAckRig(t, cfg, repo, service, principal, orgOn)
}

func buildAckRig(t *testing.T, cfg config.Config, repo *store.MemoryRepository, service *oauth.Service, principal auth.Principal, orgOn bool) *ackRig {
	t.Helper()
	cfg.ZDRAcknowledgementEnabled = true
	events := &fakeEventsPublisher{}
	posture := &fakeOrgPosture{on: orgOn}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service, Events: events, OrgZDR: posture,
		Auth: fakeVerifier{principal: principal},
	})
	return &ackRig{t: t, app: app, repo: repo, events: events, posture: posture}
}

func ownerPrincipal(org, user string) auth.Principal {
	return auth.Principal{UserID: user, OrganizationID: org, PrincipalType: "user", Role: "member"}
}

// seedSubscription stores a ChatGPT subscription connection and assesses it the way the start-up
// reconcile does, so its capability record exists.
func (r *ackRig) seedSubscription(id, org, user, status string) {
	r.t.Helper()
	ctx := r.t.Context()
	if _, err := r.repo.UpsertConnection(ctx, store.Connection{
		ID: id, ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: org, WorkspaceID: "ws-" + org, UserID: user, Status: status,
		Capabilities: []string{codexsubscription.Capability},
	}); err != nil {
		r.t.Fatalf("seed connection %s: %v", id, err)
	}
	if _, err := ReconcileConnectionZDR(ctx, ServerConfig{Repo: r.repo}); err != nil {
		r.t.Fatalf("assess connections: %v", err)
	}
}

// bearer sends a request with a verified bearer token (the value is irrelevant: the fake verifier
// answers with the rig's principal).
func (r *ackRig) bearer(method, path, body string) (int, map[string]any) {
	r.t.Helper()
	return r.send(method, path, body, map[string]string{"Authorization": "Bearer token-1"})
}

// send sends a request with exactly these headers.
func (r *ackRig) send(method, path, body string, headers map[string]string) (int, map[string]any) {
	r.t.Helper()
	req := httptest.NewRequest(method, path, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	resp, err := r.app.Test(req)
	if err != nil {
		r.t.Fatalf("app.Test %s %s: %v", method, path, err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	var decoded map[string]any
	_ = json.Unmarshal(raw, &decoded)
	return resp.StatusCode, decoded
}

func errorCode(body map[string]any) string {
	errObj, _ := body["error"].(map[string]any)
	code, _ := errObj["code"].(string)
	return code
}

func dataOf(body map[string]any) map[string]any {
	data, _ := body["data"].(map[string]any)
	return data
}

func ackPath(connection string) string { return "/api/v1/model-connections/" + connection }

// ackBody is the body the SPA sends: the digest, text version and variant it displayed. The variant
// follows the entry point, as the SPA shows it: the first-use text on first use, the ZDR-on text otherwise.
func ackBody(digest, textVersion, locale, via string) string {
	variant := "org_zdr_on"
	if via == "first_use" {
		variant = "first_use"
	}
	return ackBodyWithVariant(digest, textVersion, locale, via, variant)
}

// ackBodyWithVariant is the body with the variant the member was shown.
func ackBodyWithVariant(digest, textVersion, locale, via, variant string) string {
	return `{"digest":"` + digest + `","textVersion":"` + textVersion + `","locale":"` + locale + `","variant":"` + variant + `","acknowledgedVia":"` + via + `"}`
}

func currentDigest(t *testing.T, repo *store.MemoryRepository, connection string) string {
	t.Helper()
	record, err := repo.GetConnectionZDR(t.Context(), connection)
	if err != nil {
		t.Fatalf("record for %s: %v", connection, err)
	}
	return record.Digest
}

func activeAcks(t *testing.T, repo *store.MemoryRepository, connection string) []store.ConnectionZDRAcknowledgement {
	t.Helper()
	rows, err := repo.ListConnectionZDRAcknowledgements(t.Context(), connection, true)
	if err != nil {
		t.Fatalf("list acknowledgements: %v", err)
	}
	return rows
}

func sha256Of(text string) string {
	sum := sha256.Sum256([]byte(text))
	return hex.EncodeToString(sum[:])
}

func TestAckRoutesAreAbsentWhileTheSwitchIsOff(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.ZDRAcknowledgementEnabled = false
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service, Auth: fakeVerifier{principal: ownerPrincipal(ackOrg, ackOwner)},
		Events: &fakeEventsPublisher{}, OrgZDR: &fakeOrgPosture{on: true},
	})
	// The owner's connection exists and is assessed, so a mounted route would answer it with its own
	// JSON envelope. Only the router's bare 404 means the route is not there.
	rig := &ackRig{t: t, app: app, repo: repo, events: &fakeEventsPublisher{}}
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	for _, route := range []struct{ method, path, body string }{
		{http.MethodGet, ackPath("conn-1") + "/zdr", ""},
		{http.MethodPost, ackPath("conn-1") + "/zdr-acknowledgement", ackBody(currentDigest(t, repo, "conn-1"), "v1", "no", "connect")},
		{http.MethodDelete, ackPath("conn-1") + "/zdr-acknowledgement", ""},
		{http.MethodGet, ackPath("conn-1") + "/zdr-posture", ""},
		{http.MethodGet, zdrAcknowledgementBase + "/zdr-acknowledgements", ""},
	} {
		status, body := rig.bearer(route.method, route.path, route.body)
		if status != http.StatusNotFound || body["success"] != nil {
			t.Fatalf("%s %s with the switch off answered %d %v, want the router's bare 404 (dark by default)", route.method, route.path, status, body)
		}
	}
	if rows := activeAcks(t, repo, "conn-1"); len(rows) != 0 {
		t.Fatal("a dark route wrote an acknowledgement")
	}
}

func TestTheInternalKeyCannotReachAnyAckRoute(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	internal := map[string]string{"X-Internal-API-Key": "dev-key", "X-User-ID": ackOwner, "X-Org-ID": ackOrg}
	for _, route := range []struct{ method, path, body string }{
		{http.MethodGet, ackPath("conn-1") + "/zdr", ""},
		{http.MethodPost, ackPath("conn-1") + "/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect")},
		{http.MethodDelete, ackPath("conn-1") + "/zdr-acknowledgement", ""},
		{http.MethodGet, ackPath("conn-1") + "/zdr-posture", ""},
		{http.MethodGet, "/api/v1/model-connections/zdr-acknowledgements", ""},
	} {
		status, body := rig.send(route.method, route.path, route.body, internal)
		// The refusal must come from the bearer-only guard itself, before any handler runs.
		errObj, _ := body["error"].(map[string]any)
		message, _ := errObj["message"].(string)
		if status != http.StatusUnauthorized || message != "Bearer token required" {
			t.Fatalf("%s %s with the internal key: status %d (%v), want the bearer-only guard's 401", route.method, route.path, status, body)
		}
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatalf("an internal-key call wrote an acknowledgement: %+v", rows)
	}
}

func TestOnlyTheOwnerOfAConnectionMayActOnItsAcknowledgement(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, "user-2"), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")
	for _, route := range []struct{ method, path, body string }{
		{http.MethodGet, ackPath("conn-1") + "/zdr", ""},
		{http.MethodPost, ackPath("conn-1") + "/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect")},
		{http.MethodDelete, ackPath("conn-1") + "/zdr-acknowledgement", ""},
		{http.MethodGet, ackPath("conn-1") + "/zdr-posture", ""},
	} {
		status, body := rig.bearer(route.method, route.path, route.body)
		if status != http.StatusForbidden || errorCode(body) != "not_connection_owner" {
			t.Fatalf("%s %s by a member who does not own the connection: %d %v, want 403 not_connection_owner", route.method, route.path, status, body)
		}
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatalf("a non-owner wrote an acknowledgement: %+v", rows)
	}
}

func TestAConnectionInAnotherOrganisationIsRefusedWith403(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal("org-2", ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")
	for _, route := range []struct{ method, path, body string }{
		{http.MethodGet, ackPath("conn-1") + "/zdr", ""},
		{http.MethodPost, ackPath("conn-1") + "/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect")},
		{http.MethodDelete, ackPath("conn-1") + "/zdr-acknowledgement", ""},
		{http.MethodGet, ackPath("conn-1") + "/zdr-posture", ""},
	} {
		status, body := rig.bearer(route.method, route.path, route.body)
		if status != http.StatusForbidden || errorCode(body) != "forbidden" {
			t.Fatalf("%s %s from another organisation: %d %v, want 403 forbidden", route.method, route.path, status, body)
		}
	}
}

func TestAcknowledgingStoresTheComposedTextItsHashAndOneAuditRow(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")

	status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect"))
	if status != http.StatusCreated {
		t.Fatalf("acknowledge: %d %v, want 201", status, body)
	}
	saved := activeAcks(t, rig.repo, "conn-1")
	if len(saved) != 1 {
		t.Fatalf("active acknowledgements = %d, want 1", len(saved))
	}
	ack := saved[0]
	wording, err := zdr.DefaultWording()
	if err != nil {
		t.Fatal(err)
	}
	text, err := wording.Lookup("chatgpt_subscription_not_supported", "org_zdr_on", "no")
	if err != nil {
		t.Fatal(err)
	}
	if ack.TextSnapshot != text.Snapshot || ack.TextSHA256 != text.SHA256 || ack.TextSHA256 != sha256Of(ack.TextSnapshot) {
		t.Fatalf("the stored text is not the composed text and its hash: snapshot matches=%v hash matches=%v",
			ack.TextSnapshot == text.Snapshot, ack.TextSHA256 == text.SHA256)
	}
	if ack.TextVersion != "v1" || ack.Locale != "no" || !ack.OrgZDRAtAck || ack.AcknowledgedVia != "connect" ||
		ack.Digest != digest || ack.UserID != ackOwner || ack.OrganizationID != ackOrg {
		t.Fatalf("stored acknowledgement is wrong: %+v", ack)
	}

	audit := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledged")
	if len(audit) != 1 || audit[0].Metadata["digest"] != digest {
		t.Fatalf("acknowledgement audit rows = %+v, want one bound to the digest", audit)
	}
	var published bool
	for _, event := range rig.events.events {
		if event.Type == "verevon.ingestion.integration.connection_updated" && event.Data["change"] == "zdr_acknowledged" {
			published = true
		}
	}
	if !published {
		t.Fatal("the acknowledgement was not announced on the integration events subject")
	}
}

// The ledger holds the text the member was shown: the variant in the body picks it, and a body that
// names no acknowledgeable variant is refused before anything is written.
func TestTheLedgerHoldsTheTextTheMemberWasShown(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")
	wording, err := zdr.DefaultWording()
	if err != nil {
		t.Fatal(err)
	}
	firstUse, err := wording.Lookup("chatgpt_subscription_not_supported", "first_use", "en")
	if err != nil {
		t.Fatal(err)
	}

	for _, body := range []string{
		`{"digest":"` + digest + `","textVersion":"v1","locale":"en","acknowledgedVia":"connect"}`,
		ackBodyWithVariant(digest, "v1", "en", "connect", "org_zdr_off"),
	} {
		if status, got := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", body); status != http.StatusBadRequest || errorCode(got) != "invalid_variant" {
			t.Fatalf("ack %s: %d %v, want 400 invalid_variant", body, status, got)
		}
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatal("a refused acknowledgement was stored")
	}

	// Shown the first-use text but acknowledged through the connect flow: the ledger keeps that text.
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBodyWithVariant(digest, "v1", "en", "connect", "first_use")); status != http.StatusCreated {
		t.Fatalf("ack: %d %v", status, body)
	}
	saved := activeAcks(t, rig.repo, "conn-1")
	if len(saved) != 1 || saved[0].TextSHA256 != firstUse.SHA256 || saved[0].TextSnapshot != firstUse.Snapshot {
		t.Fatalf("the ledger does not hold the first-use text the member was shown: %+v", saved)
	}
}

func TestReplayingTheAcknowledgementIsIdempotent(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")
	status, first := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect"))
	if status != http.StatusCreated {
		t.Fatalf("first acknowledgement: %d %v", status, first)
	}
	status, second := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect"))
	if status != http.StatusOK || dataOf(second)["created"] != false {
		t.Fatalf("replay: %d %v, want 200 with created=false", status, second)
	}
	if dataOf(first)["acknowledgement"].(map[string]any)["id"] != dataOf(second)["acknowledgement"].(map[string]any)["id"] {
		t.Fatal("a replay must return the stored acknowledgement, not a new one")
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 1 {
		t.Fatalf("replay left %d active rows, want 1", len(rows))
	}
	if audit := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledged"); len(audit) != 1 {
		t.Fatalf("replay wrote %d audit rows, want the one from the first call", len(audit))
	}
}

func TestAStaleDigestOrTextVersionIsRefusedWithTheCurrentAssessment(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")
	stale := strings.Repeat("0", 64)
	for _, body := range []string{
		ackBody(stale, "v1", "no", "connect"),
		ackBody(digest, "v0", "no", "connect"),
	} {
		status, got := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", body)
		if status != http.StatusConflict || errorCode(got) != "zdr_assessment_changed" {
			t.Fatalf("stale ack %s: %d %v, want 409 zdr_assessment_changed", body, status, got)
		}
		if got["error"].(map[string]any)["current"] == nil {
			t.Fatalf("the 409 must carry the current assessment so the client can show it: %v", got)
		}
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatalf("a stale acknowledgement was stored: %+v", rows)
	}
}

func TestAcknowledgementNeedsTheOrganisationToHaveZDROn(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), false)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect"))
	if status != http.StatusConflict || errorCode(body) != "zdr_org_not_on" {
		t.Fatalf("ack with ZDR off in the organisation: %d %v, want 409 zdr_org_not_on", status, body)
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatal("an acknowledgement was stored although the organisation does not use ZDR")
	}
}

func TestAnUnavailableOrgPostureFailsClosed(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.posture.err = auth.NewError(http.StatusServiceUnavailable, "org_core_unreachable", "down")
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect"))
	if status != http.StatusServiceUnavailable || errorCode(body) != "zdr_org_state_unavailable" {
		t.Fatalf("ack with org-core down: %d %v, want 503 zdr_org_state_unavailable", status, body)
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatal("an acknowledgement was stored with the organisation posture unknown")
	}
}

func TestAnUnassessedOrSupportedOrInactiveConnectionIsRefused(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)

	// Unassessed: the connection exists but no capability record does.
	if _, err := rig.repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-new", ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: ackOrg, UserID: ackOwner, Status: "active",
	}); err != nil {
		t.Fatal(err)
	}
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-new")+"/zdr-acknowledgement", ackBody(strings.Repeat("a", 64), "v1", "no", "connect")); status != http.StatusConflict || errorCode(body) != "zdr_unassessed" {
		t.Fatalf("unassessed connection: %d %v, want 409 zdr_unassessed", status, body)
	}

	// Inactive: a pending sign-in cannot be acknowledged.
	rig.seedSubscription("conn-pending", ackOrg, ackOwner, "pending")
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-pending")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-pending"), "v1", "no", "connect")); status != http.StatusConflict || errorCode(body) != "connection_not_active" {
		t.Fatalf("pending connection: %d %v, want 409 connection_not_active", status, body)
	}

	// Supported: a capability that keeps ZDR needs no acknowledgement. No shipped provider is
	// supported today, so the record is written directly with a digest the contract computes.
	digest, err := zdr.Digest(codexsubscription.ProviderKey, "codex_chatgpt_device_code", zdr.CapabilitySupported, zdr.BasisVerevonAttestedDeployment, 1)
	if err != nil {
		t.Fatal(err)
	}
	rig.seedSubscription("conn-supported", ackOrg, ackOwner, "active")
	if _, err := rig.repo.ApplyConnectionZDRAssessment(t.Context(), store.ConnectionZDRAssessment{
		ConnectionID: "conn-supported", ProviderKey: codexsubscription.ProviderKey, AuthMode: "codex_chatgpt_device_code",
		Capability: zdr.CapabilitySupported, Basis: zdr.BasisVerevonAttestedDeployment, BasisRef: "test@1", CatalogVersion: 1, Digest: digest,
	}); err != nil {
		t.Fatal(err)
	}
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-supported")+"/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect")); status != http.StatusConflict || errorCode(body) != "zdr_acknowledgement_not_required" {
		t.Fatalf("supported connection: %d %v, want 409 zdr_acknowledgement_not_required", status, body)
	}
}

func TestMalformedAcknowledgementsAreRefusedBeforeAnyWrite(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")
	for _, body := range []string{
		ackBody("not-a-digest", "v1", "no", "connect"),
		ackBody(digest, "v1", "sv", "connect"),
		ackBody(digest, "v1", "no", "magic"),
		`{"digest":`,
	} {
		if status, got := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", body); status != http.StatusBadRequest {
			t.Fatalf("malformed ack %s: %d %v, want 400", body, status, got)
		}
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatal("a malformed acknowledgement was stored")
	}
}

func TestTheAssessmentRouteComposesTheTextForTheLocaleAndVariant(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	status, body := rig.bearer(http.MethodGet, ackPath("conn-1")+"/zdr?locale=en&variant=first_use", "")
	if status != http.StatusOK {
		t.Fatalf("assessment: %d %v", status, body)
	}
	data := dataOf(body)
	if data["capability"] != "not_supported" || data["requiresAcknowledgement"] != true || data["digest"] != currentDigest(t, rig.repo, "conn-1") {
		t.Fatalf("assessment data = %v", data)
	}
	text := data["text"].(map[string]any)
	wording, err := zdr.DefaultWording()
	if err != nil {
		t.Fatal(err)
	}
	want, err := wording.Lookup("chatgpt_subscription_not_supported", "first_use", "en")
	if err != nil {
		t.Fatal(err)
	}
	if text["sha256"] != want.SHA256 || text["title"] != want.Title || text["variant"] != "first_use" || text["locale"] != "en" {
		t.Fatalf("assessment text = %v, want the EN first_use text", text)
	}
	if status, body := rig.bearer(http.MethodGet, ackPath("conn-1")+"/zdr?locale=sv", ""); status != http.StatusBadRequest {
		t.Fatalf("unknown locale: %d %v, want 400", status, body)
	}
}

func TestRevokingIsOwnerOnlyAuditedAndIdempotent(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect")); status != http.StatusCreated {
		t.Fatalf("ack: %d %v", status, body)
	}
	drainAudit(t, rig.repo)

	status, body := rig.bearer(http.MethodDelete, ackPath("conn-1")+"/zdr-acknowledgement", "")
	if status != http.StatusOK || dataOf(body)["revoked"] != float64(1) {
		t.Fatalf("revoke: %d %v, want 200 revoked=1", status, body)
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 0 {
		t.Fatalf("active after revoke: %+v", rows)
	}
	revokedAudit := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledgement_revoked")
	if len(revokedAudit) != 1 || revokedAudit[0].Metadata["reason"] != store.ZDRRevokeOwnerRevoked {
		t.Fatalf("revoke audit = %+v, want one owner_revoked row", revokedAudit)
	}

	status, body = rig.bearer(http.MethodDelete, ackPath("conn-1")+"/zdr-acknowledgement", "")
	if status != http.StatusOK || dataOf(body)["revoked"] != float64(0) {
		t.Fatalf("second revoke: %d %v, want 200 revoked=0", status, body)
	}
	if again := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledgement_revoked"); len(again) != 0 {
		t.Fatalf("a second revoke audited again: %+v", again)
	}

	// The user can acknowledge the same assessment again after revoking it.
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect")); status != http.StatusCreated {
		t.Fatalf("re-acknowledge after revoke: %d %v, want 201", status, body)
	}
}

func TestThePostureReadCarriesOnlyTheDigestTextVersionAndAckState(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	digest := currentDigest(t, rig.repo, "conn-1")

	status, body := rig.bearer(http.MethodGet, ackPath("conn-1")+"/zdr-posture", "")
	if status != http.StatusOK {
		t.Fatalf("posture before ack: %d %v", status, body)
	}
	assertPostureKeys(t, dataOf(body))
	if dataOf(body)["ackState"] != "required" || dataOf(body)["digest"] != digest || dataOf(body)["textVersion"] != "v1" || dataOf(body)["ackId"] != "" {
		t.Fatalf("posture before ack = %v, want required with the digest and v1", dataOf(body))
	}

	status, ackBodyOut := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(digest, "v1", "no", "connect"))
	if status != http.StatusCreated {
		t.Fatalf("ack: %d %v", status, ackBodyOut)
	}
	ackID := dataOf(ackBodyOut)["acknowledgement"].(map[string]any)["id"]
	_, body = rig.bearer(http.MethodGet, ackPath("conn-1")+"/zdr-posture", "")
	if dataOf(body)["ackState"] != "acknowledged" || dataOf(body)["ackId"] != ackID {
		t.Fatalf("posture after ack = %v, want acknowledged with the ack id", dataOf(body))
	}

	rig.bearer(http.MethodDelete, ackPath("conn-1")+"/zdr-acknowledgement", "")
	_, body = rig.bearer(http.MethodGet, ackPath("conn-1")+"/zdr-posture", "")
	if dataOf(body)["ackState"] != "required" {
		t.Fatalf("posture after revoke = %v, want required again", dataOf(body))
	}

	rig.seedSubscription("conn-new", ackOrg, ackOwner, "active")
	if _, err := rig.repo.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-bare", ProviderKey: codexsubscription.ProviderKey, ConnectorType: codexsubscription.ConnectorType,
		OrganizationID: ackOrg, UserID: ackOwner, Status: "active",
	}); err != nil {
		t.Fatal(err)
	}
	_, body = rig.bearer(http.MethodGet, ackPath("conn-bare")+"/zdr-posture", "")
	if dataOf(body)["ackState"] != "unassessed" {
		t.Fatalf("posture of an unassessed connection = %v, want unassessed", dataOf(body))
	}
}

func TestThePostureReadIsNeverCached(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	req := httptest.NewRequest(http.MethodGet, ackPath("conn-1")+"/zdr-posture", nil)
	req.Header.Set("Authorization", "Bearer token-1")
	resp, err := rig.app.Test(req)
	if err != nil {
		t.Fatal(err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK || resp.Header.Get("Cache-Control") != "no-store" {
		t.Fatalf("posture read: status %d, Cache-Control %q; want 200 and no-store", resp.StatusCode, resp.Header.Get("Cache-Control"))
	}
}

func assertPostureKeys(t *testing.T, data map[string]any) {
	t.Helper()
	allowed := map[string]bool{
		"connectionId": true, "ackState": true, "ackId": true, "digest": true, "textVersion": true,
		"providerKey": true, "capability": true, "waiverRoute": true, "ackReason": true,
	}
	for key := range data {
		if !allowed[key] {
			t.Fatalf("posture read leaks %q; it may carry only the posture fields (no basis, organisation, request id or text)", key)
		}
	}
	if len(data) != len(allowed) {
		t.Fatalf("posture read carries %d fields, want exactly %d", len(data), len(allowed))
	}
}

func TestTheAdminListNeedsAnOrgAdminAndShowsIdsAndCountsNotTheText(t *testing.T) {
	member := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	member.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	member.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, member.repo, "conn-1"), "v1", "no", "connect"))
	if status, body := member.bearer(http.MethodGet, "/api/v1/model-connections/zdr-acknowledgements", ""); status != http.StatusForbidden || errorCode(body) != "org_admin_required" {
		t.Fatalf("member listing: %d %v, want 403 org_admin_required", status, body)
	}

	for _, role := range []string{"admin", "owner", "member,admin"} {
		admin := newAckRigOn(t, member.repo, auth.Principal{UserID: "admin-1", OrganizationID: ackOrg, PrincipalType: "user", Role: role}, true)
		status, body := admin.bearer(http.MethodGet, "/api/v1/model-connections/zdr-acknowledgements", "")
		if status != http.StatusOK {
			t.Fatalf("role %q listing: %d %v", role, status, body)
		}
		data := dataOf(body)
		counts := data["counts"].(map[string]any)
		if counts["active"] != float64(1) || counts["revoked"] != float64(0) {
			t.Fatalf("role %q counts = %v", role, counts)
		}
		items := data["items"].([]any)
		if len(items) != 1 {
			t.Fatalf("role %q items = %d, want 1", role, len(items))
		}
		item := items[0].(map[string]any)
		for _, forbidden := range []string{"textSnapshot", "textSha256", "digest", "requestId"} {
			if _, present := item[forbidden]; present {
				t.Fatalf("admin listing shows %q; it carries ids and counts only", forbidden)
			}
		}
		if item["id"] == "" || item["connectionId"] != "conn-1" || item["userId"] != ackOwner || item["status"] != "active" {
			t.Fatalf("admin listing item = %v", item)
		}
	}

	other := newAckRigOn(t, member.repo, auth.Principal{UserID: "admin-2", OrganizationID: "org-2", PrincipalType: "user", Role: "owner"}, true)
	_, body := other.bearer(http.MethodGet, "/api/v1/model-connections/zdr-acknowledgements", "")
	if items := dataOf(body)["items"].([]any); len(items) != 0 {
		t.Fatalf("another organisation's admin saw %d rows", len(items))
	}
}

func TestTheExistingConsentRoutesStillWorkAndNeverTouchTheLedger(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect"))
	status, body := rig.send(http.MethodGet, "/api/v1/connections/conn-1/consents", "", map[string]string{"X-Internal-API-Key": "dev-key"})
	if status != http.StatusOK {
		t.Fatalf("consents read: %d %v, want 200 as before", status, body)
	}
	if rows := activeAcks(t, rig.repo, "conn-1"); len(rows) != 1 {
		t.Fatalf("a consent read changed the ledger: %+v", rows)
	}
}

func TestDisconnectingASubscriptionRevokesItsAcknowledgementInTheSameTransaction(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	cfg.ZDRAcknowledgementEnabled = true
	runner := &zdrLoginRunner{process: &zdrLoginProcess{status: "pending"}}
	manager, err := codexsubscription.NewManager(codexsubscription.Config{Enabled: true, Home: t.TempDir()}, runner)
	if err != nil {
		t.Fatalf("NewManager: %v", err)
	}
	runner.manager = manager
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service, CodexSubscriptions: manager,
		Auth: fakeVerifier{principal: ownerPrincipal(ackOrg, ackOwner)}, Events: &fakeEventsPublisher{}, OrgZDR: &fakeOrgPosture{on: true},
	})
	rig := &ackRig{t: t, app: app, repo: repo, events: &fakeEventsPublisher{}}
	started := startSubscription(t, func(method, path, body string) (int, string) {
		status, raw := rig.send(method, path, body, map[string]string{"X-Internal-API-Key": "dev-key"})
		encoded, _ := json.Marshal(raw)
		return status, string(encoded)
	})
	connection := started.Data.Connection.ID
	runner.process.status = "connected"
	if status, raw := rig.send(http.MethodGet, "/api/v1/model-subscriptions/openai-codex/connect/"+connection+"/"+started.Data.Login.LoginID, "", map[string]string{"X-Internal-API-Key": "dev-key"}); status != http.StatusOK {
		t.Fatalf("connected poll: %d %v", status, raw)
	}
	if status, body := rig.bearer(http.MethodPost, ackPath(connection)+"/zdr-acknowledgement", ackBody(currentDigest(t, repo, connection), "v1", "no", "connect")); status != http.StatusCreated {
		t.Fatalf("ack: %d %v", status, body)
	}
	drainAudit(t, repo)

	if status, body := rig.send(http.MethodDelete, "/api/v1/model-subscriptions/openai-codex/connections/"+connection, "", map[string]string{"X-Internal-API-Key": "dev-key"}); status != http.StatusNoContent {
		t.Fatalf("disconnect: %d %v", status, body)
	}
	if rows, err := repo.ListConnectionZDRAcknowledgements(t.Context(), connection, false); err != nil || len(rows) != 1 || rows[0].RevokeReason != store.ZDRRevokeConnectionRemoved {
		t.Fatalf("after disconnect the ledger = %+v (%v), want the acknowledgement revoked for connection_removed", rows, err)
	}
	revoked := auditOfType(drainAudit(t, repo), "subscription_connection.zdr_acknowledgement_revoked")
	if len(revoked) != 1 || revoked[0].Metadata["reason"] != store.ZDRRevokeConnectionRemoved {
		t.Fatalf("disconnect audit = %+v, want one connection_removed revocation row", revoked)
	}
}

func TestTheGenericDisconnectRouteRevokesAModelConnectionsAcknowledgement(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	if status, body := rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect")); status != http.StatusCreated {
		t.Fatalf("ack: %d %v", status, body)
	}
	drainAudit(t, rig.repo)

	if status, body := rig.bearer(http.MethodDelete, "/api/v1/connections/conn-1", ""); status != http.StatusNoContent {
		t.Fatalf("generic disconnect: %d %v, want 204", status, body)
	}
	rows, err := rig.repo.ListConnectionZDRAcknowledgements(t.Context(), "conn-1", false)
	if err != nil || len(rows) != 1 || rows[0].RevokeReason != store.ZDRRevokeConnectionRemoved {
		t.Fatalf("after the generic disconnect the ledger = %+v (%v), want the acknowledgement revoked for connection_removed", rows, err)
	}
	if revoked := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledgement_revoked"); len(revoked) != 1 {
		t.Fatalf("generic disconnect audit rows = %d, want one per revoked acknowledgement", len(revoked))
	}
}

func TestAGDPRUserDeleteRevokesTheUsersAcknowledgementsAndKeepsTheRecord(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect"))
	status, body := rig.send(http.MethodPost, "/internal/gdpr/delete", `{"organizationId":"org-1","userId":"user-1","reason":"request"}`, map[string]string{"X-Internal-API-Key": "dev-key"})
	if status != http.StatusOK {
		t.Fatalf("user delete: %d %v", status, body)
	}
	rows, err := rig.repo.ListConnectionZDRAcknowledgements(t.Context(), "conn-1", false)
	if err != nil || len(rows) != 1 || rows[0].RevokeReason != store.ZDRRevokeConnectionRemoved {
		t.Fatalf("after a user delete the ledger = %+v (%v), want the acknowledgement revoked and kept for the audit window", rows, err)
	}
	if _, err := rig.repo.GetConnectionZDR(t.Context(), "conn-1"); err != nil {
		t.Fatalf("a user delete must not erase the capability record: %v", err)
	}
}

func TestAGDPROrganisationDeleteErasesTheLedger(t *testing.T) {
	rig := newAckRig(t, ownerPrincipal(ackOrg, ackOwner), true)
	rig.seedSubscription("conn-1", ackOrg, ackOwner, "active")
	rig.bearer(http.MethodPost, ackPath("conn-1")+"/zdr-acknowledgement", ackBody(currentDigest(t, rig.repo, "conn-1"), "v1", "no", "connect"))
	status, body := rig.send(http.MethodPost, "/internal/gdpr/delete", `{"organizationId":"org-1","reason":"organisation purge"}`, map[string]string{"X-Internal-API-Key": "dev-key"})
	if status != http.StatusOK {
		t.Fatalf("organisation delete: %d %v", status, body)
	}
	if rows, err := rig.repo.ListConnectionZDRAcknowledgements(t.Context(), "conn-1", false); err != nil || len(rows) != 0 {
		t.Fatalf("organisation purge left acknowledgements: %+v (%v)", rows, err)
	}
	if _, err := rig.repo.GetConnectionZDR(t.Context(), "conn-1"); err == nil {
		t.Fatal("organisation purge left the capability record")
	}
}

// Acknowledging the current text takes the row below the minimum out of force with its own audit row, which
// names the reason, and leaves exactly one active acknowledgement: the new one, on the current text.
func TestAcknowledgingTheCurrentTextAuditsTheSupersededRowWithItsReason(t *testing.T) {
	rig := newMatrixRig(t, matrixWording(t))
	owner := matrixPrincipal(matrixOrg, matrixOwner)
	rig.connect(matrixConn, matrixOrg, matrixOwner, string(zdr.CapabilityNotSupported), true, 1)
	rig.acknowledgeOldText(matrixOrg, matrixOwner, matrixConn)
	drainAudit(t, rig.repo)

	if status, _ := rig.acknowledge(owner, matrixConn); status != http.StatusCreated {
		t.Fatalf("acknowledging the current text answered %d, want 201", status)
	}
	rows, err := rig.repo.ListConnectionZDRAcknowledgements(t.Context(), matrixConn, false)
	if err != nil || len(rows) != 2 {
		t.Fatalf("ledger = (%+v, %v), want the superseded row and the new one", rows, err)
	}
	if active := activeAcks(t, rig.repo, matrixConn); len(active) != 1 || active[0].TextVersion != "v2" {
		t.Fatalf("active acknowledgements = %+v, want exactly one, on v2", active)
	}
	superseded := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledgement_revoked")
	if len(superseded) != 1 || superseded[0].Metadata["reason"] != store.ZDRRevokeTextVersionOutdated || superseded[0].Metadata["textVersion"] != "v1" {
		t.Fatalf("superseded audit = %+v, want one row for the v1 acknowledgement with reason text_version_superseded", superseded)
	}
}

// An active acknowledgement below the minimum is not in force, so the admin list shows it as superseded and does not
// count it as active. The acknowledgement of the current text is the one counted as active.
func TestTheAdminListLabelsAnAcknowledgementBelowTheMinimumAsSuperseded(t *testing.T) {
	rig := newMatrixRig(t, matrixWording(t))
	owner := matrixPrincipal(matrixOrg, matrixOwner)
	admin := auth.Principal{UserID: "admin-1", OrganizationID: matrixOrg, PrincipalType: "user", Role: "admin"}
	rig.connect(matrixConn, matrixOrg, matrixOwner, string(zdr.CapabilityNotSupported), true, 1)
	rig.acknowledgeOldText(matrixOrg, matrixOwner, matrixConn)
	rig.connect(matrixConnTwo, matrixOrg, matrixOwner, string(zdr.CapabilityNotSupported), true, 1)
	if status, _ := rig.acknowledge(owner, matrixConnTwo); status != http.StatusCreated {
		t.Fatalf("acknowledging the current text on the second connection answered %d, want 201", status)
	}

	status, _, body := rig.call(admin, http.MethodGet, "/api/v1/model-connections/zdr-acknowledgements", "")
	if status != http.StatusOK {
		t.Fatalf("admin listing: %d %v", status, body)
	}
	data := dataOf(body)
	counts := data["counts"].(map[string]any)
	if counts["active"] != float64(1) || counts["superseded"] != float64(1) || counts["revoked"] != float64(0) {
		t.Fatalf("admin counts = %v, want active 1, superseded 1, revoked 0", counts)
	}
	statusByConnection := map[string]any{}
	for _, raw := range data["items"].([]any) {
		item := raw.(map[string]any)
		statusByConnection[item["connectionId"].(string)] = item["status"]
	}
	if statusByConnection[matrixConn] != "superseded" || statusByConnection[matrixConnTwo] != "active" {
		t.Fatalf("admin statuses = %v, want superseded for the v1 row and active for the v2 row", statusByConnection)
	}
}

// The minimum is v1 while v2 is current, so an acknowledgement of v1 stays in force. When the member then acknowledges
// the v2 text they were shown, the v2 acknowledgement is created (201) and supersedes the v1 row, with its audit row.
// Only a repeat of the v2 acknowledgement is a replay.
func TestAcknowledgingTheCurrentTextSupersedesAnOlderAcknowledgementInForce(t *testing.T) {
	rig := newMatrixRig(t, matrixWordingWithMinimum(t, "v1"))
	owner := matrixPrincipal(matrixOrg, matrixOwner)
	rig.connect(matrixConn, matrixOrg, matrixOwner, string(zdr.CapabilityNotSupported), true, 1)
	rig.acknowledgeOldText(matrixOrg, matrixOwner, matrixConn)
	drainAudit(t, rig.repo)
	oldID := activeAcks(t, rig.repo, matrixConn)[0].ID

	if _, data, _ := rig.posture(owner, matrixConn); data["ackState"] != "acknowledged" || data["ackId"] != oldID || data["textVersion"] != "v1" {
		t.Fatalf("posture on a v1 acknowledgement with minimum v1 = %v, want acknowledged on the v1 row", data)
	}

	status, _, body := rig.call(owner, http.MethodPost, ackPath(matrixConn)+"/zdr-acknowledgement",
		ackBody(rig.digest(matrixConn), "v2", "no", "settings"))
	ack, _ := dataOf(body)["acknowledgement"].(map[string]any)
	if status != http.StatusCreated || ack["textVersion"] != "v2" || ack["state"] != "active" || ack["id"] == oldID {
		t.Fatalf("acknowledging the v2 text answered %d with %v, want 201 and a new active v2 acknowledgement", status, ack)
	}
	superseded := auditOfType(drainAudit(t, rig.repo), "subscription_connection.zdr_acknowledgement_revoked")
	if len(superseded) != 1 || superseded[0].Metadata["reason"] != store.ZDRRevokeTextVersionOutdated || superseded[0].Metadata["textVersion"] != "v1" {
		t.Fatalf("superseded audit = %+v, want one row for the v1 acknowledgement with reason text_version_superseded", superseded)
	}
	ledger, err := rig.repo.ListConnectionZDRAcknowledgements(t.Context(), matrixConn, false)
	if err != nil || len(ledger) != 2 {
		t.Fatalf("ledger = (%+v, %v), want the superseded v1 row and the new v2 row", ledger, err)
	}
	if active := activeAcks(t, rig.repo, matrixConn); len(active) != 1 || active[0].TextVersion != "v2" {
		t.Fatalf("active acknowledgements = %+v, want exactly one, on v2", active)
	}

	status, _, body = rig.call(owner, http.MethodPost, ackPath(matrixConn)+"/zdr-acknowledgement",
		ackBody(rig.digest(matrixConn), "v2", "no", "settings"))
	replay, _ := dataOf(body)["acknowledgement"].(map[string]any)
	if status != http.StatusOK || dataOf(body)["created"] != false || replay["id"] != ack["id"] {
		t.Fatalf("repeating the v2 acknowledgement answered %d with %v, want 200 replaying the new row", status, dataOf(body))
	}
	if _, data, _ := rig.posture(owner, matrixConn); data["ackState"] != "acknowledged" || data["textVersion"] != "v2" || data["ackId"] != ack["id"] {
		t.Fatalf("posture after acknowledging v2 = %v, want acknowledged on the v2 row", data)
	}
}

// A row is active only while its text is at or above the minimum: an active row below it is superseded, and a revoked
// row is revoked whatever its text. This is the state the admin list and the summaries carry.
func TestTheLedgerStateOfARowIsActiveSupersededOrRevoked(t *testing.T) {
	revokedAt := time.Now().UTC()
	cases := []struct {
		name string
		ack  store.ConnectionZDRAcknowledgement
		want string
	}{
		{"active at the minimum", store.ConnectionZDRAcknowledgement{TextVersion: "v2"}, "active"},
		{"active above the minimum", store.ConnectionZDRAcknowledgement{TextVersion: "v3"}, "active"},
		{"active below the minimum", store.ConnectionZDRAcknowledgement{TextVersion: "v1"}, "superseded"},
		{"revoked at the minimum", store.ConnectionZDRAcknowledgement{TextVersion: "v2", RevokedAt: &revokedAt}, "revoked"},
		{"revoked below the minimum", store.ConnectionZDRAcknowledgement{TextVersion: "v1", RevokedAt: &revokedAt}, "revoked"},
	}
	for _, tc := range cases {
		if got := zdrAckState(tc.ack, "v2"); got != tc.want {
			t.Errorf("%s: state = %q, want %q", tc.name, got, tc.want)
		}
	}
}

// The counts split the active rows by the minimum by the number of the version, so v10 is at or above v2.
func TestTheAdminCountsSplitActiveRowsByTheMinimumNumerically(t *testing.T) {
	inForce, superseded := zdrActiveSplit(map[string]int{"v1": 2, "v2": 3, "v10": 1}, "v2")
	if inForce != 4 || superseded != 2 {
		t.Fatalf("split = (%d in force, %d superseded), want (4, 2)", inForce, superseded)
	}
}
