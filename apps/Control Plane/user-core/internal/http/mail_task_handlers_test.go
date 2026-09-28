package http

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/mailtask"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// mailTaskServerWithMembership builds a Server wired against a fake
// canonical membership authority that treats (userID, orgID) as a live
// membership for every org in liveOrgs -- and ErrMembershipNotFound for any
// other requested org. This exercises the REAL path a browser request takes
// (X-Org-Id header -> resolveMembershipFromAuthCore), not a shortcut that
// injects "org_id" into the gin context directly (only the service-principal
// delegation path ever populates that key in production).
func mailTaskServerWithMembership(t *testing.T, svc *mailtask.Service, userID string, liveOrgs ...string) *Server {
	t.Helper()
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body struct {
			UserID string `json:"userId"`
			OrgID  string `json:"orgId"`
		}
		_ = json.NewDecoder(r.Body).Decode(&body)
		member := body.UserID == userID
		if member {
			member = false
			for _, org := range liveOrgs {
				if org == body.OrgID {
					member = true
					break
				}
			}
		}
		w.Header().Set("Content-Type", "application/json")
		if !member {
			_, _ = w.Write([]byte(`{"version":"v1","member":false}`))
			return
		}
		_, _ = w.Write([]byte(`{"version":"v1","member":true,"role":"member"}`))
	}))
	t.Cleanup(upstream.Close)
	return &Server{
		mailTaskService:       svc,
		httpClient:            upstream.Client(),
		authMembershipService: upstream.URL,
		authMembershipToken:   "test-membership-token-0123456789ab",
		// handleMembershipResolutionError (reused by resolveActiveOrgForMailTask,
		// same as getSessionContext) revokes a stale local membership projection
		// whenever a requested org turns out not to be live -- wire a no-op here
		// so the non-member-org test below exercises the 403 path itself rather
		// than an unrelated 500 from a nil projection remover.
		removeMembershipProjection: func(context.Context, string, string) error { return nil },
	}
}

func withOrgHeader(c *gin.Context, orgID string) {
	if orgID != "" {
		c.Request.Header.Set("X-Org-Id", orgID)
	}
}

// fakeMailTaskRepository is an in-memory mailtask.Repository, letting these
// handler tests exercise the full alias lifecycle without a live database.
type fakeMailTaskRepository struct {
	rows map[string]mailtask.Address
}

func newMailTaskFakeRepo() *fakeMailTaskRepository {
	return &fakeMailTaskRepository{rows: make(map[string]mailtask.Address)}
}

func (f *fakeMailTaskRepository) ActiveForUser(_ context.Context, orgID, userID string) (*mailtask.Address, error) {
	for _, row := range f.rows {
		if row.OrgID == orgID && row.UserID == userID && row.IsLive() {
			copied := row
			return &copied, nil
		}
	}
	return nil, nil
}

func (f *fakeMailTaskRepository) Insert(_ context.Context, address mailtask.Address) error {
	if _, exists := f.rows[address.Alias]; exists {
		return mailtask.ErrAliasCollision
	}
	f.rows[address.Alias] = address
	return nil
}

func (f *fakeMailTaskRepository) RevokeActive(_ context.Context, orgID, userID string, revokedAt time.Time) error {
	for alias, row := range f.rows {
		if row.OrgID == orgID && row.UserID == userID && row.IsLive() {
			row.RevokedAt = &revokedAt
			f.rows[alias] = row
		}
	}
	return nil
}

func (f *fakeMailTaskRepository) ByAlias(_ context.Context, alias string) (*mailtask.Address, error) {
	row, ok := f.rows[alias]
	if !ok {
		return nil, nil
	}
	copied := row
	return &copied, nil
}

// fakeUserLookup is a MailTaskUserLookup fake so resolveMailTaskAlias is
// testable without a live users.Service/database.
type fakeUserLookup struct {
	usersByID map[string]*users.User
}

func (f *fakeUserLookup) GetUser(_ context.Context, id string) (*users.User, error) {
	return f.usersByID[id], nil
}

func setMailTaskEnv(t *testing.T, domain string) {
	t.Helper()
	original, had := os.LookupEnv("MAIL_TASK_RECEIVING_DOMAIN")
	if domain == "" {
		_ = os.Unsetenv("MAIL_TASK_RECEIVING_DOMAIN")
	} else {
		_ = os.Setenv("MAIL_TASK_RECEIVING_DOMAIN", domain)
	}
	t.Cleanup(func() {
		if had {
			_ = os.Setenv("MAIL_TASK_RECEIVING_DOMAIN", original)
		} else {
			_ = os.Unsetenv("MAIL_TASK_RECEIVING_DOMAIN")
		}
	})
}

func TestMailTaskAddressIsUnavailableUntilDomainIsConfigured(t *testing.T) {
	setMailTaskEnv(t, "")
	s := &Server{mailTaskService: mailtask.NewService(newMailTaskFakeRepo())}

	c, w := newGinCtx("/api/v1/users/me/mail-task-address")
	c.Set("user_id", "user-1")
	withOrgHeader(c, "org-1")
	s.mailTaskAddress(c)

	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 while MAIL_TASK_RECEIVING_DOMAIN is unset", w.Code)
	}
}

func TestMailTaskAddressRequiresAnActiveOrganization(t *testing.T) {
	setMailTaskEnv(t, "tasks.verevon.ai")
	s := &Server{mailTaskService: mailtask.NewService(newMailTaskFakeRepo())}

	c, w := newGinCtx("/api/v1/users/me/mail-task-address")
	c.Set("user_id", "user-1")
	s.mailTaskAddress(c)

	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 without an X-Org-Id header", w.Code)
	}
}

// TestMailTaskAddressRejectsAnOrgTheCallerIsNotAMemberOf is the tenancy-hole
// regression test: X-Org-Id is client-forwarded by the gateway as a hint, not
// proof, so a caller who sets it to an org they do not belong to must be
// rejected, never silently minted an alias scoped to that org.
func TestMailTaskAddressRejectsAnOrgTheCallerIsNotAMemberOf(t *testing.T) {
	setMailTaskEnv(t, "tasks.verevon.ai")
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := mailTaskServerWithMembership(t, svc, "user-1", "org-1")

	c, w := newGinCtx("/api/v1/users/me/mail-task-address")
	c.Set("user_id", "user-1")
	withOrgHeader(c, "org-not-mine")
	s.mailTaskAddress(c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want 403 for a non-member org", w.Code)
	}
}

func TestMailTaskAddressIsIdempotentAcrossRepeatedCalls(t *testing.T) {
	setMailTaskEnv(t, "tasks.verevon.ai")
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := mailTaskServerWithMembership(t, svc, "user-1", "org-1")

	first := requestMailTaskAddress(t, s)
	second := requestMailTaskAddress(t, s)
	if first != second {
		t.Fatalf("mail-task address changed across idempotent calls: %q vs %q", first, second)
	}
	if first == "" || first[len(first)-len("@tasks.verevon.ai"):] != "@tasks.verevon.ai" {
		t.Fatalf("address %q does not end with the configured domain", first)
	}
}

func requestMailTaskAddress(t *testing.T, s *Server) string {
	t.Helper()
	c, w := newGinCtx("/api/v1/users/me/mail-task-address")
	c.Set("user_id", "user-1")
	withOrgHeader(c, "org-1")
	s.mailTaskAddress(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	var body struct {
		Address string `json:"address"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	return body.Address
}

func TestRevokeMailTaskAddressInvalidatesTheOldAddressImmediately(t *testing.T) {
	setMailTaskEnv(t, "tasks.verevon.ai")
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := mailTaskServerWithMembership(t, svc, "user-1", "org-1")

	before := requestMailTaskAddress(t, s)

	c, w := newGinCtx("/api/v1/users/me/mail-task-address")
	c.Request.Method = http.MethodDelete
	c.Set("user_id", "user-1")
	withOrgHeader(c, "org-1")
	s.revokeMailTaskAddress(c)
	// c.Status() records the code on gin's writer immediately, but only
	// flushes it to the underlying ResponseWriter (and so w.Code) once a body
	// byte is written or the request cycle ends -- neither happens when a
	// handler is called directly rather than through router.ServeHTTP. Assert
	// on the writer's recorded status, which is what the real HTTP response
	// will carry.
	if got := c.Writer.Status(); got != http.StatusNoContent {
		t.Fatalf("revoke status = %d, want 204, body = %s", got, w.Body.String())
	}

	after := requestMailTaskAddress(t, s)
	if after == before {
		t.Fatal("address did not change after revoke+regenerate")
	}

	// The alias resolver must reject the old alias, not silently keep it live.
	oldAlias := before[:len(before)-len("@tasks.verevon.ai")]
	if _, err := svc.ResolveAlias(context.Background(), oldAlias); err == nil {
		t.Fatal("old alias still resolves after revoke")
	}
}

func TestResolveMailTaskAliasRequiresAnAlias(t *testing.T) {
	s := &Server{mailTaskService: mailtask.NewService(newMailTaskFakeRepo())}
	c, w := newGinCtx("/api/v1/internal/mail-task/resolve")
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400 without alias", w.Code)
	}
}

func TestResolveMailTaskAliasReturns404IdenticallyForUnknownAndRevoked(t *testing.T) {
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := &Server{
		mailTaskService: svc,
		mailTaskUserLookup: &fakeUserLookup{usersByID: map[string]*users.User{
			"user-1": {ID: "user-1", Email: "user1@example.com", EmailVerified: true},
		}},
	}

	unknown, wUnknown := newGinCtx("/api/v1/internal/mail-task/resolve?alias=does-not-exist")
	s.resolveMailTaskAlias(unknown)
	if wUnknown.Code != http.StatusNotFound {
		t.Fatalf("unknown alias status = %d, want 404", wUnknown.Code)
	}

	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}
	if err := svc.RevokeAlias(context.Background(), "org-1", "user-1"); err != nil {
		t.Fatalf("RevokeAlias() error = %v", err)
	}

	revoked, wRevoked := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias)
	s.resolveMailTaskAlias(revoked)
	if wRevoked.Code != http.StatusNotFound {
		t.Fatalf("revoked alias status = %d, want 404", wRevoked.Code)
	}
	if wUnknown.Body.String() != wRevoked.Body.String() {
		t.Fatalf("unknown vs revoked bodies differ: %q vs %q -- must be identical to avoid alias enumeration",
			wUnknown.Body.String(), wRevoked.Body.String())
	}
}

func TestResolveMailTaskAliasRejectsAnUnverifiedOwnerEmail(t *testing.T) {
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := &Server{
		mailTaskService: svc,
		mailTaskUserLookup: &fakeUserLookup{usersByID: map[string]*users.User{
			"user-1": {ID: "user-1", Email: "user1@example.com", EmailVerified: false},
		}},
	}
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias)
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusNotFound {
		t.Fatalf("status = %d, want 404 for an unverified owner email", w.Code)
	}
}

func TestResolveMailTaskAliasHappyPath(t *testing.T) {
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := &Server{
		mailTaskService: svc,
		mailTaskUserLookup: &fakeUserLookup{usersByID: map[string]*users.User{
			"user-1": {ID: "user-1", Email: "user1@example.com", EmailVerified: true},
		}},
	}
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias)
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	var body struct {
		OrgID         string `json:"org_id"`
		UserID        string `json:"user_id"`
		VerifiedEmail string `json:"verified_email"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if body.OrgID != "org-1" || body.UserID != "user-1" || body.VerifiedEmail != "user1@example.com" {
		t.Fatalf("unexpected resolve body: %+v", body)
	}
}

// fakeMailTaskEffectPolicyResolver is a MailTaskEffectPolicyResolver fake so
// the mail-task-v1 decision-minting path (Gap B, delegated-identity design
// §2.2) is exercisable without a live Postgres-backed spaces.Repository.
type fakeMailTaskEffectPolicyResolver struct {
	policy spaces.EffectPolicy
	err    error
}

func (f *fakeMailTaskEffectPolicyResolver) ResolveOrgEffectPolicy(_ context.Context, _ string) (spaces.EffectPolicy, error) {
	if f.err != nil {
		return spaces.EffectPolicy{}, f.err
	}
	return f.policy, nil
}

// setMailTaskDecisionSigningKey wires a fresh, test-only Ed25519 Control
// Space decision keypair through the exact env vars
// spaces.LoadSigningKeyFromEnv reads, and returns the matching
// spaces.VerificationKey so a test can round-trip a minted decision.
func setMailTaskDecisionSigningKey(t *testing.T) spaces.VerificationKey {
	t.Helper()
	publicKey, privateKey, err := ed25519.GenerateKey(nil)
	if err != nil {
		t.Fatalf("ed25519.GenerateKey: %v", err)
	}
	const keyID = "control-test-mail-task"
	for name, value := range map[string]string{
		"CONTROL_SPACE_DECISION_KEY_ID":             keyID,
		"CONTROL_SPACE_DECISION_PRIVATE_KEY_BASE64": base64.RawURLEncoding.EncodeToString(privateKey.Seed()),
	} {
		original, had := os.LookupEnv(name)
		if err := os.Setenv(name, value); err != nil {
			t.Fatalf("Setenv(%s): %v", name, err)
		}
		t.Cleanup(func() {
			if had {
				_ = os.Setenv(name, original)
			} else {
				_ = os.Unsetenv(name)
			}
		})
	}
	return spaces.VerificationKey{ID: keyID, PublicKey: publicKey}
}

func eligibleMailTaskEffectPolicy(orgID string) spaces.EffectPolicy {
	return spaces.EffectPolicy{
		OrgID:                orgID,
		PrivacyPolicyRef:     "privacy:" + orgID + ":1",
		Purpose:              "mail-task automation",
		LawfulBasis:          "contract",
		PrivacyClass:         "internal",
		RetentionClass:       "standard",
		Residency:            "eu",
		DeletionScope:        "org",
		ZeroDataRetention:    false,
		ThreadCreateEntitled: true,
	}
}

func mailTaskResolveServer(t *testing.T, svc *mailtask.Service, resolver MailTaskEffectPolicyResolver) *Server {
	t.Helper()
	return &Server{
		mailTaskService: svc,
		mailTaskUserLookup: &fakeUserLookup{usersByID: map[string]*users.User{
			"user-1": {ID: "user-1", Email: "user1@example.com", EmailVerified: true},
		}},
		mailTaskEffectPolicy: resolver,
	}
}

// TestResolveMailTaskAliasOmitsThreadDecisionWithoutProviderMessageID pins
// the backward-compatible default: a caller that has not been updated to
// send provider_message_id yet (every caller, until integration-corev2 and
// the gateway BFF are wired) gets EXACTLY today's response shape, even once
// an eligible effect-policy resolver and signing key are both wired.
func TestResolveMailTaskAliasOmitsThreadDecisionWithoutProviderMessageID(t *testing.T) {
	setMailTaskDecisionSigningKey(t)
	svc := mailtask.NewService(newMailTaskFakeRepo())
	resolver := &fakeMailTaskEffectPolicyResolver{policy: eligibleMailTaskEffectPolicy("org-1")}
	s := mailTaskResolveServer(t, svc, resolver)
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias)
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	if body := w.Body.String(); jsonHasField(t, body, "thread_decision") {
		t.Fatalf("thread_decision must be absent without provider_message_id: %s", body)
	}
}

func TestResolveMailTaskAliasOmitsThreadDecisionWhenResolverIsNotWired(t *testing.T) {
	setMailTaskDecisionSigningKey(t)
	svc := mailtask.NewService(newMailTaskFakeRepo())
	s := mailTaskResolveServer(t, svc, nil) // no MailTaskEffectPolicyResolver wired
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias + "&provider_message_id=provider-msg-1")
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	if body := w.Body.String(); jsonHasField(t, body, "thread_decision") {
		t.Fatalf("thread_decision must be absent when no resolver is wired: %s", body)
	}
}

func TestResolveMailTaskAliasIncludesAThreadDecisionWhenEligible(t *testing.T) {
	verifier := setMailTaskDecisionSigningKey(t)
	svc := mailtask.NewService(newMailTaskFakeRepo())
	resolver := &fakeMailTaskEffectPolicyResolver{policy: eligibleMailTaskEffectPolicy("org-1")}
	s := mailTaskResolveServer(t, svc, resolver)
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias + "&provider_message_id=provider-msg-1")
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	var body struct {
		OrgID          string `json:"org_id"`
		UserID         string `json:"user_id"`
		ThreadDecision string `json:"thread_decision"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if body.ThreadDecision == "" {
		t.Fatalf("expected a thread_decision, got none: %s", w.Body.String())
	}
	decision, err := spaces.VerifyMailTaskThreadDecision(verifier, body.ThreadDecision, time.Now().Add(time.Second))
	if err != nil {
		t.Fatalf("minted thread_decision does not verify: %v", err)
	}
	if decision.OrgID != body.OrgID || decision.UserID != body.UserID || decision.AliasID != address.Alias ||
		decision.ProviderMessageID != "provider-msg-1" || decision.SessionKey != spaces.MailTaskSessionKey(body.OrgID, "provider-msg-1") {
		t.Fatalf("thread_decision claims do not match the resolve response: %#v vs body=%+v", decision, body)
	}
}

func TestResolveMailTaskAliasOmitsThreadDecisionForAZDROrg(t *testing.T) {
	setMailTaskDecisionSigningKey(t)
	svc := mailtask.NewService(newMailTaskFakeRepo())
	policy := eligibleMailTaskEffectPolicy("org-1")
	policy.ZeroDataRetention = true
	resolver := &fakeMailTaskEffectPolicyResolver{policy: policy}
	s := mailTaskResolveServer(t, svc, resolver)
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias + "&provider_message_id=provider-msg-1")
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	if body := w.Body.String(); jsonHasField(t, body, "thread_decision") {
		t.Fatalf("thread_decision must be absent for a ZDR org: %s", body)
	}
}

func TestResolveMailTaskAliasOmitsThreadDecisionWithoutThreadCreateEntitlement(t *testing.T) {
	setMailTaskDecisionSigningKey(t)
	svc := mailtask.NewService(newMailTaskFakeRepo())
	policy := eligibleMailTaskEffectPolicy("org-1")
	policy.ThreadCreateEntitled = false
	resolver := &fakeMailTaskEffectPolicyResolver{policy: policy}
	s := mailTaskResolveServer(t, svc, resolver)
	address, err := svc.GenerateAlias(context.Background(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GenerateAlias() error = %v", err)
	}

	c, w := newGinCtx("/api/v1/internal/mail-task/resolve?alias=" + address.Alias + "&provider_message_id=provider-msg-1")
	s.resolveMailTaskAlias(c)
	if w.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", w.Code, w.Body.String())
	}
	if body := w.Body.String(); jsonHasField(t, body, "thread_decision") {
		t.Fatalf("thread_decision must be absent without ThreadCreateEntitled: %s", body)
	}
}

// jsonHasField reports whether the top-level JSON object body has key.
func jsonHasField(t *testing.T, body, key string) bool {
	t.Helper()
	var decoded map[string]json.RawMessage
	if err := json.Unmarshal([]byte(body), &decoded); err != nil {
		t.Fatalf("decode response as JSON object: %v (body=%s)", err, body)
	}
	_, ok := decoded[key]
	return ok
}
