package api

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/attestation"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Owner ruling (security-personal-connections, 2026-09-28): a personal
// Microsoft/Google connection (mail, OneDrive/Drive, calendar, contacts) is
// usable ONLY by the user who connected it — never another org member, an
// org admin/owner, or a service call that names no end user. Org-wide
// connectors (Slack, GitHub, Notion, Shopify, ...) keep the ordinary
// org-level check. This file exercises every route class that reads or
// acts through a *specific* connection, plus the connections list and the
// /actions read/write split (see server.go's executeConnectionAction).

func personalMicrosoftConnection(id, ownerUserID string) store.Connection {
	return store.Connection{
		ID:                   id,
		ProviderKey:          "microsoft",
		ConnectorType:        "microsoft-graph",
		OrganizationID:       "org-1",
		WorkspaceID:          "org-1",
		UserID:               ownerUserID,
		Status:               "active",
		Capabilities:         []string{"profile.read", "mail.read", "mail.send", "teams.messages.read"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}
}

func orgWideSlackConnection(id string) store.Connection {
	return store.Connection{
		ID:                   id,
		ProviderKey:          "slack",
		ConnectorType:        "slack",
		OrganizationID:       "org-1",
		WorkspaceID:          "org-1",
		UserID:               "owner-1",
		Status:               "active",
		Capabilities:         []string{"workspace.read"},
		AccessTokenExpiresAt: time.Now().Add(time.Hour),
	}
}

func otherOrgMemberBearer(userID string) fakeVerifier {
	return fakeVerifier{principal: auth.Principal{
		UserID: userID, OrganizationID: "org-1", Role: "member", PrincipalType: "user",
	}}
}

// TestPersonalConnectionRouteClassesRejectCrossUserBearer pins owner ruling
// (5) across every route class that loads one specific connection and reads
// or acts through it, other than /actions (covered separately below, since
// a write's acting user comes from the attestation, not the bearer).
func TestPersonalConnectionRouteClassesRejectCrossUserBearer(t *testing.T) {
	cases := []struct {
		name   string
		method string
		path   string
		body   string
	}{
		{"get connection", "GET", "/api/v1/connections/conn-private/status", ""},
		{"get connection itself", "GET", "/api/v1/connections/conn-private", ""},
		{"disconnect", "DELETE", "/api/v1/connections/conn-private", ""},
		{"trigger sync", "POST", "/api/v1/connections/conn-private/sync", ""},
		{"inbox sync", "POST", "/api/v1/connections/conn-private/inbox-sync", `{"channel":"email"}`},
		{"inbox history", "POST", "/api/v1/connections/conn-private/inbox-history", ""},
		{"get capabilities", "GET", "/api/v1/connections/conn-private/capabilities", ""},
		{"patch capabilities", "PATCH", "/api/v1/connections/conn-private/capabilities", `{"capabilities":["profile.read"]}`},
		{"get consents", "GET", "/api/v1/connections/conn-private/consents", ""},
		{"post consents", "POST", "/api/v1/connections/conn-private/consents", `{"source":"settings","purpose":"inbox","granted":true}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, service := testOAuthStack(t)
			if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-private", "owner-1")); err != nil {
				t.Fatalf("UpsertConnection error: %v", err)
			}
			app := NewServer(ServerConfig{
				Config: cfg, Repo: repo, OAuth: service,
				Auth: otherOrgMemberBearer("other-member"),
			})

			var body *strings.Reader
			if tc.body != "" {
				body = strings.NewReader(tc.body)
			} else {
				body = strings.NewReader("")
			}
			req := httptest.NewRequest(tc.method, tc.path, body)
			if tc.body != "" {
				req.Header.Set("Content-Type", "application/json")
			}
			req.Header.Set("Authorization", "Bearer other-member-token")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode != fiber.StatusForbidden {
				t.Fatalf("status = %d, want 403", resp.StatusCode)
			}
			if code := readAPIErrorCode(t, resp); code != personalConnectionForbiddenCode {
				t.Fatalf("error code = %q, want %q", code, personalConnectionForbiddenCode)
			}
		})
	}
}

// The same route classes must NOT be blocked by ownership when the bearer
// IS the connecting user — whatever else they return (409 "no sources
// registered", 422 "unsupported channel", ...) is that route's own
// business logic, not the personal-connection gate.
func TestPersonalConnectionRouteClassesAllowOwningBearer(t *testing.T) {
	cases := []struct {
		name   string
		method string
		path   string
		body   string
	}{
		{"get connection", "GET", "/api/v1/connections/conn-private/status", ""},
		{"get connection itself", "GET", "/api/v1/connections/conn-private", ""},
		{"inbox history", "POST", "/api/v1/connections/conn-private/inbox-history", ""},
		{"get capabilities", "GET", "/api/v1/connections/conn-private/capabilities", ""},
		{"get consents", "GET", "/api/v1/connections/conn-private/consents", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg, repo, service := testOAuthStack(t)
			if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-private", "owner-1")); err != nil {
				t.Fatalf("UpsertConnection error: %v", err)
			}
			app := NewServer(ServerConfig{
				Config: cfg, Repo: repo, OAuth: service,
				Auth: otherOrgMemberBearer("owner-1"),
			})

			req := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
			req.Header.Set("Authorization", "Bearer owner-token")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode == fiber.StatusForbidden {
				if code := readAPIErrorCode(t, resp); strings.HasPrefix(code, "personal_connection") {
					t.Fatalf("owning bearer was rejected by the ownership gate: %s", code)
				}
			}
		})
	}
}

// Org-wide connectors are unaffected: any org member (or a service call
// naming no end user at all) may still use them, exactly as before this
// change.
func TestOrgWideConnectionIsNotScopedToItsConnectingUser(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	if _, err := repo.UpsertConnection(t.Context(), orgWideSlackConnection("conn-slack")); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service,
		Auth: otherOrgMemberBearer("other-member"),
	})

	req := httptest.NewRequest("GET", "/api/v1/connections/conn-slack/status", nil)
	req.Header.Set("Authorization", "Bearer other-member-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusOK {
		t.Fatalf("status = %d, want 200 for an org-wide connector read by a different org member", resp.StatusCode)
	}
}

// A service call that names no end user at all — a bare internal
// credential with no X-User-ID, or (once auth-core issues the claim) a
// scoped service token with no acting_user_id — must not read a personal
// connection, per owner ruling (5)'s fail-closed half. Distinct error code
// from "wrong user" so callers can tell the two apart.
func TestPersonalConnectionServiceCallWithNoDelegatedUserIsForbidden(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-private", "owner-1")); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	req := httptest.NewRequest("GET", "/api/v1/connections/conn-private/status", nil)
	req.Header.Set("X-Internal-API-Key", "dev-key")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != personalConnectionNoDelegatedUserCode {
		t.Fatalf("error code = %q, want %q", code, personalConnectionNoDelegatedUserCode)
	}
}

// The auth-core JWT delegation path: a service principal whose scoped
// bearer token names the acting user (Principal.DelegatedUserID, from a
// verified acting_user_id claim — see controlplane.AuthClient.VerifyToken)
// may read that user's own personal connection.
func TestPersonalConnectionServicePrincipalWithDelegatedUserSucceeds(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-private", "owner-1")); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "service:execution-core", OrganizationID: "org-1", PrincipalType: "service",
			DelegatedUserID: "owner-1", Scopes: []string{"integration:read"},
			// Auth Core stamps a jti on every delegated token and the
			// middleware honors each one exactly once (auth/single_use.go).
			TokenID: "api-delegated-owner-read-1", TokenExpiresAt: time.Now().Add(90 * time.Second),
		}},
	})

	req := httptest.NewRequest("GET", "/api/v1/connections/conn-private/status", nil)
	req.Header.Set("Authorization", "Bearer execution-core-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusOK {
		t.Fatalf("status = %d, want 200 for a delegated service call naming the connection's own owner", resp.StatusCode)
	}
}

// The legacy internal-key delegation path (gateway's real internal proxy
// calls: shared secret + X-User-ID — see gateway::domains::ingestions::
// shared::fetch_internal_json) works the same way for a personal
// connection: matching owner succeeds, a different named user is forbidden.
func TestPersonalConnectionLegacyInternalDelegationMatchesOnUserIDHeader(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-private", "owner-1")); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})

	ok := httptest.NewRequest("GET", "/api/v1/connections/conn-private/status", nil)
	ok.Header.Set("X-Internal-API-Key", "dev-key")
	ok.Header.Set("X-User-ID", "owner-1")
	resp, err := app.Test(ok)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusOK {
		t.Fatalf("owner-named internal call status = %d, want 200", resp.StatusCode)
	}

	forbidden := httptest.NewRequest("GET", "/api/v1/connections/conn-private/status", nil)
	forbidden.Header.Set("X-Internal-API-Key", "dev-key")
	forbidden.Header.Set("X-User-ID", "someone-else")
	resp, err = app.Test(forbidden)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("wrong-user-named internal call status = %d, want 403", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != personalConnectionForbiddenCode {
		t.Fatalf("error code = %q, want %q", code, personalConnectionForbiddenCode)
	}
}

// The connector-attach picker's own routes (gateway's browse.rs
// /connections/{id}/browse and /content) are a thin passthrough to this
// exact /actions route with the caller's real bearer session — never a
// service credential. A read operation (list/search — the picker's browse
// call shape) on another org member's personal connection must be rejected
// here, before the provider is ever reached, exactly like every other read
// route class above.
func TestActionsRouteRejectsReadOperationOnAnotherUsersPersonalConnection(t *testing.T) {
	providerCalled := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalled = true
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"value":[]}`))
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-private", "owner-1")); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service,
		Auth:    otherOrgMemberBearer("other-member"),
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-private/actions", strings.NewReader(`{"operation":"mail.messages"}`))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer other-member-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != personalConnectionForbiddenCode {
		t.Fatalf("error code = %q, want %q", code, personalConnectionForbiddenCode)
	}
	if providerCalled {
		t.Fatal("provider was called for a read on another user's personal connection")
	}
}

// The connections LIST never surfaces another user's personal connection —
// omission, not a 403, is the right shape for a list. Org-wide connections
// are unaffected and a caller always sees their own personal ones.
func TestConnectionsListOmitsOtherUsersPersonalConnections(t *testing.T) {
	cfg, repo, service := testOAuthStack(t)
	for _, connection := range []store.Connection{
		personalMicrosoftConnection("conn-owner-1", "owner-1"),
		personalMicrosoftConnection("conn-owner-2", "owner-2"),
		orgWideSlackConnection("conn-slack"),
	} {
		if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
			t.Fatalf("UpsertConnection(%s) error: %v", connection.ID, err)
		}
	}

	t.Run("bearer sees only their own personal connection plus org-wide", func(t *testing.T) {
		app := NewServer(ServerConfig{
			Config: cfg, Repo: repo, OAuth: service,
			Auth: otherOrgMemberBearer("owner-1"),
		})
		req := httptest.NewRequest("GET", "/api/v1/connections", nil)
		req.Header.Set("Authorization", "Bearer owner-1-token")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		ids := decodeConnectionIDs(t, resp)
		if len(ids) != 2 || !ids["conn-owner-1"] || !ids["conn-slack"] || ids["conn-owner-2"] {
			t.Fatalf("connections = %#v, want conn-owner-1 + conn-slack only", ids)
		}
	})

	t.Run("service call with no delegated user sees only org-wide connections", func(t *testing.T) {
		app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
		req := httptest.NewRequest("GET", "/api/v1/connections?organizationId=org-1", nil)
		req.Header.Set("X-Internal-API-Key", "dev-key")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		ids := decodeConnectionIDs(t, resp)
		if len(ids) != 1 || !ids["conn-slack"] {
			t.Fatalf("connections = %#v, want conn-slack only", ids)
		}
	})

	t.Run("a userId query param cannot be used to enumerate someone else's personal connections", func(t *testing.T) {
		app := NewServer(ServerConfig{
			Config: cfg, Repo: repo, OAuth: service,
			Auth: otherOrgMemberBearer("owner-1"),
		})
		req := httptest.NewRequest("GET", "/api/v1/connections?userId=owner-2", nil)
		req.Header.Set("Authorization", "Bearer owner-1-token")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		ids := decodeConnectionIDs(t, resp)
		if ids["conn-owner-2"] {
			t.Fatalf("connections = %#v, must never include another user's personal connection via ?userId=", ids)
		}
	})
}

func decodeConnectionIDs(t *testing.T, resp *http.Response) map[string]bool {
	t.Helper()
	defer resp.Body.Close()
	var decoded struct {
		Data struct {
			Connections []store.Connection `json:"connections"`
		} `json:"data"`
	}
	if err := json.NewDecoder(resp.Body).Decode(&decoded); err != nil {
		t.Fatalf("decode connections list: %v", err)
	}
	ids := map[string]bool{}
	for _, connection := range decoded.Data.Connections {
		ids[connection.ID] = true
	}
	return ids
}

// A write's acting user is the write attestation's cryptographically bound
// ActorID — never the bearer principal's own identity, since the presenter
// for a write is typically a service (conversation-core, execution-core)
// authorizing on a human's behalf. A validly SIGNED attestation whose
// ActorID names someone other than the connection's own owner must still be
// rejected: a valid signature proves the presenter is who it claims to be,
// not that the human it names may use this particular connection.
func TestWriteActionRejectsValidAttestationWhoseActorIsNotTheConnectionOwner(t *testing.T) {
	providerCalled := false
	providerServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		providerCalled = true
		w.WriteHeader(http.StatusOK)
	}))
	defer providerServer.Close()

	cfg, repo, service := testOAuthStack(t)
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	cfg.ProviderWriteAttestationKeysJSON = apiTrustedKeysJSON(t, publicKey)
	cfg.MicrosoftGraphBaseURL = providerServer.URL
	connection := personalMicrosoftConnection("conn-ms", "owner-1")
	if _, err := repo.UpsertConnection(t.Context(), connection); err != nil {
		t.Fatalf("UpsertConnection error: %v", err)
	}
	app := NewServer(ServerConfig{
		Config: cfg, Repo: repo, OAuth: service,
		Auth: fakeVerifier{principal: auth.Principal{
			UserID: "conversation-core", OrganizationID: "org-1", PrincipalType: "service", Scopes: []string{"integration:write"},
		}},
		Actions: actions.NewService(cfg, providerServer.Client()),
	})

	body := map[string]any{"message": map[string]any{"subject": "hello"}}
	idempotencyKey := "conversation:org-1:someone-elses-connection-1"
	binding := attestation.Binding{
		PresenterService: "conversation-core", OrganizationID: "org-1", ConnectionID: "conn-ms",
		ProviderKey: "microsoft", Operation: "mail.send", Body: body, IdempotencyKey: idempotencyKey,
	}
	claims := apiHumanIntentClaims(t, binding)
	// The attestation is otherwise entirely valid and validly signed — the
	// ONLY thing wrong with it is that ActorID does not own conn-ms.
	claims.ActorID = "not-owner-1"
	claims.AuthorizationID = "someone-elses-connection-1"
	claims.ActionID = claims.AuthorizationID
	claims.JWTID = "attestation-someone-elses-connection-1"
	payloadBytes, err := json.Marshal(map[string]any{
		"operation": "mail.send", "writeAttestation": signAPIWriteAttestation(t, privateKey, claims),
		"idempotencyKey": idempotencyKey, "body": body,
	})
	if err != nil {
		t.Fatalf("Marshal payload error: %v", err)
	}

	req := httptest.NewRequest("POST", "/api/v1/connections/conn-ms/actions", strings.NewReader(string(payloadBytes)))
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer conversation-service-token")
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test error: %v", err)
	}
	if resp.StatusCode != fiber.StatusForbidden {
		t.Fatalf("status = %d, want 403", resp.StatusCode)
	}
	if code := readAPIErrorCode(t, resp); code != personalConnectionForbiddenCode {
		t.Fatalf("error code = %q, want %q", code, personalConnectionForbiddenCode)
	}
	if providerCalled {
		t.Fatal("provider was called for a write whose attestation does not own the connection")
	}
	// A rejected-ownership write must never have reached
	// cfg.Repo.ClaimActionReceipt, so this idempotency key is still free to
	// claim — acquired=true here proves nothing was persisted earlier.
	_, acquired, claimErr := repo.ClaimActionReceipt(t.Context(), store.ActionReceipt{
		OrganizationID: "org-1", IdempotencyKey: idempotencyKey, RequestSHA256: "probe",
		ConnectionID: "conn-ms", ProviderKey: "microsoft", Operation: "mail.send",
		AttestationIssuer: "probe-issuer", AuthorizationID: "probe-authorization",
	})
	if claimErr != nil || !acquired {
		t.Fatalf("a rejected-ownership write must never claim an action receipt (acquired=%v err=%v)", acquired, claimErr)
	}
}

// A sync job for a personal Microsoft/Google connection carries that
// connection's own ownership (UserID/ProviderKey snapshotted onto the job at
// creation — see createSyncJob), and scopedSyncJob (GET/cancel/retry) must
// enforce it exactly like every other route that reads through the
// connection itself: its status and checkpoint (source/external IDs from
// the owner's mailbox or drive) are not org-level metadata, and cancelling
// or retrying it is "using" that connection. An org-wide connector's job
// (Notion, Slack, ...) is unaffected.
func TestSyncJobRouteClassesEnforcePersonalConnectionOwnership(t *testing.T) {
	newPersonalJob := func(t *testing.T, repo *store.MemoryRepository, id string) store.SyncJob {
		t.Helper()
		if _, err := repo.UpsertConnection(t.Context(), personalMicrosoftConnection("conn-ms-job", "owner-1")); err != nil {
			t.Fatalf("UpsertConnection error: %v", err)
		}
		job, err := repo.CreateSyncJob(t.Context(), store.SyncJob{
			ID: id, OrganizationID: "org-1", ConnectionID: "conn-ms-job",
			UserID: "owner-1", ProviderKey: "microsoft", Status: "handoff_data_plane",
		})
		if err != nil {
			t.Fatalf("CreateSyncJob error: %v", err)
		}
		return job
	}

	cases := []struct {
		name   string
		method string
		path   func(jobID string) string
		body   string
	}{
		{"get", "GET", func(id string) string { return "/api/v1/sync-jobs/" + id }, ""},
		{"cancel", "POST", func(id string) string { return "/api/v1/sync-jobs/" + id + "/cancel" }, ""},
		{"retry", "POST", func(id string) string { return "/api/v1/sync-jobs/" + id + "/retry" }, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name+" rejects a cross-user bearer", func(t *testing.T) {
			cfg, repo, service := testOAuthStack(t)
			job := newPersonalJob(t, repo, "sync-"+tc.name+"-cross-user")
			app := NewServer(ServerConfig{
				Config: cfg, Repo: repo, OAuth: service,
				Auth: otherOrgMemberBearer("other-member"),
			})
			req := httptest.NewRequest(tc.method, tc.path(job.ID), strings.NewReader(tc.body))
			req.Header.Set("Authorization", "Bearer other-member-token")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode != fiber.StatusForbidden {
				t.Fatalf("status = %d, want 403", resp.StatusCode)
			}
			if code := readAPIErrorCode(t, resp); code != personalConnectionForbiddenCode {
				t.Fatalf("error code = %q, want %q", code, personalConnectionForbiddenCode)
			}
		})

		t.Run(tc.name+" allows the owning bearer", func(t *testing.T) {
			cfg, repo, service := testOAuthStack(t)
			job := newPersonalJob(t, repo, "sync-"+tc.name+"-owner")
			app := NewServer(ServerConfig{
				Config: cfg, Repo: repo, OAuth: service,
				Auth: otherOrgMemberBearer("owner-1"),
			})
			req := httptest.NewRequest(tc.method, tc.path(job.ID), strings.NewReader(tc.body))
			req.Header.Set("Authorization", "Bearer owner-token")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode == fiber.StatusForbidden {
				if code := readAPIErrorCode(t, resp); strings.HasPrefix(code, "personal_connection") {
					t.Fatalf("owning bearer was rejected by the ownership gate: %s", code)
				}
			}
		})

		t.Run(tc.name+" rejects a service call with no delegated user", func(t *testing.T) {
			cfg, repo, service := testOAuthStack(t)
			job := newPersonalJob(t, repo, "sync-"+tc.name+"-no-delegate")
			app := NewServer(ServerConfig{Config: cfg, Repo: repo, OAuth: service})
			req := httptest.NewRequest(tc.method, tc.path(job.ID), strings.NewReader(tc.body))
			req.Header.Set("X-Internal-API-Key", "dev-key")
			resp, err := app.Test(req)
			if err != nil {
				t.Fatalf("app.Test error: %v", err)
			}
			if resp.StatusCode != fiber.StatusForbidden {
				t.Fatalf("status = %d, want 403", resp.StatusCode)
			}
			if code := readAPIErrorCode(t, resp); code != personalConnectionNoDelegatedUserCode {
				t.Fatalf("error code = %q, want %q", code, personalConnectionNoDelegatedUserCode)
			}
		})
	}

	t.Run("an org-wide connector's sync job is unaffected", func(t *testing.T) {
		cfg, repo, service := testOAuthStack(t)
		if _, err := repo.UpsertConnection(t.Context(), orgWideSlackConnection("conn-slack-job")); err != nil {
			t.Fatalf("UpsertConnection error: %v", err)
		}
		job, err := repo.CreateSyncJob(t.Context(), store.SyncJob{
			ID: "sync-org-wide", OrganizationID: "org-1", ConnectionID: "conn-slack-job",
			UserID: "owner-1", ProviderKey: "slack", Status: "handoff_data_plane",
		})
		if err != nil {
			t.Fatalf("CreateSyncJob error: %v", err)
		}
		app := NewServer(ServerConfig{
			Config: cfg, Repo: repo, OAuth: service,
			Auth: otherOrgMemberBearer("other-member"),
		})
		req := httptest.NewRequest("GET", "/api/v1/sync-jobs/"+job.ID, nil)
		req.Header.Set("Authorization", "Bearer other-member-token")
		resp, err := app.Test(req)
		if err != nil {
			t.Fatalf("app.Test error: %v", err)
		}
		if resp.StatusCode != fiber.StatusOK {
			t.Fatalf("status = %d, want 200 for an org-wide connector's sync job read by a different org member", resp.StatusCode)
		}
	})
}
