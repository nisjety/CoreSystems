package http

import (
	"context"
	"encoding/json"
	"net/http"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S2 live legs (TEST_DATABASE_URL via d12Pool; skipped without it): the
// real handlers against a real repository.

func seedApprovalRoom(t *testing.T, prefix string, mutate func(*spaces.EffectPolicy)) (w2Room, *spaces.Repository, func(string, ...any)) {
	t.Helper()
	pool := d12Pool(t)
	repo := newLiveSpaceRepo(pool)
	d12SigningEnv(t)
	room := newW2Room(prefix)
	seedW2Room(t, pool, repo, room, func(p *spaces.EffectPolicy) {
		p.ApprovalGrantEntitled, p.AgentActionEntitled, p.ThreadCreateEntitled = true, true, true
		if mutate != nil {
			mutate(p)
		}
	})
	exec := func(query string, args ...any) {
		t.Helper()
		if _, err := pool.Exec(context.Background(), query, args...); err != nil {
			t.Fatalf("%s: %v", query, err)
		}
	}
	// Registered after the seed's cleanup, so it runs first (approval rows hold
	// foreign keys to registered_spaces).
	t.Cleanup(func() {
		for _, table := range []string{"space_approval_grant_events", "space_approval_grants", "space_approval_policies"} {
			_, _ = pool.Exec(context.Background(), `DELETE FROM `+table+` WHERE space_ref = $1`, room.Room)
		}
	})
	return room, repo, exec
}

func liveApprovalEngine(t *testing.T, repo *spaces.Repository, resolver RunActionAuthorityResolver, org, user string) *gin.Engine {
	ctx := gatewayContext()
	ctx["org_id"], ctx["user_id"] = org, user
	return approvalHarness(t, repo, resolver, ctx)
}

func ceilingBody(scope string, revision int64) map[string]any {
	return map[string]any{
		"max_grant_scope": scope, "default_addressee_kind": "requester", "quarantine_mode": "off", "expected_revision": revision,
	}
}

func createGrantBody(room w2Room, key string) map[string]any {
	body := validCreateApprovalGrantBody()
	body["space_ref"], body["agent_subject_id"], body["idempotency_key"] = room.Room, room.Agent, key
	return body
}

func TestApprovalGrantRoutesTypedRefusalsAndCreatorRevokeLive(t *testing.T) {
	room, repo, exec := seedApprovalRoom(t, "ws10s2h", nil)
	base := "/api/v1/internal/spaces/" + room.Room
	as := func(user string) *gin.Engine { return liveApprovalEngine(t, repo, nil, room.Org, user) }

	// Under the default ceiling (once) a rule is not permitted, even for the owner.
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPost, "/api/v1/internal/spaces/approval-grants", createGrantBody(room, "live-key-000001")),
		http.StatusForbidden, "approval_scope_not_permitted")
	// A manager raises the ceiling, then a rule is created.
	if rec := doJSON(as(room.Manager), http.MethodPut, base+"/approval-policy", ceilingBody("always", 0)); rec.Code != http.StatusOK {
		t.Fatalf("raise ceiling: %d %s", rec.Code, rec.Body.String())
	}
	rec := doJSON(as(room.Manager), http.MethodPost, "/api/v1/internal/spaces/approval-grants", createGrantBody(room, "live-key-000001"))
	if rec.Code != http.StatusOK {
		t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
	}
	var created struct {
		Data struct {
			Grant spaces.ApprovalRule `json:"grant"`
		} `json:"data"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &created); err != nil || created.Data.Grant.RuleID == "" {
		t.Fatalf("create body: %v %s", err, rec.Body.String())
	}
	ruleID := created.Data.Grant.RuleID

	// An editor can neither create, list nor change the ceiling.
	wantApprovalError(t, doJSON(as(room.Editor), http.MethodPost, "/api/v1/internal/spaces/approval-grants", createGrantBody(room, "live-key-000002")),
		http.StatusForbidden, "approval_scope_not_permitted")
	wantApprovalError(t, doJSON(as(room.Viewer), http.MethodGet, base+"/approval-grants", nil), http.StatusForbidden, "approval_scope_not_permitted")
	wantApprovalError(t, doJSON(as(room.Editor), http.MethodPut, base+"/approval-policy", ceilingBody("once", 1)), http.StatusForbidden, "approval_scope_not_permitted")
	// A non-member sees no Space at all.
	wantApprovalError(t, doJSON(as("ws10s2h-stranger"), http.MethodGet, base+"/approval-grants", nil), http.StatusNotFound, "space_not_found")
	// The same key for a different action is a 409.
	reused := createGrantBody(room, "live-key-000001")
	reused["tool_key"], reused["action_key"] = "mcp__github__create_issue", "mcp__github__create_issue"
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPost, "/api/v1/internal/spaces/approval-grants", reused), http.StatusConflict, "idempotency_key_reused")
	// An agent that is not in the Space is a 409 too.
	stranger := createGrantBody(room, "live-key-000003")
	stranger["agent_subject_id"] = "not-in-this-space"
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPost, "/api/v1/internal/spaces/approval-grants", stranger), http.StatusConflict, "approval_agent_not_in_space")
	// A stale ceiling revision conflicts.
	wantApprovalError(t, doJSON(as(room.Manager), http.MethodPut, base+"/approval-policy", ceilingBody("once", 0)), http.StatusConflict, "approval_policy_revision_conflict")

	// The creator, now demoted to viewer, can still revoke their own rule; an
	// unrelated viewer cannot.
	exec(`UPDATE space_memberships SET role='viewer' WHERE space_ref=$1 AND subject_id=$2`, room.Room, room.Manager)
	wantApprovalError(t, doJSON(as(room.Viewer), http.MethodDelete, base+"/approval-grants/"+ruleID, nil), http.StatusForbidden, "approval_scope_not_permitted")
	if rec := doJSON(as(room.Manager), http.MethodDelete, base+"/approval-grants/"+ruleID, nil); rec.Code != http.StatusOK {
		t.Fatalf("creator revoke: %d %s", rec.Code, rec.Body.String())
	}
	wantApprovalError(t, doJSON(as(room.Owner), http.MethodDelete, base+"/approval-grants/apr_missing", nil), http.StatusNotFound, "approval_grant_not_found")
}

func TestApprovalGrantEntitlementOffAndZDRLive(t *testing.T) {
	t.Run("entitlement off refuses create but not revoke or list", func(t *testing.T) {
		room, repo, exec := seedApprovalRoom(t, "ws10s2i", nil)
		base := "/api/v1/internal/spaces/" + room.Room
		as := func(user string) *gin.Engine { return liveApprovalEngine(t, repo, nil, room.Org, user) }
		if rec := doJSON(as(room.Manager), http.MethodPut, base+"/approval-policy", ceilingBody("always", 0)); rec.Code != http.StatusOK {
			t.Fatalf("raise ceiling: %d %s", rec.Code, rec.Body.String())
		}
		rec := doJSON(as(room.Manager), http.MethodPost, "/api/v1/internal/spaces/approval-grants", createGrantBody(room, "off-key-0000001"))
		if rec.Code != http.StatusOK {
			t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
		}
		var created struct {
			Data struct {
				Grant spaces.ApprovalRule `json:"grant"`
			} `json:"data"`
		}
		_ = json.Unmarshal(rec.Body.Bytes(), &created)
		exec(`UPDATE space_effect_policies SET approval_grant_entitled=FALSE WHERE org_id=$1`, room.Org)
		wantApprovalError(t, doJSON(as(room.Manager), http.MethodPost, "/api/v1/internal/spaces/approval-grants", createGrantBody(room, "off-key-0000002")),
			http.StatusForbidden, "approval_grants_not_entitled")
		// Tightening keeps working with the entitlement off.
		if rec := doJSON(as(room.Manager), http.MethodGet, base+"/approval-grants", nil); rec.Code != http.StatusOK {
			t.Fatalf("list with entitlement off: %d %s", rec.Code, rec.Body.String())
		}
		if rec := doJSON(as(room.Manager), http.MethodDelete, base+"/approval-grants/"+created.Data.Grant.RuleID, nil); rec.Code != http.StatusOK {
			t.Fatalf("revoke with entitlement off: %d %s", rec.Code, rec.Body.String())
		}
		if rec := doJSON(as(room.Manager), http.MethodPut, base+"/approval-policy", ceilingBody("once", 1)); rec.Code != http.StatusOK {
			t.Fatalf("tighten with entitlement off: %d %s", rec.Code, rec.Body.String())
		}
		wantApprovalError(t, doJSON(as(room.Manager), http.MethodPut, base+"/approval-policy", ceilingBody("always", 2)),
			http.StatusForbidden, "approval_grants_not_entitled")
	})
	t.Run("zero data retention refuses create with 412", func(t *testing.T) {
		room, repo, _ := seedApprovalRoom(t, "ws10s2j", func(p *spaces.EffectPolicy) { p.ZeroDataRetention = true })
		as := liveApprovalEngine(t, repo, nil, room.Org, room.Manager)
		// The ceiling write is a loosening, so ZDR refuses it too.
		wantApprovalError(t, doJSON(as, http.MethodPut, "/api/v1/internal/spaces/"+room.Room+"/approval-policy", ceilingBody("always", 0)),
			http.StatusPreconditionFailed, "zdr_durable_mutation_forbidden")
		wantApprovalError(t, doJSON(as, http.MethodPost, "/api/v1/internal/spaces/approval-grants", createGrantBody(room, "zdr-key-0000001")),
			http.StatusPreconditionFailed, "zdr_durable_mutation_forbidden")
	})
}

// liveRunAuthority reads the requester's current authority so the fake run
// binding matches what Control resolves, then lets a test perturb it.
func liveRunAuthority(t *testing.T, repo *spaces.Repository, room w2Room, user string) spaces.RunActionAuthority {
	t.Helper()
	evidence, err := repo.ResolveAgentActionDecisionEvidence(context.Background(), room.Room, room.Org, user)
	if err != nil {
		t.Fatalf("resolve agent action evidence: %v", err)
	}
	return spaces.RunActionAuthority{
		RunID: "run-1", OrgID: room.Org, SubjectID: user, ThreadID: "thread-1", SpaceRef: room.Room,
		RecipientAudienceRef: evidence.RecipientAudienceRef, RecipientAudienceRevision: evidence.Membership.Revisions.RecipientAudience,
		RecipientAudienceHash: evidence.RecipientAudienceHash, PrivacyPolicyRef: evidence.Privacy.PolicyRef,
		RunContextAuthorizationRef: "control:" + room.Room + ":thread-create:" + strconv.FormatInt(evidence.Membership.Revisions.Authority, 10),
		AuthorityRevision:          evidence.Membership.Revisions.Authority, RunStatus: "running",
	}
}

func gateBody(room w2Room) map[string]any {
	body := validApprovalGateBody()
	body["org_id"], body["agent_subject_id"] = room.Org, room.Agent
	return body
}

func TestApprovalGateRoutesLive(t *testing.T) {
	room, repo, exec := seedApprovalRoom(t, "ws10s2k", nil)
	manager := liveApprovalEngine(t, repo, nil, room.Org, room.Manager)
	base := "/api/v1/internal/spaces/" + room.Room
	if rec := doJSON(manager, http.MethodPut, base+"/approval-policy", ceilingBody("always", 0)); rec.Code != http.StatusOK {
		t.Fatalf("raise ceiling: %d %s", rec.Code, rec.Body.String())
	}
	authority := liveRunAuthority(t, repo, room, room.Editor)
	gate := func(scope string) *gin.Engine {
		return approvalHarness(t, repo, fakeRunAuthority{authority: authority}, executionCoreContext(scope))
	}
	body := gateBody(room)
	body["schema_hash"] = "sha256:" + strings.Repeat("a", 64)

	// No rule yet: 404 no_applicable_grant, and the gate view says rules can apply.
	wantApprovalError(t, doJSON(gate(spaceApprovalGrantUseScope), http.MethodPost, approvalUsePath, body), http.StatusNotFound, "no_applicable_grant")
	var view struct {
		Data struct {
			Enabled bool   `json:"standing_rules_enabled"`
			Reason  string `json:"standing_rules_disabled_reason"`
			Scope   string `json:"max_grant_scope"`
		} `json:"data"`
	}
	rec := doJSON(gate(spaceToolGateViewScope), http.MethodPost, toolGateViewPath, body)
	if rec.Code != http.StatusOK {
		t.Fatalf("gate view: %d %s", rec.Code, rec.Body.String())
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &view)
	if !view.Data.Enabled || view.Data.Reason != "" || view.Data.Scope != "always" {
		t.Fatalf("gate view = %+v", view.Data)
	}

	// Create a rule, then use it: a signed token Control itself can verify.
	create := createGrantBody(room, "gate-key-0000001")
	create["action_schema_hash"] = body["schema_hash"]
	if rec := doJSON(manager, http.MethodPost, "/api/v1/internal/spaces/approval-grants", create); rec.Code != http.StatusOK {
		t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
	}
	rec = doJSON(gate(spaceApprovalGrantUseScope), http.MethodPost, approvalUsePath, body)
	if rec.Code != http.StatusOK {
		t.Fatalf("use: %d %s", rec.Code, rec.Body.String())
	}
	var used struct {
		Data struct {
			Token string `json:"token"`
			Scope string `json:"scope"`
		} `json:"data"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &used)
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		t.Fatalf("signing key: %v", err)
	}
	claims, err := spaces.VerifyApprovalGrantUse(key, used.Data.Token, time.Now().UTC())
	if err != nil || claims.SubjectID != room.Editor || claims.AgentSubjectID != room.Agent || claims.RunID != "run-1" || used.Data.Scope != "always" {
		t.Fatalf("token claims = %+v err=%v", claims, err)
	}
	// A schema change is no longer covered.
	changed := gateBody(room)
	changed["schema_hash"] = "sha256:" + strings.Repeat("b", 64)
	wantApprovalError(t, doJSON(gate(spaceApprovalGrantUseScope), http.MethodPost, approvalUsePath, changed), http.StatusNotFound, "no_applicable_grant")

	// Continuation: authorized now, authority_changed once the requester leaves.
	if rec := doJSON(gate(spaceApprovalContinuationScope), http.MethodPost, approvalContinuePath, body); rec.Code != http.StatusOK ||
		!strings.Contains(rec.Body.String(), `"authorized":true`) {
		t.Fatalf("continuation before removal: %d %s", rec.Code, rec.Body.String())
	}
	// Entitlement off: the use route reports grants_not_entitled.
	exec(`UPDATE space_effect_policies SET approval_grant_entitled=FALSE WHERE org_id=$1`, room.Org)
	wantApprovalError(t, doJSON(gate(spaceApprovalGrantUseScope), http.MethodPost, approvalUsePath, body), http.StatusForbidden, "grants_not_entitled")
	exec(`UPDATE space_memberships SET active=FALSE WHERE space_ref=$1 AND subject_id=$2`, room.Room, room.Editor)
	wantApprovalError(t, doJSON(gate(spaceApprovalContinuationScope), http.MethodPost, approvalContinuePath, body), http.StatusForbidden, "authority_changed")
}

func TestToolGateViewReportsStandingRulesDisabledUnderZDRLive(t *testing.T) {
	room, repo, _ := seedApprovalRoom(t, "ws10s2l", func(p *spaces.EffectPolicy) { p.ZeroDataRetention = true })
	authority := liveRunAuthority(t, repo, room, room.Editor)
	engine := approvalHarness(t, repo, fakeRunAuthority{authority: authority}, executionCoreContext(spaceToolGateViewScope))
	rec := doJSON(engine, http.MethodPost, toolGateViewPath, gateBody(room))
	if rec.Code != http.StatusOK {
		t.Fatalf("gate view under ZDR must still answer: %d %s", rec.Code, rec.Body.String())
	}
	var view struct {
		Data struct {
			Enabled bool   `json:"standing_rules_enabled"`
			Reason  string `json:"standing_rules_disabled_reason"`
			Scope   string `json:"max_grant_scope"`
		} `json:"data"`
	}
	_ = json.Unmarshal(rec.Body.Bytes(), &view)
	if view.Data.Enabled || view.Data.Reason != string(spaces.DenialZDRForbidsEffect) || view.Data.Scope != "once" {
		t.Fatalf("gate view under ZDR = %+v, want rules disabled with reason %s and ceiling once", view.Data, spaces.DenialZDRForbidsEffect)
	}
}
