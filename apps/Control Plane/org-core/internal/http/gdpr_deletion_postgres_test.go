package http

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	nethttp "net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
)

// deletionFanoutRecorder is a fake orgcore.SharedPublisher that records every
// PublishPlain call so tests can assert on the exact cross-plane Flow C
// events (velion.org.deletion.pending/.cancelled) a request produced,
// without standing up real NATS. Mirrors the existing
// sharedPlanChangeTestPublisher fake in internal/org/plan_change_postgres_test.go.
type deletionFanoutRecorder struct {
	mu    sync.Mutex
	calls []struct {
		subject string
		payload map[string]any
	}
}

func (r *deletionFanoutRecorder) PublishOrgCreated(context.Context, string, string, string, string, map[string]any) {
}
func (r *deletionFanoutRecorder) PublishOrgUpdated(context.Context, string, map[string]any) {}
func (r *deletionFanoutRecorder) PublishOrgDeleted(context.Context, string, string)          {}
func (r *deletionFanoutRecorder) PublishPlanChanged(context.Context, string, string, string, string, string, string, int64) error {
	return nil
}
func (r *deletionFanoutRecorder) PublishMemberAdded(context.Context, string, string, string, string, string) {
}
func (r *deletionFanoutRecorder) PublishMemberRemoved(context.Context, string, string) {}

func (r *deletionFanoutRecorder) PublishPlain(subject string, payload map[string]any) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.calls = append(r.calls, struct {
		subject string
		payload map[string]any
	}{subject, payload})
}

// alwaysSucceedsAuditPublisher is a no-op orgcore.AuditPublisher used only to
// let TestControlLifecycleOrgDeletionSelfServiceHTTPFlow drain (never
// actually deliver anywhere) its own GDPR audit-outbox rows before it exits,
// keeping the shared disposable Postgres clean for other packages' tests.
type alwaysSucceedsAuditPublisher struct{}

func (alwaysSucceedsAuditPublisher) PublishAudit(context.Context, string, string, map[string]any) error {
	return nil
}

func (r *deletionFanoutRecorder) lastPayload(subject string) (map[string]any, bool) {
	r.mu.Lock()
	defer r.mu.Unlock()
	for i := len(r.calls) - 1; i >= 0; i-- {
		if r.calls[i].subject == subject {
			return r.calls[i].payload, true
		}
	}
	return nil, false
}

// TestControlLifecycleOrgDeletionSelfServiceHTTPFlow exercises the full Flow
// C org-deletion HTTP surface end to end against a disposable Postgres:
// soft-delete's confirm/org_name gate, ledger creation +
// velion.org.deletion.pending on success, restore's 409-when-not-pending /
// 200-when-pending + velion.org.deletion.cancelled, and the
// mark-exported/acknowledge/status self-or-admin gate (an active member acts
// on their own row; a caller who is not a member of this org at all is
// rejected; an owner sees every member's ledger row via status, a plain
// member sees only their own).
func TestControlLifecycleOrgDeletionSelfServiceHTTPFlow(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(
		ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	t.Setenv(serviceCredentialEnv, orgServiceTestRegistry("org:erase:self", "org:deletion:self"))

	repo := orgcore.NewRepository(db)
	fanout := &deletionFanoutRecorder{}
	orgService := orgcore.NewService(repo, nil)
	orgService.SetSharedPublisher(fanout)
	// SoftDelete's newGDPRAuditEvent/EnqueueGDPRAuditEvent machinery writes
	// unconditionally to the SHARED organization_gdpr_audit_outbox table
	// (never scoped to this test's org — see gdpr_audit_outbox_test.go), and
	// this suite's own package never flushes it. internal/org's
	// TestControlLifecycleGDPRAuditOutboxIsAtomicRetryableAndBounded (a
	// different package, but the same disposable Postgres in one `go test
	// ./...` run) asserts an EXACT pending-row count, so any row this test
	// leaves behind would inflate that count and fail it. Drain everything
	// this test writes at the end (see the deferred flush below) so this
	// suite never leaks state into another package's test.
	orgService.SetAuditPublisher(alwaysSucceedsAuditPublisher{})
	server := NewServer(0, orgService, nil, "", "")
	defer func() {
		// Drain regardless of pass/fail so a t.Fatalf mid-test still cleans
		// up. Loop: SoftDeleteOrganization's atomic-audit-intent write means
		// a single flush might race a not-yet-committed row; a handful of
		// bounded retries is enough to catch up.
		for i := 0; i < 5; i++ {
			result, err := orgService.FlushGDPRAuditOutbox(context.Background(), 100)
			if err != nil || (result.Published == 0 && result.DeadLettered == 0) {
				return
			}
		}
	}()

	suffix := fmt.Sprintf("%d", time.Now().UnixNano())
	orgID := "org-deletion-http-" + suffix
	orgName := "Deletion HTTP Org " + suffix
	ownerID := "owner-deletion-http-" + suffix
	memberID := "member-deletion-http-" + suffix
	strangerID := "stranger-deletion-http-" + suffix // never joins orgID at all

	if err := repo.ProvisionOrganizationWithOwner(ctx, orgcore.Organization{
		ID: orgID, Name: orgName, Plan: "free", Status: "active",
	}, ownerID); err != nil {
		t.Fatalf("provision organization: %v", err)
	}
	if err := repo.AddOrganizationMember(ctx, orgID, memberID, "member"); err != nil {
		t.Fatalf("seed member: %v", err)
	}

	nonceSeq := 0
	nextNonce := func() string {
		nonceSeq++
		return fmt.Sprintf("nonce-%032d", nonceSeq)
	}

	// doAs signs and sends a fully-verified :self service-delegation request
	// (the shape the real gateway sends) as the given end-user, matching
	// signOrgDelegation's mechanics (service_auth_test.go) but parameterized
	// per-call so different scenarios can act as different users.
	doAs := func(method, path, body, userID, userRole string) *httptest.ResponseRecorder {
		t.Helper()
		req := httptest.NewRequest(method, path, bytes.NewBufferString(body))
		if body != "" {
			req.Header.Set("Content-Type", "application/json")
		}
		now := time.Now()
		nonce := nextNonce()
		digest := serviceDelegationBodyDigest([]byte(body))
		claims := serviceDelegationClaims{
			Principal:  "velion-gateway",
			Audience:   "org-core",
			Timestamp:  now.UTC().Format(time.RFC3339),
			Nonce:      nonce,
			Method:     req.Method,
			URI:        req.URL.RequestURI(),
			UserID:     userID,
			OrgID:      orgID,
			UserRole:   userRole,
			BodySHA256: digest,
		}
		req.Header.Set("X-Service-Id", claims.Principal)
		req.Header.Set("X-Service-Token", orgServiceTestToken)
		req.Header.Set("X-User-Id", claims.UserID)
		req.Header.Set("X-Org-Id", claims.OrgID)
		req.Header.Set("X-User-Role", claims.UserRole)
		req.Header.Set("X-Delegation-Version", "v3")
		req.Header.Set("X-Delegation-Timestamp", claims.Timestamp)
		req.Header.Set("X-Delegation-Nonce", claims.Nonce)
		req.Header.Set("X-Delegation-Body-SHA256", digest)
		req.Header.Set("X-Delegation-Signature", serviceDelegationSignature(orgServiceTestToken, claims))
		resp := httptest.NewRecorder()
		server.router.ServeHTTP(resp, req)
		return resp
	}

	softDeletePath := "/orgs/" + orgID + "/gdpr/soft-delete"
	restorePath := "/orgs/" + orgID + "/gdpr/restore"
	statusPath := "/orgs/" + orgID + "/gdpr/deletion/status"
	markExportedPath := "/orgs/" + orgID + "/gdpr/deletion/mark-exported"
	acknowledgePath := "/orgs/" + orgID + "/gdpr/deletion/acknowledge"

	// --- restore before any deletion: 409, not pending. ---
	if resp := doAs(nethttp.MethodPost, restorePath, "", ownerID, "owner"); resp.Code != nethttp.StatusConflict {
		t.Fatalf("restore-before-delete status=%d body=%s", resp.Code, resp.Body.String())
	}

	// --- soft-delete: missing confirm is rejected. ---
	if resp := doAs(nethttp.MethodDelete, softDeletePath, `{"confirm":false,"org_name":"`+orgName+`"}`, ownerID, "owner"); resp.Code != nethttp.StatusBadRequest {
		t.Fatalf("soft-delete missing confirm status=%d body=%s", resp.Code, resp.Body.String())
	}

	// --- soft-delete: wrong org_name is rejected, even with confirm:true. ---
	if resp := doAs(nethttp.MethodDelete, softDeletePath, `{"confirm":true,"org_name":"Not The Real Name"}`, ownerID, "owner"); resp.Code != nethttp.StatusBadRequest {
		t.Fatalf("soft-delete wrong org_name status=%d body=%s", resp.Code, resp.Body.String())
	}
	if pending, name, deadline := readStatus(t, doAs(nethttp.MethodGet, statusPath, "", ownerID, "owner")); pending || name == "" || deadline != nil {
		t.Fatalf("org must still be active after a rejected soft-delete: pending=%v name=%q deadline=%v", pending, name, deadline)
	}

	// --- soft-delete: the real, correct confirmation succeeds. ---
	if resp := doAs(nethttp.MethodDelete, softDeletePath, `{"confirm":true,"org_name":"`+orgName+`"}`, ownerID, "owner"); resp.Code != nethttp.StatusOK {
		t.Fatalf("soft-delete status=%d body=%s", resp.Code, resp.Body.String())
	}

	ledger, err := repo.ListDeletionLedger(ctx, orgID)
	if err != nil {
		t.Fatalf("list deletion ledger: %v", err)
	}
	ledgerUserIDs := map[string]bool{}
	for _, entry := range ledger {
		ledgerUserIDs[entry.UserID] = true
	}
	if !ledgerUserIDs[ownerID] || !ledgerUserIDs[memberID] {
		t.Fatalf("deletion ledger missing active members: %+v", ledger)
	}

	pendingPayload, ok := fanout.lastPayload("velion.org.deletion.pending")
	if !ok {
		t.Fatal("velion.org.deletion.pending was not published")
	}
	if pendingPayload["org_id"] != orgID || pendingPayload["org_name"] != orgName || pendingPayload["requested_by"] != ownerID {
		t.Fatalf("velion.org.deletion.pending payload = %+v", pendingPayload)
	}
	deadlineStr, _ := pendingPayload["deadline"].(string)
	if _, err := time.Parse(time.RFC3339, deadlineStr); err != nil {
		t.Fatalf("velion.org.deletion.pending deadline %q is not RFC3339: %v", deadlineStr, err)
	}
	memberIDs, _ := pendingPayload["member_user_ids"].([]string)
	memberSet := map[string]bool{}
	for _, id := range memberIDs {
		memberSet[id] = true
	}
	if !memberSet[ownerID] || !memberSet[memberID] {
		t.Fatalf("velion.org.deletion.pending member_user_ids = %v, want to include %q and %q", memberIDs, ownerID, memberID)
	}

	// --- retrying soft-delete on an already-deleted org fails, and must not
	// double-publish a second pending event. ---
	pendingCallsBefore := len(fanout.calls)
	if resp := doAs(nethttp.MethodDelete, softDeletePath, `{"confirm":true,"org_name":"`+orgName+`"}`, ownerID, "owner"); resp.Code == nethttp.StatusOK {
		t.Fatalf("soft-delete retry on already-deleted org unexpectedly succeeded: %s", resp.Body.String())
	}
	if len(fanout.calls) != pendingCallsBefore {
		t.Fatalf("soft-delete retry published extra event(s): before=%d after=%d", pendingCallsBefore, len(fanout.calls))
	}

	// --- status: the plain member sees only their own checkpoint, not the
	// full member list. ---
	memberResp := doAs(nethttp.MethodGet, statusPath, "", memberID, "member")
	if memberResp.Code != nethttp.StatusOK {
		t.Fatalf("member status status=%d body=%s", memberResp.Code, memberResp.Body.String())
	}
	if strings.Contains(memberResp.Body.String(), `"members"`) {
		t.Fatalf("plain member must not receive the all-members ledger array: %s", memberResp.Body.String())
	}
	if !strings.Contains(memberResp.Body.String(), `"member_status"`) {
		t.Fatalf("member status missing member_status: %s", memberResp.Body.String())
	}

	// --- status: the owner sees every member's ledger row. ---
	ownerResp := doAs(nethttp.MethodGet, statusPath, "", ownerID, "owner")
	if ownerResp.Code != nethttp.StatusOK {
		t.Fatalf("owner status status=%d body=%s", ownerResp.Code, ownerResp.Body.String())
	}
	if !strings.Contains(ownerResp.Body.String(), `"members"`) || !strings.Contains(ownerResp.Body.String(), memberID) {
		t.Fatalf("owner status missing the all-members ledger array: %s", ownerResp.Body.String())
	}

	// --- mark-exported / acknowledge: a caller who is not a member of this
	// org at all is rejected — self-or-admin, never "anyone". ---
	if resp := doAs(nethttp.MethodPost, markExportedPath, "", strangerID, "member"); resp.Code != nethttp.StatusForbidden {
		t.Fatalf("mark-exported by non-member status=%d body=%s", resp.Code, resp.Body.String())
	}
	if resp := doAs(nethttp.MethodPost, acknowledgePath, "", strangerID, "member"); resp.Code != nethttp.StatusForbidden {
		t.Fatalf("acknowledge by non-member status=%d body=%s", resp.Code, resp.Body.String())
	}

	// --- mark-exported / acknowledge: the member acting on their own row
	// succeeds. ---
	if resp := doAs(nethttp.MethodPost, markExportedPath, "", memberID, "member"); resp.Code != nethttp.StatusOK {
		t.Fatalf("mark-exported by self status=%d body=%s", resp.Code, resp.Body.String())
	}
	if resp := doAs(nethttp.MethodPost, acknowledgePath, "", memberID, "member"); resp.Code != nethttp.StatusOK {
		t.Fatalf("acknowledge by self status=%d body=%s", resp.Code, resp.Body.String())
	}

	entries, err := repo.ListDeletionLedger(ctx, orgID)
	if err != nil {
		t.Fatalf("list deletion ledger after marks: %v", err)
	}
	var memberEntry *orgcore.DeletionLedgerEntry
	for i := range entries {
		if entries[i].UserID == memberID {
			memberEntry = &entries[i]
		}
	}
	if memberEntry == nil || memberEntry.ExportedAt == nil || memberEntry.AcknowledgedAt == nil {
		t.Fatalf("member ledger row after marks = %+v", memberEntry)
	}

	// --- restore: reverses the pending deletion, clears the ledger, and
	// publishes velion.org.deletion.cancelled. ---
	if resp := doAs(nethttp.MethodPost, restorePath, "", ownerID, "owner"); resp.Code != nethttp.StatusOK {
		t.Fatalf("restore status=%d body=%s", resp.Code, resp.Body.String())
	}
	cancelledPayload, ok := fanout.lastPayload("velion.org.deletion.cancelled")
	if !ok {
		t.Fatal("velion.org.deletion.cancelled was not published")
	}
	if cancelledPayload["org_id"] != orgID || cancelledPayload["cancelled_by"] != ownerID {
		t.Fatalf("velion.org.deletion.cancelled payload = %+v", cancelledPayload)
	}
	remainingLedger, err := repo.ListDeletionLedger(ctx, orgID)
	if err != nil {
		t.Fatalf("list deletion ledger after restore: %v", err)
	}
	if len(remainingLedger) != 0 {
		t.Fatalf("deletion ledger not cleared after restore: %+v", remainingLedger)
	}
	if pending, _, _ := readStatus(t, doAs(nethttp.MethodGet, statusPath, "", ownerID, "owner")); pending {
		t.Fatal("organization still reports pending deletion after restore")
	}

	// --- restore again: 409, no longer pending (idempotent negative). ---
	if resp := doAs(nethttp.MethodPost, restorePath, "", ownerID, "owner"); resp.Code != nethttp.StatusConflict {
		t.Fatalf("restore-after-restore status=%d body=%s", resp.Code, resp.Body.String())
	}
}

// readStatus extracts the fields TestControlLifecycleOrgDeletionSelfServiceHTTPFlow
// asserts on from a GET .../deletion/status response body.
func readStatus(t *testing.T, resp *httptest.ResponseRecorder) (pending bool, orgName string, deadline *string) {
	t.Helper()
	if resp.Code != nethttp.StatusOK {
		t.Fatalf("deletion status request failed: status=%d body=%s", resp.Code, resp.Body.String())
	}
	var payload struct {
		Pending  bool    `json:"pending"`
		OrgName  string  `json:"org_name"`
		Deadline *string `json:"deadline"`
	}
	if err := json.Unmarshal(resp.Body.Bytes(), &payload); err != nil {
		t.Fatalf("decode deletion status: %v body=%s", err, resp.Body.String())
	}
	return payload.Pending, payload.OrgName, payload.Deadline
}
