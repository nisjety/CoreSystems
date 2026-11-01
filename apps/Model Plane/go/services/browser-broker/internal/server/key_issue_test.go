package server

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// INNH-10 MP-2: issuing a key grant, the revoked-key list and RevokeKey.

// keyReason is the ErrorInfo reason of a refusal ("" when none).
func keyReason(err error) string {
	st, _ := status.FromError(err)
	for _, d := range st.Details() {
		if info, ok := d.(*errdetails.ErrorInfo); ok {
			return info.GetReason()
		}
	}
	return ""
}

func onKeyServer(principal authctx.Principal) (*Server, *grant.MemoryRepository) {
	store := grant.NewMemoryRepository()
	return keyServer(store, principal, true), store
}

func TestKeyIssue_IssuesAKeyGrantWithTheKeyTTLAndBudget(t *testing.T) {
	srv, store := onKeyServer(keyCaller("key-1"))
	before := time.Now()
	resp, err := srv.AcquireGrant(context.Background(), keyAcquireReq())
	if err != nil {
		t.Fatalf("acquire: %v", err)
	}
	g, err := store.GetScoped(context.Background(), resp.GetGrantId(), "org1", "user1")
	if err != nil {
		t.Fatal(err)
	}
	if g.KeyID != "key-1" || g.OwnerID != "user1" || g.Kind != grant.KindSession || g.PolicyMode != grant.PolicyModeAllowlist {
		t.Fatalf("grant = %#v", g)
	}
	if g.Budget.MicroUSD != 500_000 || g.Budget.MaxSteps != 20 || g.Budget.MaxRuntimeS != 300 || g.Budget.Source != "request" {
		t.Fatalf("budget = %#v", g.Budget)
	}
	if life := g.ExpiresAt.Sub(before); life < DefaultKeyGrantTTL-time.Second || life > DefaultKeyGrantTTL+time.Second {
		t.Fatalf("lifetime %v, want %v", life, DefaultKeyGrantTTL)
	}
	if resp.GetBudgetMicroUsd() != 500_000 || len(resp.GetAllowedDomains()) != 1 {
		t.Fatalf("response = %#v", resp)
	}
	if resp.GetKeyId() != "key-1" {
		t.Fatalf("AcquireGrantResponse.key_id = %q", resp.GetKeyId())
	}
	// The key validates its own grant, and the response names the key.
	v, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: resp.GetGrantId()})
	if err != nil || v.GetKeyId() != "key-1" {
		t.Fatalf("validate as the key: %#v, %v", v, err)
	}
	// An ordinary grant carries no key id.
	srv.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"}, nil
	}
	plain, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{OrgId: "org1", SessionKey: "u", Mode: "cloud", AllowedDomains: []string{"example.com"}})
	if err != nil || plain.GetKeyId() != "" {
		t.Fatalf("ordinary grant key_id = %q, %v", plain.GetKeyId(), err)
	}
	if v, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: plain.GetGrantId()}); err != nil || v.GetKeyId() != "" {
		t.Fatalf("ordinary validate key_id = %q, %v", v.GetKeyId(), err)
	}
}

func TestKeyIssue_HostsMustBeASubsetOfTheCeiling(t *testing.T) {
	cases := []struct {
		name    string
		domains []string
		ceiling []string
		code    codes.Code
		reason  string
	}{
		{"exact subset", []string{"example.com", "docs.example.com"}, nil, codes.OK, ""},
		{"case and space normalize", []string{" Example.COM "}, nil, codes.OK, ""},
		{"none", nil, nil, codes.InvalidArgument, ""},
		{"malformed", []string{"http://example.com"}, nil, codes.InvalidArgument, ""},
		{"outside", []string{"example.com", "evil.example"}, nil, codes.PermissionDenied, reasonKeyHostCeiling},
		{"subdomain of a ceiling host", []string{"a.example.com"}, nil, codes.PermissionDenied, reasonKeyHostCeiling},
		{"parent of a ceiling host", []string{"example.com"}, []string{"docs.example.com"}, codes.PermissionDenied, reasonKeyHostCeiling},
		{"no ceiling", []string{"example.com"}, []string{}, codes.PermissionDenied, reasonKeyHostCeiling},
		{"malformed ceiling", []string{"example.com"}, []string{"example.com", "not a host"}, codes.PermissionDenied, reasonKeyHostCeiling},
	}
	for i, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			p := keyCaller("key-1")
			if c.ceiling != nil {
				p.KeyHostCeiling = c.ceiling
			}
			srv, _ := onKeyServer(p)
			req := keyAcquireReq()
			req.SessionKey = fmt.Sprintf("s%d", i)
			req.AllowedDomains = c.domains
			_, err := srv.AcquireGrant(context.Background(), req)
			if status.Code(err) != c.code || keyReason(err) != c.reason {
				t.Fatalf("code %v reason %q (%v), want %v %q", status.Code(err), keyReason(err), err, c.code, c.reason)
			}
		})
	}
}

func TestKeyIssue_BudgetIsMandatoryAndWithinTheCeiling(t *testing.T) {
	type b struct {
		micro         int64
		steps, run    int32
		source        string
		ceiling       *authctx.KeyBudget
		code          codes.Code
		reason, label string
	}
	cases := []b{
		{label: "at the ceiling", micro: 1_000_000, steps: 50, run: 600, source: "request", code: codes.OK},
		{label: "none", code: codes.InvalidArgument},
		{label: "no steps", micro: 1, run: 1, source: "request", code: codes.InvalidArgument},
		{label: "no runtime", micro: 1, steps: 1, source: "request", code: codes.InvalidArgument},
		{label: "no spend", steps: 1, run: 1, source: "request", code: codes.InvalidArgument},
		{label: "negative", micro: -1, steps: 1, run: 1, source: "request", code: codes.InvalidArgument},
		{label: "no source", micro: 1, steps: 1, run: 1, code: codes.InvalidArgument},
		{label: "spend over", micro: 1_000_001, steps: 1, run: 1, source: "request", code: codes.PermissionDenied, reason: reasonKeyBudgetCeiling},
		{label: "steps over", micro: 1, steps: 51, run: 1, source: "request", code: codes.PermissionDenied, reason: reasonKeyBudgetCeiling},
		{label: "runtime over", micro: 1, steps: 1, run: 601, source: "request", code: codes.PermissionDenied, reason: reasonKeyBudgetCeiling},
		{label: "ceiling without steps", micro: 1, steps: 1, run: 1, source: "request",
			ceiling: &authctx.KeyBudget{MicroUSD: 10, MaxRuntimeS: 10}, code: codes.PermissionDenied, reason: reasonKeyBudgetCeiling},
		{label: "no ceiling", micro: 1, steps: 1, run: 1, source: "request",
			ceiling: &authctx.KeyBudget{}, code: codes.PermissionDenied, reason: reasonKeyBudgetCeiling},
	}
	for _, c := range cases {
		t.Run(c.label, func(t *testing.T) {
			p := keyCaller("key-1")
			if c.ceiling != nil {
				p.KeyBudgetCeiling = *c.ceiling
			}
			srv, _ := onKeyServer(p)
			req := keyAcquireReq()
			req.BudgetMicroUsd, req.BudgetMaxSteps, req.BudgetMaxRuntimeS, req.BudgetSource = c.micro, c.steps, c.run, c.source
			_, err := srv.AcquireGrant(context.Background(), req)
			if status.Code(err) != c.code || keyReason(err) != c.reason {
				t.Fatalf("code %v reason %q (%v), want %v %q", status.Code(err), keyReason(err), err, c.code, c.reason)
			}
		})
	}
}

func TestKeyIssue_TTLOnlyShortensAndIsClamped(t *testing.T) {
	for _, c := range []struct {
		cfgTTL  time.Duration
		seconds int32
		want    time.Duration
		code    codes.Code
	}{
		{DefaultKeyGrantTTL, 0, DefaultKeyGrantTTL, codes.OK},
		{DefaultKeyGrantTTL, 60, time.Minute, codes.OK},
		{DefaultKeyGrantTTL, 30, 30 * time.Second, codes.OK},
		{DefaultKeyGrantTTL, 3600, DefaultKeyGrantTTL, codes.OK},
		{DefaultKeyGrantTTL, 29, 0, codes.InvalidArgument},
		{DefaultKeyGrantTTL, -1, 0, codes.InvalidArgument},
		{2 * time.Hour, 0, MaxKeyGrantTTL, codes.OK},
		{time.Second, 0, MinKeyGrantTTL, codes.OK},
	} {
		store := grant.NewMemoryRepository()
		cfg := DefaultConfig()
		cfg.KeyGrantsEnabled = true
		cfg.KeyGrantTTL = c.cfgTTL
		srv := NewServerWithConfig(store, cfg)
		p := keyCaller("key-1")
		srv.principal = func(context.Context) (authctx.Principal, error) { return p, nil }
		req := keyAcquireReq()
		req.TtlSeconds = c.seconds
		before := time.Now()
		resp, err := srv.AcquireGrant(context.Background(), req)
		if status.Code(err) != c.code {
			t.Fatalf("cfg %v ttl %d: %v, want %v", c.cfgTTL, c.seconds, err, c.code)
		}
		if err != nil {
			continue
		}
		life := resp.GetExpiresAt().AsTime().Sub(before)
		if life < c.want-time.Second || life > c.want+time.Second {
			t.Fatalf("cfg %v ttl %d: lifetime %v, want %v", c.cfgTTL, c.seconds, life, c.want)
		}
	}
}

func TestKeyIssue_PerKeyAndOwnerCaps(t *testing.T) {
	srv, _ := onKeyServer(keyCaller("key-1"))
	ctx := context.Background()
	for i := 0; i < DefaultMaxActiveGrantsPerKey; i++ {
		req := keyAcquireReq()
		req.SessionKey = fmt.Sprintf("s%d", i)
		if _, err := srv.AcquireGrant(ctx, req); err != nil {
			t.Fatalf("grant %d: %v", i, err)
		}
	}
	if _, err := srv.AcquireGrant(ctx, keyAcquireReq()); status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("over the per-key cap: %v, want ResourceExhausted", err)
	}
	// Another key of the same owner has its own count.
	srv.principal = func(context.Context) (authctx.Principal, error) { return keyCaller("key-2"), nil }
	if _, err := srv.AcquireGrant(ctx, keyAcquireReq()); err != nil {
		t.Fatalf("second key: %v", err)
	}

	// The owner cap counts key grants with the owner's own.
	store := grant.NewMemoryRepository()
	cfg := DefaultConfig()
	cfg.KeyGrantsEnabled, cfg.MaxActivePerOwner = true, 1
	owner := NewServerWithConfig(store, cfg)
	owner.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"}, nil
	}
	if _, err := owner.AcquireGrant(ctx, &AcquireGrantRequest{OrgId: "org1", SessionKey: "u", Mode: "cloud", AllowedDomains: []string{"example.com"}}); err != nil {
		t.Fatal(err)
	}
	owner.principal = func(context.Context) (authctx.Principal, error) { return keyCaller("key-1"), nil }
	if _, err := owner.AcquireGrant(ctx, keyAcquireReq()); status.Code(err) != codes.ResourceExhausted {
		t.Fatalf("key grant over the owner cap: %v, want ResourceExhausted", err)
	}
}

func TestKeyIssue_ZdrStaysInMemoryAndReadOnlyNeedsZdr(t *testing.T) {
	p := keyCaller("key-1")
	p.ZeroDataRetention = true
	srv, _ := onKeyServer(p)
	resp, err := srv.AcquireGrant(context.Background(), keyAcquireReq())
	if err != nil || !strings.HasPrefix(resp.GetGrantId(), "z_") || !resp.GetZdr() {
		t.Fatalf("zdr key grant: %#v, %v", resp, err)
	}
	// The request can raise ZDR; it cannot lower the token's.
	srv2, _ := onKeyServer(keyCaller("key-1"))
	req := keyAcquireReq()
	req.Zdr = true
	if resp, err := srv2.AcquireGrant(context.Background(), req); err != nil || !resp.GetZdr() {
		t.Fatalf("request-raised zdr: %#v, %v", resp, err)
	}
	req = keyAcquireReq()
	req.SessionKey, req.ReadOnly = "ro", true
	if _, err := srv2.AcquireGrant(context.Background(), req); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("durable read-only key grant: %v, want FailedPrecondition", err)
	}
}

func TestKeyIssue_RevokeKeyKillsGrantsAndRefusesNewOnes(t *testing.T) {
	srv, store := onKeyServer(keyCaller("key-1"))
	ctx := context.Background()
	resp, err := srv.AcquireGrant(ctx, keyAcquireReq())
	if err != nil {
		t.Fatal(err)
	}
	n, err := srv.RevokeKey(ctx, "key-1")
	if err != nil || n != 1 {
		t.Fatalf("RevokeKey = %d, %v", n, err)
	}
	g, _ := store.GetScoped(ctx, resp.GetGrantId(), "org1", "user1")
	if g != nil {
		t.Fatalf("grant still live after RevokeKey: %#v", g)
	}
	if _, err := srv.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: resp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("validate a revoked key's grant: %v", err)
	}
	// A token minted before the revoke still verifies; the list refuses it.
	req := keyAcquireReq()
	req.SessionKey = "after"
	if _, err := srv.AcquireGrant(ctx, req); status.Code(err) != codes.PermissionDenied || keyReason(err) != reasonKeyRevoked {
		t.Fatalf("acquire after revoke: %v (reason %q)", err, keyReason(err))
	}
	if _, err := srv.RevokeKey(ctx, "bad key"); err == nil {
		t.Fatal("RevokeKey accepted an invalid key id")
	}
}

func TestKeyIssue_RevokeKeyRunsWithTheSwitchOff(t *testing.T) {
	store := grant.NewMemoryRepository()
	on := keyServer(store, keyCaller("key-1"), true)
	resp, err := on.AcquireGrant(context.Background(), keyAcquireReq())
	if err != nil {
		t.Fatal(err)
	}
	off := keyServer(store, keyCaller("key-1"), false)
	if n, err := off.RevokeKey(context.Background(), "key-1"); err != nil || n != 1 {
		t.Fatalf("RevokeKey with the switch off = %d, %v", n, err)
	}
	if _, err := store.GetScoped(context.Background(), resp.GetGrantId(), "org1", "user1"); err == nil {
		t.Fatal("grant survived")
	}
}

// The list alone (the sweep missed a grant, e.g. a store error) makes the
// key's grants validate as revoked, for the key and for its owner.
func TestKeyIssue_RevokedListAloneStopsValidation(t *testing.T) {
	srv, _ := onKeyServer(keyCaller("key-1"))
	ctx := context.Background()
	resp, err := srv.AcquireGrant(ctx, keyAcquireReq())
	if err != nil {
		t.Fatal(err)
	}
	srv.revokedKeys.mark("key-1")
	if _, err := srv.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: resp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("key validate: %v", err)
	}
	srv.principal = func(context.Context) (authctx.Principal, error) {
		return authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"}, nil
	}
	if _, err := srv.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: resp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("owner validate: %v", err)
	}
}

// markOnCreate marks the key revoked while CreateSession runs: the revoke
// event lands between the pre-check and the insert, after the sweep.
type markOnCreate struct {
	*grant.MemoryRepository
	srv *Server
}

func (m markOnCreate) CreateSession(ctx context.Context, p grant.CreateSessionParams) (*grant.Grant, error) {
	g, err := m.MemoryRepository.CreateSession(ctx, p)
	m.srv.revokedKeys.mark(p.KeyID)
	return g, err
}

func TestKeyIssue_ARevokeRacingTheInsertLeavesNoGrant(t *testing.T) {
	mem := grant.NewMemoryRepository()
	store := &markOnCreate{MemoryRepository: mem}
	srv := keyServer(store, keyCaller("key-1"), true)
	store.srv = srv
	_, err := srv.AcquireGrant(context.Background(), keyAcquireReq())
	if status.Code(err) != codes.PermissionDenied || keyReason(err) != reasonKeyRevoked {
		t.Fatalf("acquire racing a revoke: %v", err)
	}
	if n, _ := mem.RevokeByKey(context.Background(), "key-1", "probe"); n != 0 {
		t.Fatalf("%d live grants left behind", n)
	}
}

func TestKeyIssue_LogCarriesKeyIDButNotUnderZdr(t *testing.T) {
	buf := captureLog(t)
	srv, _ := onKeyServer(keyCaller("key-log-1"))
	if _, err := srv.AcquireGrant(context.Background(), keyAcquireReq()); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(buf.String(), `"key_id":"key-log-1"`) {
		t.Fatalf("issue line lacks key_id: %s", buf.String())
	}
	buf.Reset()
	p := keyCaller("key-log-2")
	p.ZeroDataRetention = true
	srv, _ = onKeyServer(p)
	if _, err := srv.AcquireGrant(context.Background(), keyAcquireReq()); err != nil {
		t.Fatal(err)
	}
	for _, leak := range []string{"key-log-2", "example.com"} {
		if strings.Contains(buf.String(), leak) {
			t.Fatalf("ZDR issue line carries %q: %s", leak, buf.String())
		}
	}
}

func TestRevokedKeys_ExpireAndFailClosedOnOverflow(t *testing.T) {
	now := time.Date(2026, 11, 1, 12, 0, 0, 0, time.UTC)
	list := newRevokedKeys(func() time.Time { return now })
	list.mark("k1")
	if !list.revoked("k1") || list.revoked("k2") {
		t.Fatal("mark/revoked")
	}
	now = now.Add(RevokedKeyMemory - time.Second)
	if !list.revoked("k1") {
		t.Fatal("forgot k1 too early")
	}
	now = now.Add(2 * time.Second)
	if list.revoked("k1") {
		t.Fatal("k1 remembered past RevokedKeyMemory")
	}
	if RevokedKeyMemory < MaxKeyGrantTTL+2*time.Minute {
		t.Fatalf("RevokedKeyMemory %v must outlive a key grant plus a key token", RevokedKeyMemory)
	}
	// Overflow: fill the list, then one more key closes every key.
	for i := 0; i < maxRevokedKeys; i++ {
		list.until[fmt.Sprintf("f%d", i)] = now.Add(RevokedKeyMemory)
	}
	list.mark("dropped")
	if !list.revoked("dropped") || !list.revoked("never-revoked") {
		t.Fatal("overflow must fail closed")
	}
	now = now.Add(RevokedKeyMemory + time.Second)
	if list.revoked("never-revoked") {
		t.Fatal("overflow closed past its window")
	}
}

func TestKeyIssue_DisableKeyGrantsActsLikeTheSwitchOff(t *testing.T) {
	srv, _ := onKeyServer(keyCaller("key-1"))
	ctx := context.Background()
	resp, err := srv.AcquireGrant(ctx, keyAcquireReq())
	if err != nil {
		t.Fatal(err)
	}
	srv.DisableKeyGrants()
	if srv.KeyGrantsActive() {
		t.Fatal("still active")
	}
	if _, err := srv.ValidateGrant(ctx, &ValidateGrantRequest{GrantId: resp.GetGrantId()}); status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("validate after disable: %v", err)
	}
	req := keyAcquireReq()
	req.SessionKey = "after"
	if _, err := srv.AcquireGrant(ctx, req); status.Code(err) != codes.PermissionDenied || keyReason(err) != reasonKeyGrantsDisabled {
		t.Fatalf("acquire after disable: %v", err)
	}
	// A revoke still runs.
	if n, err := srv.RevokeKey(ctx, "key-1"); err != nil || n != 1 {
		t.Fatalf("RevokeKey after disable = %d, %v", n, err)
	}
}
