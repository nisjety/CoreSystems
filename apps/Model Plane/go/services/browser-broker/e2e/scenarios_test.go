//go:build e2e

package e2e

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/browser-broker/e2e/idp"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	grpc_health_v1 "google.golang.org/grpc/health/grpc_health_v1"
)

// TestBrokerE2E runs the scenarios in order against one broker container. They
// share state on purpose: (a) leaves a non-ZDR grant that (e) validates again
// after the database outage, and every restart is a real docker restart.
func TestBrokerE2E(t *testing.T) {
	e := newEnv(t)
	var shared struct {
		sessionA string // scenario a's non-ZDR session grant, owned by user-a
	}
	t.Run("a_restart_survival", func(t *testing.T) { scenarioA(t, e, &shared.sessionA) })
	t.Run("b_zdr_is_memory_only", func(t *testing.T) { scenarioB(t, e) })
	t.Run("c_derive", func(t *testing.T) { scenarioC(t, e) })
	t.Run("d_budget_claim", func(t *testing.T) { scenarioD(t, e) })
	t.Run("f_auth", func(t *testing.T) { scenarioF(t, e, shared.sessionA) })
	t.Run("g_sensitive_approval", func(t *testing.T) { scenarioG(t, e) })
	t.Run("e_postgres_down", func(t *testing.T) { scenarioE(t, e, shared.sessionA) })
}

// (a) A non-ZDR session grant survives a broker restart, and its row is in Postgres.
func scenarioA(t *testing.T, e *env, sessionA *string) {
	s := &sc{t: t, name: "a_restart_survival"}
	defer s.done()
	userA := e.person(t, "user-a")

	s.ok(e.httpStatus("/healthz") == 200, "/healthz is 200")
	s.ok(e.httpStatus("/readyz") == 200, "/readyz is 200 with the database up")
	h, err := e.health(userA)
	s.code(err, codes.OK, "Health with a person token")
	s.ok(err == nil && h.GetStatus() == "ok", "Health answers ok")

	before := e.count(t, "")
	g, err := e.acquire(userA, e.session("e2e-a-session", parentHosts))
	s.code(err, codes.OK, "AcquireGrant (non-ZDR session)")
	s.must(err == nil && g.GetGrantId() != "", "a grant id came back")
	*sessionA = g.GetGrantId()
	s.ok(!hasPrefix(g.GetGrantId(), "z_") && !g.GetZdr(), "a non-ZDR grant has no z_ id and zdr=false")
	s.ok(g.GetKind() == "session", "kind is session")
	s.ok(e.count(t, "id = '"+g.GetGrantId()+"'") == 1, "the grant's row is in browser_grants")
	s.ok(e.count(t, "") == before+1, "exactly one row was added (%d -> %d)", before, e.count(t, ""))

	v1, err := e.validate(userA, g.GetGrantId())
	s.code(err, codes.OK, "ValidateGrant before the restart")
	s.must(err == nil, "validate before the restart answered")
	rowsBefore := e.count(t, "")

	e.restartBroker(t)

	s.ok(e.httpStatus("/readyz") == 200, "/readyz is 200 after the restart")
	v2, err := e.validate(userA, g.GetGrantId())
	s.code(err, codes.OK, "ValidateGrant after the restart")
	s.must(err == nil, "validate after the restart answered")
	s.ok(v2.GetActive() && v2.GetGrantId() == g.GetGrantId(), "the same grant id is active after the restart")
	s.ok(v2.GetOwnerId() == "user-a" && v2.GetKind() == "session" && !v2.GetZdr(), "owner, kind and zdr survived")
	s.ok(slices.Equal(v2.GetAllowedDomains(), v1.GetAllowedDomains()) && len(v2.GetAllowedDomains()) == len(parentHosts), "the host list survived unchanged")
	s.ok(v2.GetExpiresAt().AsTime().Equal(g.GetExpiresAt().AsTime()), "the expiry survived unchanged")
	s.ok(e.count(t, "") == rowsBefore, "the restart changed no row count (%d)", rowsBefore)
}

// (b) A ZDR session grant never reaches Postgres and is gone after a restart.
func scenarioB(t *testing.T, e *env) {
	s := &sc{t: t, name: "b_zdr_is_memory_only"}
	defer s.done()
	userA := e.person(t, "user-a")
	before := e.count(t, "")

	req := e.session("e2e-b-zdr", parentHosts)
	req.Zdr = true
	g, err := e.acquire(userA, req)
	s.code(err, codes.OK, "AcquireGrant with zdr=true")
	s.must(err == nil, "ZDR grant issued")
	s.ok(hasPrefix(g.GetGrantId(), "z_") && g.GetZdr(), "a ZDR grant has a z_ id and zdr=true")
	s.ok(e.count(t, "") == before, "creation wrote no row (%d -> %d)", before, e.count(t, ""))
	s.ok(e.count(t, "id = '"+g.GetGrantId()+"' OR parent_grant_id = '"+g.GetGrantId()+"'") == 0, "no row carries the ZDR id")

	v, err := e.validate(userA, g.GetGrantId())
	s.code(err, codes.OK, "ValidateGrant of the ZDR grant before the restart")
	s.ok(err == nil && v.GetActive() && v.GetZdr(), "the ZDR grant is active and zdr before the restart")

	// A token that carries zdr=true raises the floor with no request flag.
	g2, err := e.acquire(e.personZDR(t, "user-a"), e.session("e2e-b-zdr-token", parentHosts))
	s.code(err, codes.OK, "AcquireGrant with a zdr token and no request flag")
	s.ok(err == nil && hasPrefix(g2.GetGrantId(), "z_") && g2.GetZdr(), "the token's zdr claim made the grant ZDR")
	s.ok(e.count(t, "") == before, "the token-ZDR grant wrote no row either")

	e.restartBroker(t)

	s.ok(e.count(t, "") == before, "row count unchanged after the restart")
	v, err = e.validate(userA, g.GetGrantId())
	s.code(err, codes.NotFound, "ValidateGrant of the ZDR grant after the restart (memory-only by design, X-2)")
	s.ok(err != nil && !v.GetActive(), "the ZDR grant is not active after the restart")
	_, err = e.validate(userA, g2.GetGrantId())
	s.code(err, codes.NotFound, "the token-ZDR grant after the restart")
}

// (c) DeriveRunGrant: narrowing, expiry cap, idempotency, the per-parent cap,
// ZDR inheritance and revocation of the whole subtree (before and after a restart).
func scenarioC(t *testing.T, e *env) {
	s := &sc{t: t, name: "c_derive"}
	defer s.done()
	userA := e.person(t, "user-a")
	gateway := e.service(t, "model-gateway", scopeRead, scopeWrite)

	p1, err := e.acquire(userA, e.session("e2e-c-p1", parentHosts))
	s.code(err, codes.OK, "parent session P1")
	s.must(err == nil, "P1 issued")
	parent := p1.GetGrantId()

	// narrowing + expiry cap
	req := e.runReq(parent, "run-1", []string{"docs.example.com"})
	req.TtlSeconds = 3600
	r1, err := e.derive(userA, req)
	s.code(err, codes.OK, "derive run-1 (a subdomain of a parent host, ttl above the parent's remaining life)")
	s.must(err == nil, "run-1 issued")
	s.ok(r1.GetKind() == "run" && r1.GetParentGrantId() == parent && !r1.GetZdr(), "run-1 is a non-ZDR run grant bound to P1")
	s.ok(slices.Equal(r1.GetAllowedDomains(), []string{"docs.example.com"}), "run-1 hosts are exactly the narrowed subset")
	s.ok(!r1.GetExpiresAt().AsTime().After(p1.GetExpiresAt().AsTime()), "run-1 expiry <= parent expiry")
	s.ok(r1.GetExpiresAt().AsTime().After(time.Now()), "run-1 is not already expired")
	s.ok(e.count(t, "id = '"+r1.GetGrantId()+"' AND parent_grant_id = '"+parent+"' AND kind = 'run'") == 1, "run-1 is a durable row under P1")

	short := e.runReq(parent, "run-short", nil)
	short.TtlSeconds = 60
	rs, err := e.derive(userA, short)
	s.code(err, codes.OK, "derive with ttl_seconds=60 and inherited hosts")
	s.ok(err == nil && rs.GetExpiresAt().AsTime().Before(p1.GetExpiresAt().AsTime()), "a shorter ttl gives an earlier expiry than the parent's")
	s.ok(err == nil && len(rs.GetAllowedDomains()) == len(parentHosts), "an empty host list inherits the parent's hosts")

	// idempotency and conflict
	again, err := e.derive(userA, e.runReq(parent, "run-1", []string{"docs.example.com"}))
	s.code(err, codes.OK, "replay with the same session_key and hosts")
	s.ok(err == nil && again.GetGrantId() == r1.GetGrantId(), "the replay returned the same grant id")
	_, err = e.derive(userA, e.runReq(parent, "run-1", []string{"docs.example.org"}))
	s.code(err, codes.AlreadyExists, "replay with the same session_key and different hosts")
	_, err = e.derive(userA, e.runReq(parent, "run-wide", []string{"evil.example.net"}))
	s.code(err, codes.PermissionDenied, "a host outside the parent's is refused")
	_, err = e.derive(userA, e.runReq(parent, "run-wide2", []string{"badexample.com"}))
	s.code(err, codes.PermissionDenied, "badexample.com is not 'below' example.com (dot boundary)")
	badPurpose := e.runReq(parent, "run-purpose", nil)
	badPurpose.Purpose = "attach"
	_, err = e.derive(userA, badPurpose)
	s.code(err, codes.InvalidArgument, "an unknown purpose is refused")

	// per-parent cap of 8 live run grants (run-1, run-short and six more)
	for i := 3; i <= 8; i++ {
		_, err = e.derive(userA, e.runReq(parent, "run-"+string(rune('0'+i)), nil))
		s.code(err, codes.OK, "derive run-"+string(rune('0'+i)))
	}
	s.ok(e.count(t, "parent_grant_id = '"+parent+"' AND kind = 'run' AND revoked_at IS NULL") == 8, "eight live run grants under P1")
	_, err = e.derive(userA, e.runReq(parent, "run-9", nil))
	s.code(err, codes.ResourceExhausted, "the ninth live run grant under one session is refused")
	s.ok(e.count(t, "parent_grant_id = '"+parent+"' AND kind = 'run'") == 8, "the refused ninth left no row")
	again, err = e.derive(userA, e.runReq(parent, "run-1", []string{"docs.example.com"}))
	s.code(err, codes.OK, "a replay of an issued key at the cap")
	s.ok(err == nil && again.GetGrantId() == r1.GetGrantId(), "a replay at the cap is not a new grant")

	// a service (model-gateway) derives from a user's session; the child belongs to the user
	p2, err := e.acquire(userA, e.session("e2e-c-p2", parentHosts))
	s.code(err, codes.OK, "parent session P2")
	s.must(err == nil, "P2 issued")
	svcRun, err := e.derive(gateway, e.runReq(p2.GetGrantId(), "svc-run", nil))
	s.code(err, codes.OK, "model-gateway derives from the user's session")
	s.must(err == nil, "service-derived run issued")
	vsvc, err := e.validate(userA, svcRun.GetGrantId())
	s.code(err, codes.OK, "the user validates the service-derived run grant")
	s.ok(err == nil && vsvc.GetOwnerId() == "user-a" && vsvc.GetKind() == "run" && vsvc.GetParentGrantId() == p2.GetGrantId(), "the run belongs to the parent's owner")

	// ZDR parent -> ZDR child that never reaches Postgres
	rowsBefore := e.count(t, "")
	zreq := e.session("e2e-c-pz", parentHosts)
	zreq.Zdr = true
	pz, err := e.acquire(userA, zreq)
	s.code(err, codes.OK, "ZDR parent")
	s.must(err == nil, "ZDR parent issued")
	zc, err := e.derive(userA, e.runReq(pz.GetGrantId(), "zrun-1", nil))
	s.code(err, codes.OK, "derive from the ZDR parent")
	s.must(err == nil, "ZDR child issued")
	s.ok(zc.GetZdr() && hasPrefix(zc.GetGrantId(), "z_") && zc.GetParentGrantId() == pz.GetGrantId(), "the child of a ZDR parent is ZDR with a z_ id")
	zs, err := e.derive(gateway, e.runReq(pz.GetGrantId(), "zrun-svc", nil))
	s.ok(err == nil && zs.GetZdr() && hasPrefix(zs.GetGrantId(), "z_"), "a service derive from a ZDR parent is ZDR too")
	s.ok(e.count(t, "") == rowsBefore, "no ZDR grant of the chain reached Postgres (%d rows before and after)", rowsBefore)
	// a ZDR principal cannot derive from a non-ZDR parent
	_, err = e.derive(e.personZDR(t, "user-a"), e.runReq(p2.GetGrantId(), "zdr-principal", nil))
	s.code(err, codes.FailedPrecondition, "a ZDR principal deriving from a non-ZDR parent")
	// revoking the ZDR parent revokes its (memory-only) subtree
	_, err = e.revoke(userA, pz.GetGrantId(), "e2e")
	s.code(err, codes.OK, "revoke the ZDR parent")
	_, err = e.validate(userA, zc.GetGrantId())
	s.code(err, codes.FailedPrecondition, "the ZDR child is revoked with its parent")
	_, err = e.validate(gateway, zs.GetGrantId())
	s.code(err, codes.FailedPrecondition, "the service-derived ZDR child is revoked too")

	// subtree revoke T1 (before a restart)
	_, err = e.revoke(userA, parent, "e2e-subtree")
	s.code(err, codes.OK, "revoke P1")
	_, err = e.validate(userA, parent)
	s.code(err, codes.FailedPrecondition, "P1 is revoked")
	_, err = e.validate(userA, r1.GetGrantId())
	s.code(err, codes.FailedPrecondition, "run-1 is revoked with P1")
	s.ok(e.count(t, "(id = '"+parent+"' OR parent_grant_id = '"+parent+"') AND revoked_at IS NOT NULL") == 9, "P1 and its eight run grants are revoked in Postgres")
	_, err = e.derive(userA, e.runReq(parent, "after-revoke", nil))
	s.code(err, codes.FailedPrecondition, "deriving from a revoked parent")

	// subtree T2: restart first, then revoke the parent
	p3, err := e.acquire(userA, e.session("e2e-c-p3", parentHosts))
	s.code(err, codes.OK, "parent session P3")
	s.must(err == nil, "P3 issued")
	var kids []string
	for _, k := range []string{"t2-1", "t2-2", "t2-3"} {
		c, err := e.derive(userA, e.runReq(p3.GetGrantId(), k, nil))
		s.code(err, codes.OK, "derive "+k)
		s.must(err == nil, "%s issued", k)
		kids = append(kids, c.GetGrantId())
	}
	e.restartBroker(t)
	_, err = e.validate(userA, parent)
	s.code(err, codes.FailedPrecondition, "P1 stays revoked after the restart")
	_, err = e.validate(userA, r1.GetGrantId())
	s.code(err, codes.FailedPrecondition, "run-1 stays revoked after the restart")
	s.ok(e.count(t, "(id = '"+parent+"' OR parent_grant_id = '"+parent+"') AND revoked_at IS NOT NULL") == 9, "the nine revoked rows are still revoked after the restart")
	for _, id := range kids {
		v, err := e.validate(userA, id)
		s.ok(err == nil && v.GetActive() && v.GetParentGrantId() == p3.GetGrantId(), "a run grant of P3 is active after the restart")
	}
	_, err = e.revoke(userA, p3.GetGrantId(), "e2e-after-restart")
	s.code(err, codes.OK, "revoke P3 after the restart")
	for _, id := range append([]string{p3.GetGrantId()}, kids...) {
		_, err := e.validate(userA, id)
		s.code(err, codes.FailedPrecondition, "a grant of the P3 subtree after revoking P3")
	}
	s.ok(e.count(t, "(id = '"+p3.GetGrantId()+"' OR parent_grant_id = '"+p3.GetGrantId()+"') AND revoked_at IS NOT NULL") == 4, "P3 and its three children are revoked in Postgres")
	// P2 and its service-derived child were not touched
	v, err := e.validate(userA, svcRun.GetGrantId())
	s.ok(err == nil && v.GetActive(), "an unrelated run grant is still active")
}

// (d) WS6.E: a derive's budget claim can only tighten, and a replay without a
// claim is a lookup that never resets the stored claim.
func scenarioD(t *testing.T, e *env) {
	s := &sc{t: t, name: "d_budget_claim"}
	defer s.done()
	userA := e.person(t, "user-a")

	// parent claim: shape errors first
	bad := e.session("e2e-d-bad", parentHosts)
	bad.BudgetMicroUsd, bad.BudgetSource = 100, "bogus"
	_, err := e.acquire(userA, bad)
	s.code(err, codes.InvalidArgument, "an unknown budget source")
	bad = e.session("e2e-d-bad2", parentHosts)
	bad.BudgetSource = "request"
	_, err = e.acquire(userA, bad)
	s.code(err, codes.InvalidArgument, "a source with no bound")
	bad = e.session("e2e-d-bad3", parentHosts)
	bad.BudgetMicroUsd, bad.BudgetSource, bad.BudgetMintedAtUnix = 100, "request", 5
	_, err = e.acquire(userA, bad)
	s.code(err, codes.InvalidArgument, "a caller-set minted_at stamp")

	preq := e.session("e2e-d-parent", parentHosts)
	preq.BudgetMicroUsd, preq.BudgetMaxSteps, preq.BudgetMaxRuntimeS, preq.BudgetSource = 5_000_000, 40, 600, "org_default"
	parent, err := e.acquire(userA, preq)
	s.code(err, codes.OK, "a parent session carrying a budget claim")
	s.must(err == nil, "budget parent issued")
	s.ok(parent.GetBudgetMicroUsd() == 5_000_000 && parent.GetBudgetMaxSteps() == 40 && parent.GetBudgetMaxRuntimeS() == 600 && parent.GetBudgetSource() == "org_default", "the parent's claim is echoed")
	s.ok(parent.GetBudgetMintedAtUnix() > 0, "the broker stamped the claim")
	pid := parent.GetGrantId()

	// a claim above the parent's changes nothing
	over := e.runReq(pid, "d-over", nil)
	over.BudgetMicroUsd, over.BudgetSource = 6_000_000, "request"
	_, err = e.derive(userA, over)
	s.code(err, codes.PermissionDenied, "micro_usd above the parent's")
	overSteps := e.runReq(pid, "d-over-steps", nil)
	overSteps.BudgetMaxSteps, overSteps.BudgetSource = 41, "request"
	_, err = e.derive(userA, overSteps)
	s.code(err, codes.PermissionDenied, "max_steps above the parent's")
	overRuntime := e.runReq(pid, "d-over-runtime", nil)
	overRuntime.BudgetMaxRuntimeS, overRuntime.BudgetSource = 601, "request"
	_, err = e.derive(userA, overRuntime)
	s.code(err, codes.PermissionDenied, "max_runtime_s above the parent's")
	s.ok(e.count(t, "parent_grant_id = '"+pid+"'") == 0, "the refused derives left no row")
	stamp := e.sql(t, "SELECT budget_micro_usd||','||budget_max_steps||','||budget_max_runtime_s||','||budget_source||','||budget_minted_at_unix FROM browser_grants WHERE id = '"+pid+"'")
	s.ok(strings.HasPrefix(stamp, "5000000,40,600,org_default,"), "the parent's stored claim is unchanged by the refused derives")
	forged := e.runReq(pid, "d-forged-stamp", nil)
	forged.BudgetMicroUsd, forged.BudgetSource, forged.BudgetMintedAtUnix = 1_000_000, "request", 1
	_, err = e.derive(userA, forged)
	s.code(err, codes.InvalidArgument, "a derive that sets the broker's minted_at stamp")

	// a tighter claim: the omitted dimensions inherit
	tight := e.runReq(pid, "d-tight", nil)
	tight.BudgetMicroUsd, tight.BudgetSource = 2_000_000, "request"
	child, err := e.derive(userA, tight)
	s.code(err, codes.OK, "a tighter claim")
	s.must(err == nil, "tight child issued")
	s.ok(child.GetBudgetMicroUsd() == 2_000_000, "micro_usd is the tighter value")
	s.ok(child.GetBudgetMaxSteps() == 40 && child.GetBudgetMaxRuntimeS() == 600, "omitted dimensions inherit the parent's bounds")
	s.ok(child.GetBudgetSource() == "request" && child.GetBudgetMintedAtUnix() > 0, "source is the request's, stamp is the broker's")
	minted := child.GetBudgetMintedAtUnix()

	time.Sleep(2200 * time.Millisecond) // a re-stamp would move the unix second

	// a replay without a claim is a lookup
	look, err := e.derive(userA, e.runReq(pid, "d-tight", nil))
	s.code(err, codes.OK, "a replay without a claim")
	s.ok(err == nil && look.GetGrantId() == child.GetGrantId(), "the replay returned the issued grant")
	s.ok(err == nil && look.GetBudgetMicroUsd() == 2_000_000 && look.GetBudgetMaxSteps() == 40 && look.GetBudgetMaxRuntimeS() == 600 && look.GetBudgetSource() == "request", "the stored claim came back, not a fresh one")
	s.ok(err == nil && look.GetBudgetMintedAtUnix() == minted, "the stamp was not refreshed by the lookup")
	// a replay with a different claim is a conflict; above the parent's it is refused
	diff := e.runReq(pid, "d-tight", nil)
	diff.BudgetMicroUsd, diff.BudgetSource = 1_000_000, "request"
	_, err = e.derive(userA, diff)
	s.code(err, codes.AlreadyExists, "a replay with a different (tighter) claim")
	wide := e.runReq(pid, "d-tight", nil)
	wide.BudgetMicroUsd, wide.BudgetSource = 7_000_000, "request"
	_, err = e.derive(userA, wide)
	s.code(err, codes.PermissionDenied, "a replay with a claim above the parent's")
	same := e.runReq(pid, "d-tight", nil)
	same.BudgetMicroUsd, same.BudgetSource = 2_000_000, "request"
	sr, err := e.derive(userA, same)
	s.code(err, codes.OK, "a replay with the same claim")
	s.ok(err == nil && sr.GetGrantId() == child.GetGrantId() && sr.GetBudgetMintedAtUnix() == minted, "the same claim returns the same grant and stamp")

	// no claim at all inherits the parent's claim exactly
	inh, err := e.derive(userA, e.runReq(pid, "d-inherit", nil))
	s.code(err, codes.OK, "a derive with no claim")
	s.ok(err == nil && inh.GetBudgetMicroUsd() == 5_000_000 && inh.GetBudgetMaxSteps() == 40 && inh.GetBudgetMaxRuntimeS() == 600 && inh.GetBudgetSource() == "org_default", "the child carries the parent's claim")
	s.ok(err == nil && inh.GetBudgetMintedAtUnix() == parent.GetBudgetMintedAtUnix(), "the inherited stamp is the parent's")

	// ZDR parent with a claim: the child carries it, nothing reaches Postgres
	rows := e.count(t, "")
	zreq := e.session("e2e-d-zdr", parentHosts)
	zreq.Zdr, zreq.BudgetMaxSteps, zreq.BudgetSource = true, 10, "entitlement"
	zp, err := e.acquire(userA, zreq)
	s.code(err, codes.OK, "a ZDR parent with a claim")
	s.must(err == nil, "ZDR budget parent issued")
	zc := e.runReq(zp.GetGrantId(), "d-zrun", nil)
	zc.BudgetMaxSteps, zc.BudgetSource = 5, "request"
	zchild, err := e.derive(userA, zc)
	s.code(err, codes.OK, "a tighter claim under a ZDR parent")
	s.ok(err == nil && zchild.GetZdr() && zchild.GetBudgetMaxSteps() == 5 && hasPrefix(zchild.GetGrantId(), "z_"), "the ZDR child carries the tighter claim")
	zover := e.runReq(zp.GetGrantId(), "d-zover", nil)
	zover.BudgetMaxSteps, zover.BudgetSource = 11, "request"
	_, err = e.derive(userA, zover)
	s.code(err, codes.PermissionDenied, "a claim above a ZDR parent's")
	s.ok(e.count(t, "") == rows, "no ZDR claim reached Postgres (%d rows)", rows)

	// durability: the claim comes back from Postgres after a restart
	e.restartBroker(t)
	v, err := e.validate(userA, child.GetGrantId())
	s.code(err, codes.OK, "validate the tightened child after the restart")
	s.ok(err == nil && v.GetBudgetMicroUsd() == 2_000_000 && v.GetBudgetMaxSteps() == 40 && v.GetBudgetMaxRuntimeS() == 600 && v.GetBudgetSource() == "request" && v.GetBudgetMintedAtUnix() == minted, "the claim and its stamp survived the restart")
	look, err = e.derive(userA, e.runReq(pid, "d-tight", nil))
	s.ok(err == nil && look.GetGrantId() == child.GetGrantId() && look.GetBudgetMintedAtUnix() == minted, "a no-claim replay after the restart still finds the grant with its original stamp")
}

// (f) Authentication and authorization over the real verifier.
func scenarioF(t *testing.T, e *env, sessionA string) {
	s := &sc{t: t, name: "f_auth"}
	defer s.done()
	s.must(sessionA != "", "scenario a left a session grant")
	userA := e.person(t, "user-a")

	// authentication: every refusal is Unauthenticated, never a silent allow
	_, err := e.validate("", sessionA)
	s.code(err, codes.Unauthenticated, "no token")
	wrongAud := idp.Person(e.issuer, e.org, "user-a")
	wrongAud.Audience = "model-gateway"
	_, err = e.validate(e.sign(t, wrongAud), sessionA)
	s.code(err, codes.Unauthenticated, "a token for another audience")
	expired := idp.Person(e.issuer, e.org, "user-a")
	expired.IssuedAt, expired.NotBefore, expired.Expires = time.Now().Add(-30*time.Minute), time.Now().Add(-30*time.Minute), time.Now().Add(-10*time.Minute)
	_, err = e.validate(e.sign(t, expired), sessionA)
	s.code(err, codes.Unauthenticated, "an expired token")
	_, err = e.validate(e.sign(t, idp.Person("http://elsewhere.invalid", e.org, "user-a")), sessionA)
	s.code(err, codes.Unauthenticated, "a token from another issuer")
	foreign, err := idp.Generate(idp.DefaultKID)
	s.must(err == nil, "a second key for the forgery")
	forged, err := foreign.Sign(idp.Person(e.issuer, e.org, "user-a"))
	s.must(err == nil, "sign the forgery")
	_, err = e.validate(forged, sessionA)
	s.code(err, codes.Unauthenticated, "a token signed by a key the JWKS does not hold")
	_, err = e.acquire(wrongAudToken(t, e), e.session("e2e-f-denied", parentHosts))
	s.code(err, codes.Unauthenticated, "AcquireGrant with a wrong-audience token")
	// the standard health service stays public
	hc := healthClient(t, e)
	hctx, hcancel := rpcCtx("")
	defer hcancel()
	hres, err := hc.Check(hctx, &grpc_health_v1.HealthCheckRequest{})
	s.ok(err == nil && hres.GetStatus() == grpc_health_v1.HealthCheckResponse_SERVING, "grpc.health.v1 answers SERVING without a token")

	// authorization: a person may call every method; a service needs the right scope
	readOnly := e.service(t, "quarry-edge", scopeRead)
	v, err := e.validate(readOnly, sessionA)
	s.code(err, codes.OK, "a browser:read service validates a grant in its org")
	s.ok(err == nil && v.GetActive(), "and sees it active")
	_, err = e.acquire(readOnly, e.session("e2e-f-svc", parentHosts))
	s.code(err, codes.PermissionDenied, "a browser:read service acquiring a grant")
	_, err = e.derive(readOnly, e.runReq(sessionA, "f-ro", nil))
	s.code(err, codes.PermissionDenied, "a browser:read service deriving a grant")
	_, err = e.revoke(readOnly, sessionA, "e2e")
	s.code(err, codes.PermissionDenied, "a browser:read service revoking a grant")
	writeOnly := e.service(t, "writer-only", scopeWrite)
	_, err = e.validate(writeOnly, sessionA)
	s.code(err, codes.PermissionDenied, "a browser:write-only service validating (validate needs browser:read)")
	writer := e.service(t, "model-gateway", scopeRead, scopeWrite)
	w, err := e.acquire(writer, e.session("e2e-f-svc-ok", otherHosts))
	s.code(err, codes.OK, "a browser:write service acquiring a grant")
	s.ok(err == nil && e.count(t, "id = '"+w.GetGrantId()+"' AND owner_id = 'model-gateway'") == 1, "the grant is owned by the service identity")
	_, err = e.revoke(writer, w.GetGrantId(), "e2e")
	s.code(err, codes.OK, "a browser:write service revoking it")
	// No BrowserBroker method is service-only (authz.Authorize lets every
	// person through), so there is no 'person calls a service-only method'
	// refusal to assert; the scope refusals above are the service-side checks.

	// tenant pinning
	cross := e.session("e2e-f-cross-org", parentHosts)
	cross.OrgId = e.org2
	_, err = e.acquire(userA, cross)
	s.code(err, codes.PermissionDenied, "a request that names another organization")
	crossDerive := e.runReq(sessionA, "f-cross", nil)
	crossDerive.OrgId = e.org2
	_, err = e.derive(userA, crossDerive)
	s.code(err, codes.PermissionDenied, "a derive that names another organization")

	// ownership: another person, and another organization, get NotFound for an existing grant
	_, errMissing := e.validate(userA, "0123456789abcdef0123456789abcdef")
	s.code(errMissing, codes.NotFound, "a grant id that does not exist")
	userB := e.person(t, "user-b")
	_, errOther := e.validate(userB, sessionA)
	s.code(errOther, codes.NotFound, "another person validating user-a's grant (NotFound, not PermissionDenied)")
	s.ok(errOther != nil && errMissing != nil && errMessage(errOther) == errMessage(errMissing), "the message is identical to the one for a missing grant: no existence oracle")
	_, err = e.revoke(userB, sessionA, "e2e")
	s.code(err, codes.NotFound, "another person revoking user-a's grant")
	_, err = e.derive(userB, e.runReq(sessionA, "f-b", nil))
	s.code(err, codes.NotFound, "another person deriving from user-a's grant")
	_, err = e.validate(e.personInOrg2(t, "user-a"), sessionA)
	s.code(err, codes.NotFound, "the same user id in another organization")
	// another organization cannot derive from, revoke or read it, as a person or as a
	// browser:write service: the same NotFound as for a missing grant, never a refusal that
	// shows the grant exists
	_, errMissingDerive := e.derive(userA, e.runReq("0123456789abcdef0123456789abcdef", "f-missing", nil))
	s.code(errMissingDerive, codes.NotFound, "deriving from a parent that does not exist")
	org2Person := e.personInOrg2(t, "user-a")
	org2Service := e.sign(t, idp.Service(e.issuer, e.org2, "model-gateway", scopeRead, scopeWrite))
	org2Run := e.runReq(sessionA, "f-org2", nil)
	org2Run.OrgId = e.org2
	for _, who := range []struct{ name, token string }{{"a person of another organization", org2Person}, {"a service of another organization", org2Service}} {
		_, err = e.derive(who.token, org2Run)
		s.code(err, codes.NotFound, who.name+" deriving from the grant")
		s.ok(err != nil && errMissingDerive != nil && errMessage(err) == errMessage(errMissingDerive), who.name+": the derive message equals the one for a missing parent")
		_, err = e.revoke(who.token, sessionA, "e2e")
		s.code(err, codes.NotFound, who.name+" revoking the grant")
		_, err = e.validate(who.token, sessionA)
		s.code(err, codes.NotFound, who.name+" validating the grant")
	}
	s.ok(e.count(t, "parent_grant_id = '"+sessionA+"' AND session_key = 'f-org2'") == 0, "the cross-organization derive left no row")
	v, err = e.validate(userA, sessionA)
	s.ok(err == nil && v.GetActive(), "user-a's grant is still active after the attempts")
}

// (e) With the database down nothing answers 'valid'; when it is back, the
// earlier grant validates again.
func scenarioE(t *testing.T, e *env, sessionA string) {
	s := &sc{t: t, name: "e_postgres_down"}
	defer s.done()
	s.must(sessionA != "", "scenario a left a session grant")
	userA := e.person(t, "user-a")

	// a ZDR grant made while the database is still up, to show it does not need it
	zreq := e.session("e2e-e-zdr", parentHosts)
	zreq.Zdr = true
	zg, err := e.acquire(userA, zreq)
	s.code(err, codes.OK, "a ZDR grant before the outage")
	s.must(err == nil, "ZDR grant issued")
	v, err := e.validate(userA, sessionA)
	s.ok(err == nil && v.GetActive(), "the non-ZDR grant is valid before the outage")

	out, err := docker("stop", e.pg)
	s.must(err == nil, "docker stop postgres: %v %s", err, out)
	down := false
	for i := 0; i < 30 && !down; i++ {
		down = e.httpStatus("/readyz") == 503
		if !down {
			time.Sleep(time.Second)
		}
	}
	s.ok(down, "/readyz is 503 while the database is down")
	s.ok(e.httpStatus("/healthz") == 200, "/healthz stays 200 (liveness is not readiness)")

	start := time.Now()
	vd, err := e.validate(userA, sessionA)
	s.code(err, codes.Unavailable, "ValidateGrant of the durable grant with the database down")
	s.ok(err != nil && !vd.GetActive(), "no 'valid' answer: the response is not active")
	s.ok(time.Since(start) < 15*time.Second, "the refusal is prompt (%s)", time.Since(start).Round(time.Millisecond))
	_, err = e.acquire(userA, e.session("e2e-e-down", parentHosts))
	s.code(err, codes.Unavailable, "AcquireGrant (durable) with the database down")
	_, err = e.derive(userA, e.runReq(sessionA, "e-down-run", nil))
	s.code(err, codes.Unavailable, "DeriveRunGrant with the database down")
	_, err = e.revoke(userA, sessionA, "e2e")
	s.code(err, codes.Unavailable, "RevokeGrant with the database down (a revoke must not be reported done)")
	vz, err := e.validate(userA, zg.GetGrantId())
	s.ok(err == nil && vz.GetActive(), "a ZDR grant, held in memory, does not depend on the database")

	out, err = docker("start", e.pg)
	s.must(err == nil, "docker start postgres: %v %s", err, out)
	up := false
	for i := 0; i < 90 && !up; i++ {
		if _, perr := docker("exec", e.pg, "sh", "-c", `pg_isready -q -h "$(hostname -i)" -U `+e.pgUser+` -d `+e.pgDB); perr == nil {
			up = e.httpStatus("/readyz") == 200
		}
		if !up {
			time.Sleep(time.Second)
		}
	}
	s.ok(up, "/readyz is 200 again once the database is back")
	v, err = e.validate(userA, sessionA)
	s.code(err, codes.OK, "the earlier non-ZDR grant after the database came back")
	s.ok(err == nil && v.GetActive() && v.GetGrantId() == sessionA, "it validates again, same id")
	again, err := e.acquire(userA, e.session("e2e-e-after", parentHosts))
	s.code(err, codes.OK, "a new durable grant after the outage")
	s.ok(err == nil && e.count(t, "id = '"+again.GetGrantId()+"'") == 1, "its row is in Postgres")
}

// ---- small helpers local to the scenarios ----------------------------------

func wrongAudToken(t *testing.T, e *env) string {
	spec := idp.Person(e.issuer, e.org, "user-a")
	spec.Audience = "model-gateway"
	return e.sign(t, spec)
}

func healthClient(t *testing.T, e *env) grpc_health_v1.HealthClient {
	port, err := hostPort(e.broker, "9095/tcp")
	if err != nil {
		t.Fatalf("resolve the broker gRPC port: %v", err)
	}
	conn, err := grpc.NewClient("127.0.0.1:"+port, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	return grpc_health_v1.NewHealthClient(conn)
}
