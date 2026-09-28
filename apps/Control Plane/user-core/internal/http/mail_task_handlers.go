package http

import (
	"context"
	"errors"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/mailtask"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// MailTaskUserLookup is the narrow "does this alias's owner have a verified
// email" read resolveMailTaskAlias needs. *users.Service satisfies this
// already (wired automatically in NewServer); tests substitute a fake so the
// endpoint is exercisable without a live database.
type MailTaskUserLookup interface {
	GetUser(ctx context.Context, id string) (*users.User, error)
}

// MailTaskEffectPolicyResolver is the narrow "read this org's current
// privacy/entitlement floor" read issueMailTaskThreadDecision needs.
// *spaces.Repository satisfies this already (wired automatically whenever
// SetSpaceRepository is called -- server.go); tests substitute a fake so the
// mail-task-v1 decision-minting path is exercisable without a live database.
type MailTaskEffectPolicyResolver interface {
	ResolveOrgEffectPolicy(ctx context.Context, orgID string) (spaces.EffectPolicy, error)
}

// mail_task_handlers.go: self-service alias management for the Settings page
// (GET/POST/DELETE /api/v1/users/me/mail-task-address) plus the internal
// resolve endpoint integration-corev2's inbound webhook receiver calls to
// turn an alias back into (org_id, user_id, verified_email).
//
// Off by default: MAIL_TASK_RECEIVING_DOMAIN is empty until the owner
// finishes the Postmark account + DNS setup (mail-task spec §7, owner
// decision #1). Every handler here 503s rather than inventing a domain or
// silently degrading while that is unset.

// mailTaskReceivingDomain reads the domain mail-task addresses are shown
// with (e.g. "tasks.verevon.ai"). Empty means the feature is not configured
// -- never a secret, but deliberately env-driven so it can ship dark.
func mailTaskReceivingDomain() string {
	return strings.TrimSpace(os.Getenv("MAIL_TASK_RECEIVING_DOMAIN"))
}

// SetMailTaskService wires the alias lifecycle service. A nil service (the
// zero value, and the state of any Server that never calls this) leaves
// every mail-task route fail-closed with 503 -- matching this file's other
// optional-dependency setters (e.g. SetSpaceRepository).
func (s *Server) SetMailTaskService(service *mailtask.Service) {
	s.mailTaskService = service
}

// SetMailTaskEffectPolicyResolver overrides the org-policy read
// issueMailTaskThreadDecision uses -- normally wired automatically to the
// same repository SetSpaceRepository sets, exposed separately only so tests
// can substitute a fake without a live database.
func (s *Server) SetMailTaskEffectPolicyResolver(resolver MailTaskEffectPolicyResolver) {
	s.mailTaskEffectPolicy = resolver
}

// resolveActiveOrgForMailTask verifies the session's active organization for
// a Bearer-authenticated Settings-page request.
//
// This must NOT read c.GetString("org_id"): that context key is populated
// only on the service-principal v2-delegation path (service_auth.go), which
// never applies to a plain user Bearer token, so an ordinary browser request
// would see an empty string there every time (the earlier version of this
// handler did exactly that and 400'd on every real call). It also must NOT
// trust c.GetHeader("X-Org-Id") on its own -- the gateway forwards it as a
// hint of the session's active org, but it is client-influenced and unverified
// (any caller could set it to an org they are not a member of). The correct,
// existing pattern in this file is getSessionContext's: treat X-Org-Id as a
// requested org and verify it against auth-core's canonical membership
// authority via resolveMembershipFromAuthCore before trusting it -- the same
// tenancy boundary every other org-scoped self-service endpoint here relies
// on.
func (s *Server) resolveActiveOrgForMailTask(c *gin.Context, userID string) (string, bool) {
	requestedOrg := strings.TrimSpace(c.GetHeader("X-Org-Id"))
	if requestedOrg == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "an active organization is required"})
		return "", false
	}
	orgID, _, err := s.resolveMembershipFromAuthCore(c.Request.Context(), userID, requestedOrg)
	if err != nil {
		denied, continueRequest := s.handleMembershipResolutionError(c, userID, requestedOrg, err)
		if !continueRequest {
			// handleMembershipResolutionError already wrote the response
			// (503 authority-unavailable, or 500 on a stale-projection cleanup
			// failure).
			return "", false
		}
		if denied {
			c.JSON(http.StatusForbidden, gin.H{"error": "not a member of the requested organization"})
			return "", false
		}
	}
	return orgID, true
}

// mailTaskAddress backs both GET and POST /api/v1/users/me/mail-task-address:
// both are the same idempotent get-or-create (GenerateAlias is itself
// idempotent per (org_id, user_id)), offered under both verbs so the SPA can
// use whichever reads more naturally at each call site (a passive Settings
// load vs. an explicit "Enable" action).
func (s *Server) mailTaskAddress(c *gin.Context) {
	domain := mailTaskReceivingDomain()
	if domain == "" || s.mailTaskService == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "mail-task is not configured"})
		return
	}
	userID, ok := requireContextUserID(c)
	if !ok {
		return
	}
	orgID, ok := s.resolveActiveOrgForMailTask(c, userID)
	if !ok {
		return
	}
	address, err := s.mailTaskService.GenerateAlias(c.Request.Context(), orgID, userID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to generate mail-task address"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"address": address.Alias + "@" + domain})
}

// revokeMailTaskAddress backs DELETE /api/v1/users/me/mail-task-address --
// the "Generer på nytt / Regenerate" action. It revokes the live alias (and,
// per Service.RevokeAlias, mints its replacement immediately) and returns no
// body; the SPA re-fetches with GET, matching MailTaskAddressSection.tsx's
// documented DELETE-then-GET flow.
func (s *Server) revokeMailTaskAddress(c *gin.Context) {
	domain := mailTaskReceivingDomain()
	if domain == "" || s.mailTaskService == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "mail-task is not configured"})
		return
	}
	userID, ok := requireContextUserID(c)
	if !ok {
		return
	}
	orgID, ok := s.resolveActiveOrgForMailTask(c, userID)
	if !ok {
		return
	}
	if err := s.mailTaskService.RevokeAlias(c.Request.Context(), orgID, userID); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to revoke mail-task address"})
		return
	}
	c.Status(http.StatusNoContent)
}

// resolveMailTaskAlias: GET /api/v1/internal/mail-task/resolve?alias=&provider_message_id=
//
// Internal-only -- the enclosing route group already requires a service
// principal (server.go: internal.Use(s.requireServicePrincipal)).
//
// Returns an IDENTICAL 404 body for "no such alias", "alias was revoked", and
// "the alias resolves but its owner has no verified email" -- the resolve
// endpoint must never let an inbound-webhook-reachable caller distinguish a
// live alias from a dead one (spec §6, alias-enumeration risk). The last
// case matters as much as the first two: an unverified-email owner can never
// legitimately pass the From==verified-email check the receiver applies next
// (spec §6, the single non-optional anti-spoofing control), so refusing to
// resolve here is not a shortcut, it is the same rule enforced one hop
// earlier with the same non-distinguishing response.
//
// Delegated-identity design (Gap B, 03-design-2-token-exchange.md §2.2):
// when the caller also supplies provider_message_id, the response gains a
// `thread_decision` field -- a signed mail-task-v1 decision minted from this
// SAME server-resolved (org_id, user_id), never from anything in the inbound
// email itself, that session-core's PrepareUserOwnedThread will require for
// a mail_task-origin request once the pipeline is wired (still gated off by
// MailTaskPipeline today). Minting is best-effort and additive: a ZDR org, an
// org with no ThreadCreateEntitled floor, a signer that is not configured, or
// simply an absent provider_message_id all leave thread_decision out of the
// response rather than failing the alias resolve itself -- the existing
// resolve behavior (and its non-distinguishing 404s above) is completely
// unaffected either way, matching the design's own fail-closed rollout
// (§2.8: "recommend refusing issuance until the owner decides").
func (s *Server) resolveMailTaskAlias(c *gin.Context) {
	if s.mailTaskService == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "mail-task is not configured"})
		return
	}
	alias := strings.TrimSpace(c.Query("alias"))
	if alias == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "alias is required"})
		return
	}
	address, err := s.mailTaskService.ResolveAlias(c.Request.Context(), alias)
	if err != nil {
		// mailtask.ErrNotFound and mailtask.ErrRevoked collapse to the same
		// response by construction -- neither branch inspects which one this
		// is before responding.
		c.JSON(http.StatusNotFound, gin.H{"error": "alias not found"})
		return
	}
	if s.mailTaskUserLookup == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "mail-task is not configured"})
		return
	}
	user, err := s.mailTaskUserLookup.GetUser(c.Request.Context(), address.UserID)
	if err != nil || user == nil || !user.EmailVerified || strings.TrimSpace(user.Email) == "" {
		c.JSON(http.StatusNotFound, gin.H{"error": "alias not found"})
		return
	}
	body := gin.H{
		"org_id":         address.OrgID,
		"user_id":        address.UserID,
		"verified_email": user.Email,
	}
	if providerMessageID := strings.TrimSpace(c.Query("provider_message_id")); providerMessageID != "" {
		if decisionToken, ok := s.issueMailTaskThreadDecision(c.Request.Context(), address, providerMessageID); ok {
			body["thread_decision"] = decisionToken
		}
	}
	c.JSON(http.StatusOK, body)
}

// issueMailTaskThreadDecision resolves the org's current effect policy and
// mints a mail-task-v1 thread-prepare decision. Every failure mode here is
// logged and folded into ok=false (never an error surfaced to the resolve
// caller) -- see resolveMailTaskAlias's doc comment for why.
func (s *Server) issueMailTaskThreadDecision(ctx context.Context, address mailtask.Address, providerMessageID string) (string, bool) {
	if s.mailTaskEffectPolicy == nil {
		return "", false
	}
	policy, err := s.mailTaskEffectPolicy.ResolveOrgEffectPolicy(ctx, address.OrgID)
	if err != nil {
		if !errors.Is(err, spaces.ErrNoOrgEffectPolicy) {
			log.Error().Err(err).Str("org_id", address.OrgID).Msg("mail-task thread decision: resolve effect policy")
		}
		return "", false
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		return "", false
	}
	token, err := spaces.IssueMailTaskThreadDecision(policy, key, address.OrgID, address.UserID, address.Alias, providerMessageID, time.Now())
	if err != nil {
		// ZDR / not-entitled / policy-mismatch refusals land here too --
		// deliberately not logged as errors, since "this org isn't eligible"
		// is an expected, fail-closed outcome, not a malfunction.
		return "", false
	}
	return token, true
}
