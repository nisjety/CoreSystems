//go:build e2e

package e2e

import (
	"slices"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/services/browser-broker/e2e/idp"
	"google.golang.org/grpc/codes"
)

// approval is an AcquireGrant request for a sensitive approval (one
// download_ref action in one frame) under parent. The session key must be the
// parent's: an approval is bound to the session or run it was approved in.
func (e *env) approval(parent, sessionKey string, hosts []string) *mpv1.AcquireGrantRequest {
	return &mpv1.AcquireGrantRequest{
		SessionKey: sessionKey, Mode: "cloud", OrgId: e.org, AllowedDomains: hosts,
		AllowedActions: []string{"download_ref"}, AllowedFrameIds: []string{"frame-1"},
		ParentGrantId: parent,
	}
}

// (g) Sensitive approvals, the other derived-grant path: never wider than the
// parent (hosts, expiry, ZDR), bound to one parent, refused across owners and
// organizations with NotFound, revoked with the parent.
func scenarioG(t *testing.T, e *env) {
	s := &sc{t: t, name: "g_sensitive_approval"}
	defer s.done()
	userG := e.person(t, "user-g")
	userG2 := e.person(t, "user-g2")
	gateway := e.service(t, "model-gateway", scopeRead, scopeWrite)
	org2Gateway := e.sign(t, idp.Service(e.issuer, e.org2, "model-gateway", scopeRead, scopeWrite))
	org2Req := func(r *mpv1.AcquireGrantRequest) *mpv1.AcquireGrantRequest { r.OrgId = e.org2; return r }

	p, err := e.acquire(userG, e.session("e2e-g-s", parentHosts))
	s.code(err, codes.OK, "parent session P")
	s.must(err == nil, "P issued")
	pid := p.GetGrantId()
	runReq := e.runReq(pid, "e2e-g-run", nil)
	runReq.TtlSeconds = 60
	r, err := e.derive(userG, runReq)
	s.code(err, codes.OK, "run grant R under P with ttl 60s")
	s.must(err == nil, "R issued")
	rid := r.GetGrantId()

	// an approval under the session: kind, parent, scopes, hosts, expiry, row
	a1, err := e.acquire(userG, e.approval(pid, "e2e-g-s", []string{parentHosts[0]}))
	s.code(err, codes.OK, "approval A1 under P")
	s.must(err == nil, "A1 issued")
	s.ok(a1.GetKind() == "sensitive_approval" && a1.GetParentGrantId() == pid && !a1.GetZdr() && !hasPrefix(a1.GetGrantId(), "z_"), "A1 is a non-ZDR sensitive_approval bound to P")
	s.ok(slices.Equal(a1.GetAllowedActions(), []string{"download_ref"}) && slices.Equal(a1.GetAllowedFrameIds(), []string{"frame-1"}), "A1 carries exactly its one action and frame")
	s.ok(slices.Equal(a1.GetAllowedDomains(), []string{parentHosts[0]}), "A1's hosts are the requested subset")
	s.ok(!a1.GetExpiresAt().AsTime().After(p.GetExpiresAt().AsTime()), "A1 expiry <= P expiry")
	s.ok(e.count(t, "id = '"+a1.GetGrantId()+"' AND parent_grant_id = '"+pid+"' AND kind = 'sensitive_approval'") == 1, "A1 is a durable row under P")
	va, err := e.validate(userG, a1.GetGrantId())
	s.ok(err == nil && va.GetActive() && va.GetOwnerId() == "user-g" && va.GetKind() == "sensitive_approval", "the owner validates A1")
	sub, err := e.acquire(userG, e.approval(pid, "e2e-g-s", []string{"docs." + parentHosts[0]}))
	s.code(err, codes.OK, "an approval for a subdomain of a parent host")
	s.ok(err == nil && slices.Equal(sub.GetAllowedDomains(), []string{"docs." + parentHosts[0]}), "its host is the subdomain")

	// an approval is never wider than its parent
	rows := e.count(t, "")
	_, err = e.acquire(userG, e.approval(pid, "e2e-g-s", otherHosts))
	s.code(err, codes.PermissionDenied, "an approval for a host outside the parent's")
	_, err = e.acquire(userG, e.approval(pid, "e2e-g-s", []string{"bad" + parentHosts[0]}))
	s.code(err, codes.PermissionDenied, "a look-alike host is not 'below' a parent host (dot boundary)")
	_, err = e.acquire(userG, e.approval(pid, "e2e-g-s", []string{parentHosts[0], otherHosts[0]}))
	s.code(err, codes.PermissionDenied, "one host outside the parent's refuses the whole approval")
	s.ok(e.count(t, "") == rows, "the refused approvals left no row (%d)", rows)

	// the expiry is capped at the parent's even though the approval ttl is 15 minutes
	a2, err := e.acquire(userG, e.approval(rid, "e2e-g-run", []string{parentHosts[0]}))
	s.code(err, codes.OK, "approval A2 under the run grant R")
	s.must(err == nil, "A2 issued")
	s.ok(a2.GetParentGrantId() == rid && a2.GetKind() == "sensitive_approval", "A2 is bound to R")
	s.ok(!a2.GetExpiresAt().AsTime().After(r.GetExpiresAt().AsTime()), "A2 expiry <= R expiry")
	s.ok(a2.GetExpiresAt().AsTime().Before(time.Now().Add(2*time.Minute)), "A2 expires within 2 minutes although the approval ttl is longer: the parent's expiry capped it")

	rows = e.count(t, "") // A2 added a row
	// request shape
	bad := e.approval(pid, "e2e-g-s", []string{parentHosts[0]})
	bad.AllowedActions = []string{"navigate"}
	_, err = e.acquire(userG, bad)
	s.code(err, codes.InvalidArgument, "an action that is not a sensitive action")
	bad = e.approval(pid, "e2e-g-s", []string{parentHosts[0]})
	bad.AllowedActions = []string{"download_ref", "upload_ref"}
	_, err = e.acquire(userG, bad)
	s.code(err, codes.InvalidArgument, "two actions in one approval")
	bad = e.approval("", "e2e-g-s", []string{parentHosts[0]})
	_, err = e.acquire(userG, bad)
	s.code(err, codes.InvalidArgument, "an approval without a parent")
	bad = e.approval(pid, "e2e-g-s", []string{parentHosts[0]})
	bad.BudgetMicroUsd, bad.BudgetSource = 1000, "request"
	_, err = e.acquire(userG, bad)
	s.code(err, codes.InvalidArgument, "an approval carrying a budget claim")
	s.ok(e.count(t, "") == rows, "none of them left a row")

	// the parent must be an ordinary grant of the same session
	_, err = e.derive(userG, e.runReq(a1.GetGrantId(), "g-from-approval", nil))
	s.code(err, codes.FailedPrecondition, "deriving a run grant from an approval")
	_, err = e.derive(userG, e.runReq(rid, "g-from-run", nil))
	s.code(err, codes.FailedPrecondition, "deriving a run grant from a run grant (no chains)")
	_, err = e.acquire(userG, e.approval(pid, "another-session-key", []string{parentHosts[0]}))
	s.code(err, codes.FailedPrecondition, "an approval whose session key is not the parent's (a caller mistake, not Internal)")
	_, err = e.acquire(userG, e.approval(a1.GetGrantId(), "e2e-g-s", []string{parentHosts[0]}))
	s.code(err, codes.FailedPrecondition, "an approval under an approval (a caller mistake, not Internal)")
	s.ok(e.count(t, "") == rows, "neither left a row")

	// ZDR: a ZDR principal cannot hang a persisted approval under a persisted parent
	_, err = e.acquire(e.personZDR(t, "user-g"), e.approval(pid, "e2e-g-s", []string{parentHosts[0]}))
	s.code(err, codes.FailedPrecondition, "a ZDR principal acquiring an approval under a persistent parent")
	s.ok(e.count(t, "") == rows, "no row was added")

	// other owner and other organization: the same NotFound as for a missing parent
	_, errMissing := e.acquire(userG, e.approval("0123456789abcdef0123456789abcdef", "e2e-g-s", []string{parentHosts[0]}))
	s.code(errMissing, codes.NotFound, "an approval under a parent id that does not exist")
	_, err = e.acquire(userG2, e.approval(pid, "e2e-g-s", []string{parentHosts[0]}))
	s.code(err, codes.NotFound, "another person acquiring an approval under user-g's session")
	s.ok(err != nil && errMissing != nil && errMessage(err) == errMessage(errMissing), "the message equals the one for a missing parent: no existence oracle")
	_, err = e.acquire(e.personInOrg2(t, "user-g"), org2Req(e.approval(pid, "e2e-g-s", []string{parentHosts[0]})))
	s.code(err, codes.NotFound, "the same user id in another organization acquiring an approval under P")
	_, err = e.acquire(org2Gateway, org2Req(e.approval(pid, "e2e-g-s", []string{parentHosts[0]})))
	s.code(err, codes.NotFound, "a browser:write service of another organization acquiring an approval under P")
	_, err = e.acquire(org2Gateway, org2Req(e.approval(rid, "e2e-g-run", []string{parentHosts[0]})))
	s.code(err, codes.NotFound, "the same, under the run grant R")
	// an approval is owned by the identity that acquires it: a service of the
	// same organization does not mint one under a user's grant (fail closed)
	_, err = e.acquire(gateway, e.approval(pid, "e2e-g-s", []string{parentHosts[0]}))
	s.code(err, codes.NotFound, "a same-organization service acquiring an approval under a user's session")
	s.ok(e.count(t, "") == rows, "none of the cross-tenant attempts left a row")
	_, err = e.derive(userG2, e.runReq(rid, "g-x", nil))
	s.code(err, codes.NotFound, "another person deriving from user-g's run grant")

	// another owner or organization cannot revoke or read an approval
	_, err = e.revoke(userG2, a1.GetGrantId(), "e2e")
	s.code(err, codes.NotFound, "another person revoking A1")
	_, err = e.revoke(e.personInOrg2(t, "user-g"), a1.GetGrantId(), "e2e")
	s.code(err, codes.NotFound, "the same user id in another organization revoking A1")
	_, err = e.revoke(org2Gateway, a1.GetGrantId(), "e2e")
	s.code(err, codes.NotFound, "a service of another organization revoking A1")
	_, err = e.validate(userG2, a1.GetGrantId())
	s.code(err, codes.NotFound, "another person validating A1")
	v, err := e.validate(userG, a1.GetGrantId())
	s.ok(err == nil && v.GetActive(), "A1 is still active after the attempts")

	// revoking an approval leaves its parent; revoking the parent takes the rest
	_, err = e.revoke(userG, a1.GetGrantId(), "e2e")
	s.code(err, codes.OK, "revoke A1 alone")
	_, err = e.validate(userG, a1.GetGrantId())
	s.code(err, codes.FailedPrecondition, "A1 is revoked")
	v, err = e.validate(userG, pid)
	s.ok(err == nil && v.GetActive(), "P is untouched by the revoke of its approval")
	_, err = e.revoke(userG, pid, "e2e-subtree")
	s.code(err, codes.OK, "revoke P")
	for _, id := range []string{pid, rid, a2.GetGrantId(), sub.GetGrantId()} {
		_, err = e.validate(userG, id)
		s.code(err, codes.FailedPrecondition, "a grant of P's subtree after revoking P")
	}
	_, err = e.acquire(userG, e.approval(pid, "e2e-g-s", []string{parentHosts[0]}))
	s.code(err, codes.FailedPrecondition, "an approval under a revoked parent")
	s.ok(e.count(t, "(id = '"+pid+"' OR parent_grant_id = '"+pid+"' OR parent_grant_id = '"+rid+"') AND revoked_at IS NULL") == 0, "every row of the subtree is revoked in Postgres")

	// a live approval survives a restart; a revoked subtree stays revoked
	p4, err := e.acquire(userG, e.session("e2e-g-s4", parentHosts))
	s.code(err, codes.OK, "parent session P4")
	s.must(err == nil, "P4 issued")
	a4, err := e.acquire(userG, e.approval(p4.GetGrantId(), "e2e-g-s4", []string{parentHosts[0]}))
	s.code(err, codes.OK, "approval A4 under P4")
	s.must(err == nil, "A4 issued")
	e.restartBroker(t)
	v, err = e.validate(userG, a4.GetGrantId())
	s.code(err, codes.OK, "A4 after the restart")
	s.ok(err == nil && v.GetActive() && v.GetKind() == "sensitive_approval" && v.GetParentGrantId() == p4.GetGrantId() &&
		slices.Equal(v.GetAllowedActions(), []string{"download_ref"}) && slices.Equal(v.GetAllowedFrameIds(), []string{"frame-1"}) &&
		v.GetExpiresAt().AsTime().Sub(a4.GetExpiresAt().AsTime()).Abs() < time.Millisecond, "A4 kept its parent, scopes and expiry")
	_, err = e.validate(userG, a2.GetGrantId())
	s.code(err, codes.FailedPrecondition, "A2 stays revoked after the restart")

	// ZDR parent: its approvals are ZDR, memory-only and revoked with it
	rows = e.count(t, "")
	zreq := e.session("e2e-g-z", parentHosts)
	zreq.Zdr = true
	pz, err := e.acquire(userG, zreq)
	s.code(err, codes.OK, "ZDR parent PZ")
	s.must(err == nil, "PZ issued")
	zrunReq := e.runReq(pz.GetGrantId(), "e2e-g-zrun", nil)
	zrunReq.TtlSeconds = 60
	rz, err := e.derive(userG, zrunReq)
	s.code(err, codes.OK, "ZDR run grant RZ under PZ")
	s.must(err == nil, "RZ issued")
	az, err := e.acquire(userG, e.approval(pz.GetGrantId(), "e2e-g-z", []string{parentHosts[0]}))
	s.code(err, codes.OK, "a non-ZDR request for an approval under a ZDR parent")
	s.ok(err == nil && az.GetZdr() && hasPrefix(az.GetGrantId(), "z_") && az.GetKind() == "sensitive_approval" && az.GetParentGrantId() == pz.GetGrantId(), "the approval inherits ZDR: z_ id, bound to PZ")
	az2, err := e.acquire(userG, e.approval(rz.GetGrantId(), "e2e-g-zrun", []string{parentHosts[0]}))
	s.code(err, codes.OK, "an approval under the ZDR run grant")
	s.ok(err == nil && az2.GetZdr() && hasPrefix(az2.GetGrantId(), "z_") && !az2.GetExpiresAt().AsTime().After(rz.GetExpiresAt().AsTime()) &&
		az2.GetExpiresAt().AsTime().Before(time.Now().Add(2*time.Minute)), "it is ZDR and capped at the ZDR run grant's expiry")
	_, err = e.acquire(userG, e.approval(pz.GetGrantId(), "e2e-g-z", otherHosts))
	s.code(err, codes.PermissionDenied, "a ZDR approval for a host outside the parent's")
	_, err = e.acquire(userG2, e.approval(pz.GetGrantId(), "e2e-g-z", []string{parentHosts[0]}))
	s.code(err, codes.NotFound, "another person acquiring an approval under a ZDR parent")
	_, err = e.acquire(org2Gateway, org2Req(e.approval(pz.GetGrantId(), "e2e-g-z", []string{parentHosts[0]})))
	s.code(err, codes.NotFound, "another organization's service acquiring an approval under a ZDR parent")
	s.ok(e.count(t, "") == rows, "no ZDR grant of the chain reached Postgres (%d rows before and after)", rows)
	_, err = e.revoke(userG2, az.GetGrantId(), "e2e")
	s.code(err, codes.NotFound, "another person revoking a ZDR approval")
	_, err = e.revoke(userG, pz.GetGrantId(), "e2e")
	s.code(err, codes.OK, "revoke PZ")
	for _, id := range []string{rz.GetGrantId(), az.GetGrantId(), az2.GetGrantId()} {
		_, err = e.validate(userG, id)
		s.code(err, codes.FailedPrecondition, "a ZDR grant of PZ's subtree after revoking PZ")
	}
}
