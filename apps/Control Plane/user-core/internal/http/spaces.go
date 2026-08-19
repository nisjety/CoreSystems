package http

import (
	"crypto/rand"
	"encoding/base64"
	"errors"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
)

const applicationSpaceLifecyclePrincipal = "application-space-lifecycle"
const verevonGatewayPrincipal = "verevon-gateway"
const controlSpacePolicyPrincipal = "control-space-policy"
const importsCorePrincipal = "imports-core"
const capabilityCorePrincipal = "capability-core"
const orchestratorCorePrincipal = "orchestrator-core"
const executionCorePrincipal = "execution-core"
const conversationCorePrincipal = "conversation-core"

// requireSpaceLifecycleRegistrar admits only the narrowly-scoped Application
// lifecycle workload. A general service credential, bearer token, or forged
// `service_id` never establishes registration authority.
func (s *Server) requireSpaceLifecycleRegistrar(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != applicationSpaceLifecyclePrincipal {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Application Space lifecycle principal required"})
		return
	}
	c.Next()
}

// requireSpaceMembershipWriter is separate from registration and from audience
// publication for the same reason those are separate from each other: writing
// a roster decides who can reach a Space's content, which is a strictly larger
// power than creating an empty one. A deployment can grant the organization
// sync this scope without also letting that workload register Spaces or
// publish recipient audiences.
func (s *Server) requireSpaceMembershipWriter(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != applicationSpaceLifecyclePrincipal ||
		!hasServiceScope(c, "spaces:membership:write") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Application Space membership writer required"})
		return
	}
	c.Next()
}

// requireSpaceAudiencePublisher is deliberately separate from lifecycle
// registration. A deployment can rotate/revoke the participant-set publisher
// without giving another workload the ability to create registered Spaces.
func (s *Server) requireSpaceAudiencePublisher(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != applicationSpaceLifecyclePrincipal ||
		!hasServiceScope(c, "spaces:audience:publish") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Application Space audience publisher required"})
		return
	}
	c.Next()
}

// requireSpaceDeletionAuthorizer is separate from registration publication:
// an Application worker may submit an immutable user-validated request, but
// only Control decides whether current owner, policy, and legal-hold facts let
// that request fence a Space.
func (s *Server) requireSpaceDeletionAuthorizer(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != applicationSpaceLifecyclePrincipal ||
		!hasServiceScope(c, "spaces:deletion:authorize") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Application Space deletion authorizer required"})
		return
	}
	c.Next()
}

// requireVerifiedSpaceResolver accepts only the same gateway workload with a
// cryptographically signed User Core delegation. The gateway's authenticated
// session supplies the actor; User Core verifies that the delegated subject
// and organization are bound to the request, so URL or body fields cannot
// impersonate another user. It returns membership facts, not a decision and
// never gives the gateway authority to manufacture privacy, audience, or
// resource claims.
func (s *Server) requireVerifiedSpaceResolver(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != verevonGatewayPrincipal ||
		!c.GetBool("delegation_verified") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "signed gateway delegation required"})
		return
	}
	c.Next()
}

func (s *Server) requireSpacePolicyWriter(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != controlSpacePolicyPrincipal ||
		!hasServiceScope(c, "spaces:policy:write") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Control Space policy principal required"})
		return
	}
	c.Next()
}

// requireSpaceImportReauthorizer admits only Imports Core's dedicated service
// credential. It deliberately does not accept a gateway delegation: the worker
// must re-resolve current Control evidence at effect time from its immutable,
// non-secret intent, not replay an ingress bearer.
func (s *Server) requireSpaceImportReauthorizer(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != importsCorePrincipal {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Imports Core Space reauthorization principal required"})
		return
	}
	c.Next()
}

// requireSpaceScheduleFireReauthorizer gives only Capability Core's scheduler
// identity access to the fire-time decision endpoint. Creating or editing a
// cron record does not grant this scope; it is a narrowly-scoped service
// capability that still receives a freshly resolved, one-fire decision.
func (s *Server) requireSpaceScheduleFireReauthorizer(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != capabilityCorePrincipal ||
		!hasServiceScope(c, "spaces:schedule:reauthorize") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Capability Core schedule reauthorization principal required"})
		return
	}
	c.Next()
}

// requireSpaceScheduledRunExecutor gives only Orchestrator Core a fresh,
// effect-time decision for a run Capability Core has already prepared. It is
// deliberately distinct from the scheduler's reauthorization capability.
func (s *Server) requireSpaceScheduledRunExecutor(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != orchestratorCorePrincipal ||
		!hasServiceScope(c, "spaces:schedule:execute") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Orchestrator Core scheduled-run executor principal required"})
		return
	}
	c.Next()
}

// requireSpaceScheduledStepExecutor is narrower than scheduled-run start:
// Orchestrator receives a fresh decision for each deterministic turn and may
// not reuse the run-creation credential as Model execution authority.
func (s *Server) requireSpaceScheduledStepExecutor(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != orchestratorCorePrincipal ||
		!hasServiceScope(c, "spaces:schedule:step") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Orchestrator Core scheduled-step principal required"})
		return
	}
	c.Next()
}

// requireSpaceAgentActionAuthorizer lets only the execution workload request a
// fresh Control decision for a run it is actively supervising. It cannot name
// a subject or target resource: Control resolves the former from Session Core,
// and the owning plane authorizes the latter at effect time.
func (s *Server) requireSpaceAgentActionAuthorizer(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != executionCorePrincipal ||
		!hasServiceScope(c, "spaces:agent-action:reauthorize") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Execution Core agent action authorizer required"})
		return
	}
	c.Next()
}

// requireSpaceAgentActionViewer lets only Execution Core request the view-only
// model catalog binding. It is strictly narrower than the payload-bound effect
// decision scope and neither Capability Core nor a browser can mint a view.
func (s *Server) requireSpaceAgentActionViewer(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != executionCorePrincipal ||
		!hasServiceScope(c, "spaces:agent-action:view") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Execution Core model action viewer required"})
		return
	}
	c.Next()
}

// requireCurrentRunActionAuthorityChecker is deliberately separate from the
// execution-time decision issuer. Conversation Core gets only the ability to
// re-check Control's already-signed, non-secret claims immediately before its
// own owner-plane effect; it cannot mint a decision or bypass that owner
// resource authorization.
func (s *Server) requireCurrentRunActionAuthorityChecker(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != conversationCorePrincipal ||
		!hasServiceScope(c, "spaces:agent-action:current-authority") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Conversation Core current agent authority checker required"})
		return
	}
	c.Next()
}

// requireOwnerEffectReservationCoordinator is the only Control reservation
// caller. Conversation Core may reserve/commit one already signed decision;
// it cannot issue a decision, widen an owner grant, or ask for another
// workload's reservation.
func (s *Server) requireOwnerEffectReservationCoordinator(c *gin.Context) {
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != conversationCorePrincipal ||
		!hasServiceScope(c, "spaces:agent-action:reservation") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "Conversation Core owner effect reservation principal required"})
		return
	}
	c.Next()
}

func (s *Server) registerSpace(c *gin.Context) {
	var registration spaces.Registration
	if err := c.ShouldBindJSON(&registration); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space registration"})
		return
	}
	if err := registration.Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	registered, err := s.spaceRepo.Register(c.Request.Context(), registration)
	if err != nil {
		if strings.Contains(err.Error(), "different organization, kind, or owner") || errors.Is(err, spaces.ErrInactiveOwnerMembership) {
			c.JSON(http.StatusConflict, gin.H{"error": "Space reference conflicts with registered authority"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Space registration failed"})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": registered})
}

// spaceRoster answers who is in a Space, to someone who is in it.
func (s *Server) spaceRoster(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	members, err := s.spaceRepo.RosterForSpace(
		c.Request.Context(), c.Param("space_ref"), c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Space roster resolution failed"})
		return
	}
	// A caller who is not a member gets an empty roster from the query, which
	// is returned as 404 rather than an empty room: "you cannot see this" and
	// "nobody is here" are different facts, and every Space has at least an
	// owner.
	if len(members) == 0 {
		c.JSON(http.StatusNotFound, gin.H{"error": "Space roster not available"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"members": members, "count": len(members)}})
}

// listSpacesForSubject is the actor-filtered Space index.
//
// The acting subject comes from the verified delegation the `spaces:resolve`
// scope already requires, never from a query parameter — an index keyed on a
// caller-supplied id is an enumeration endpoint.
func (s *Server) listSpacesForSubject(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	entries, err := s.spaceRepo.SpacesForSubject(c.Request.Context(), c.GetString("org_id"), c.GetString("user_id"))
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Space index resolution failed"})
		return
	}
	// An empty index is a valid answer, not an error: a subject can legitimately
	// belong to no registered Space yet.
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"spaces": entries, "count": len(entries)}})
}

// replaceSpaceMemberships converges a Space's roster on the declared set.
//
// The Space reference comes from the path, not the body: the route is already
// the resource, and accepting a second copy in the payload would let the two
// disagree.
func (s *Server) replaceSpaceMemberships(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var replacement spaces.MembershipReplacement
	if err := c.ShouldBindJSON(&replacement); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space membership replacement"})
		return
	}
	replacement.SpaceRef = strings.TrimSpace(c.Param("space_ref"))
	revisions, err := s.spaceRepo.ReplaceMemberships(c.Request.Context(), replacement)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusNotFound, gin.H{"error": "registered Space not found"})
		return
	}
	if err != nil {
		// A rejected roster is the caller's contract error, not a server fault:
		// an unknown role, a duplicate subject, a personal Space, or a Space
		// that is not registered active. Returning 500 for those would send an
		// at-least-once caller into a retry loop over an unfixable request.
		c.JSON(http.StatusUnprocessableEntity, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"space_ref": replacement.SpaceRef,
		"members":   len(replacement.Members),
		"revisions": revisions,
	}})
}

func (s *Server) registerRecipientAudience(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var registration spaces.RecipientAudienceRegistration
	if err := c.ShouldBindJSON(&registration); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid recipient audience registration"})
		return
	}
	registered, err := s.spaceRepo.RegisterRecipientAudience(c.Request.Context(), registration)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusNotFound, gin.H{"error": "registered Space not found"})
		return
	}
	if err != nil {
		c.JSON(http.StatusConflict, gin.H{"error": "recipient audience registration rejected"})
		return
	}
	c.JSON(http.StatusCreated, gin.H{"data": registered})
}

func (s *Server) authorizeSpaceDeletion(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request spaces.DeletionAuthorizationRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space deletion request"})
		return
	}
	receipt, err := s.spaceRepo.AuthorizeDeletion(c.Request.Context(), request)
	if err != nil {
		if strings.Contains(err.Error(), "is required") || strings.Contains(err.Error(), "unknown") {
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space deletion request"})
			return
		}
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space deletion authorization unavailable"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": receipt})
}

func (s *Server) upsertSpaceDeletionPolicy(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var policy spaces.DeletionPolicy
	if err := c.ShouldBindJSON(&policy); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space deletion policy"})
		return
	}
	updated, err := s.spaceRepo.UpsertDeletionPolicy(c.Request.Context(), policy, c.GetString("service_id"))
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space deletion policy unavailable"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"updated": updated}})
}

func (s *Server) applySpaceLegalHold(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var hold spaces.LegalHold
	if err := c.ShouldBindJSON(&hold); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space legal hold"})
		return
	}
	hold.SpaceRef = c.Param("space_ref")
	updated, err := s.spaceRepo.ApplyLegalHold(c.Request.Context(), hold, c.GetString("service_id"))
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusNotFound, gin.H{"error": "registered Space not found"})
		return
	}
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space legal hold"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"updated": updated}})
}

func (s *Server) releaseSpaceLegalHold(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	updated, err := s.spaceRepo.ReleaseLegalHold(c.Request.Context(), c.Param("space_ref"), c.GetString("service_id"))
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusNotFound, gin.H{"error": "registered Space not found"})
		return
	}
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space legal hold release"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"updated": updated}})
}

func (s *Server) resolveCurrentSpaceMembership(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(
		c.Request.Context(), c.Param("space_ref"), c.GetString("org_id"), c.GetString("user_id"),
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space membership required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "Space membership resolution failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": membership})
}

type personalThreadDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	SessionKey     string `json:"session_key"`
	IdempotencyKey string `json:"idempotency_key"`
}

// threadAppendDecisionRequest has only the identifiers needed to bind the
// target effect. Content is represented by a SHA-256 commitment, so Control
// does not become a transcript processor and an old create decision cannot be
// substituted for a later append.
type threadAppendDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	ThreadID       string `json:"thread_id"`
	ContentDigest  string `json:"content_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issuePersonalThreadDecision is a gateway-only, delegated issuance surface.
// The body deliberately contains no actor, organization, role, audience,
// privacy, resource, action, schema, digest, decision reference, or nonce:
// those are resolved or minted by Control. A missing policy/key/membership is
// unavailable or forbidden; it can never turn into a permissive decision.
func (s *Server) issuePersonalThreadDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request personalThreadDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.IdempotencyKey) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref and idempotency_key are required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"),
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssuePersonalThreadCreateDecision(evidence, spaces.PersonalThreadDecisionRequest{
		DecisionRef: decisionRef, SessionKey: request.SessionKey, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "personal Space thread creation is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// issueThreadDecision chooses a Control-owned personal or shared evidence
// resolver from the registered Space kind. The gateway supplies only the
// selected Space/session/idempotency tuple under signed user delegation;
// recipients, policy, resource authorization, revisions, nonce, and digest
// are all resolved or minted here.
func (s *Server) issueThreadDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request personalThreadDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.IdempotencyKey) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref and idempotency_key are required"})
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority unavailable"})
		return
	}
	var evidence spaces.PersonalThreadDecisionEvidence
	if membership.Kind == spaces.KindPersonal {
		evidence, err = s.spaceRepo.ResolvePersonalThreadDecisionEvidence(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	} else {
		evidence, err = s.spaceRepo.ResolveSharedThreadDecisionEvidence(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	}
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "current recipient audience authority required"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	issuerRequest := spaces.PersonalThreadDecisionRequest{DecisionRef: decisionRef, SessionKey: request.SessionKey, IdempotencyKey: request.IdempotencyKey, Nonce: nonce}
	var decision spaces.Decision
	if membership.Kind == spaces.KindPersonal {
		decision, err = spaces.IssuePersonalThreadCreateDecision(evidence, issuerRequest, time.Now().UTC())
	} else {
		decision, err = spaces.IssueSharedThreadCreateDecision(evidence, issuerRequest, time.Now().UTC())
	}
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space thread creation is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	// space_kind travels in the envelope, not the signed decision, for any
	// caller that wants to branch on it (e.g. UI copy); it is not needed to
	// choose a follow-up authority anymore — retrieval-decision now resolves
	// personal vs shared itself from Control state, the same way this
	// handler does, so a caller never needs to know or claim the kind to get
	// a correctly-scoped retrieval decision. Verifiers of the signed token
	// itself must still keep deriving kind from Control state, never a claim.
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token, "space_kind": membership.Kind}})
}

// issueThreadAppendDecision resolves fresh current evidence for every append.
// The service does not trust the browser to choose actor, audience, revision,
// policy, or effect digest; all except the content commitment are derived from
// the signed gateway delegation and Control's current authority state.
func (s *Server) issueThreadAppendDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request threadAppendDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.ThreadID) == "" || strings.TrimSpace(request.ContentDigest) == "" || strings.TrimSpace(request.IdempotencyKey) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref, thread_id, content_digest, and idempotency_key are required"})
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority unavailable"})
		return
	}
	var evidence spaces.PersonalThreadDecisionEvidence
	if membership.Kind == spaces.KindPersonal {
		evidence, err = s.spaceRepo.ResolvePersonalThreadDecisionEvidence(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	} else {
		evidence, err = s.spaceRepo.ResolveSharedThreadDecisionEvidence(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	}
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "current recipient audience authority required"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssueThreadAppendDecision(evidence, spaces.ThreadAppendDecisionRequest{
		DecisionRef: decisionRef, ThreadID: request.ThreadID, ContentDigest: request.ContentDigest,
		IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space thread append is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

type personalRetrievalDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issuePersonalRetrievalDecision is deliberately distinct from thread
// creation. It can only be reached through the verified gateway delegation,
// and Control derives all resource, audience, privacy, and entitlement claims
// from current state before Data sees the signed result.
func (s *Server) issuePersonalRetrievalDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request personalRetrievalDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.IdempotencyKey) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref and idempotency_key are required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalRetrievalDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"),
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space retrieval authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssuePersonalRetrievalDecision(evidence, spaces.PersonalRetrievalDecisionRequest{
		DecisionRef: decisionRef, IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "personal Space retrieval is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// issueRetrievalDecision chooses a Control-owned personal or shared retrieval
// evidence resolver from the registered Space kind, the same dispatch
// issueThreadDecision uses for thread creation. personal-retrieval-decision
// remains for any existing caller of that narrower path; the gateway should
// call this one instead so a Space's own kind — not a caller's own guess —
// decides which retrieval authority gets resolved.
func (s *Server) issueRetrievalDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request personalRetrievalDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.IdempotencyKey) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref and idempotency_key are required"})
		return
	}
	membership, err := s.spaceRepo.ResolveCurrentUserMembership(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority unavailable"})
		return
	}
	var evidence spaces.PersonalThreadDecisionEvidence
	if membership.Kind == spaces.KindPersonal {
		evidence, err = s.spaceRepo.ResolvePersonalRetrievalDecisionEvidence(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	} else {
		evidence, err = s.spaceRepo.ResolveSharedRetrievalDecisionEvidence(c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"))
	}
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "current retrieval authority required"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	issuerRequest := spaces.PersonalRetrievalDecisionRequest{DecisionRef: decisionRef, IdempotencyKey: request.IdempotencyKey, Nonce: nonce}
	var decision spaces.Decision
	if membership.Kind == spaces.KindPersonal {
		decision, err = spaces.IssuePersonalRetrievalDecision(evidence, issuerRequest, time.Now().UTC())
	} else {
		decision, err = spaces.IssueSharedRetrievalDecision(evidence, issuerRequest, time.Now().UTC())
	}
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space retrieval is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

type personalImportDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	IdempotencyKey string `json:"idempotency_key"`
	SourceType     string `json:"source_type"`
}

type scheduleCreateDecisionRequest struct {
	SpaceRef       string `json:"space_ref"`
	ScheduleID     string `json:"schedule_id"`
	TemplateDigest string `json:"template_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issueScheduleCreateDecision is gateway-delegated. The authenticated user is
// resolved from the verified delegation; the browser cannot select a creator,
// authority revision, audience, privacy policy, or model service audience.
func (s *Server) issueScheduleCreateDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request scheduleCreateDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.ScheduleID) == "" || strings.TrimSpace(request.TemplateDigest) == "" || strings.TrimSpace(request.IdempotencyKey) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref, schedule_id, template_digest, and idempotency_key are required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"),
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space schedule authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssueScheduleCreateDecision(evidence, spaces.ScheduleCreateRequest{
		DecisionRef: decisionRef, ScheduleID: request.ScheduleID, TemplateDigest: request.TemplateDigest,
		IdempotencyKey: request.IdempotencyKey, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space schedule creation is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// issuePersonalImportDecision is an independently scoped durable-write grant.
// It never accepts a browser-selected actor, document target, privacy claim,
// or resource reference. The separate Ingestion/Data owners must still
// reauthorize a queued job immediately before each durable write.
func (s *Server) issuePersonalImportDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request personalImportDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil || strings.TrimSpace(request.SpaceRef) == "" || strings.TrimSpace(request.IdempotencyKey) == "" || strings.TrimSpace(request.SourceType) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "space_ref, source_type, and idempotency_key are required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalImportDecisionEvidence(
		c.Request.Context(), request.SpaceRef, c.GetString("org_id"), c.GetString("user_id"),
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space import authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssuePersonalImportDecision(evidence, spaces.PersonalImportDecisionRequest{
		DecisionRef: decisionRef, IdempotencyKey: request.IdempotencyKey, SourceType: request.SourceType, Nonce: nonce,
	}, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "personal Space import is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

type personalImportExecutionDecisionRequest struct {
	Intent personalImportExecutionIntent `json:"intent"`
}

// scheduleFireDecisionRequest is an immutable, non-secret record view from
// Capability Core. The caller cannot include any policy, audience, role, or
// previously issued token. Control resolves those facts just before fire.
type scheduleFireDecisionRequest struct {
	Intent scheduleFireIntent `json:"intent"`
}

type scheduleFireIntent struct {
	OrgID          string `json:"org_id"`
	SpaceRef       string `json:"space_ref"`
	SubjectID      string `json:"subject_id"`
	ScheduleID     string `json:"schedule_id"`
	FireKey        string `json:"fire_key"`
	TemplateDigest string `json:"template_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

type scheduledRunDecisionRequest struct {
	Intent scheduledRunIntent `json:"intent"`
}

type scheduledRunExecutionDecisionRequest struct {
	Intent struct {
		scheduledRunIntent
		ThreadID string `json:"thread_id"`
	} `json:"intent"`
}

type scheduledStepDecisionRequest struct {
	Intent scheduledStepIntent `json:"intent"`
}

type scheduledStepIntent struct {
	OrgID          string `json:"org_id"`
	SpaceRef       string `json:"space_ref"`
	SubjectID      string `json:"subject_id"`
	RunID          string `json:"run_id"`
	ThreadID       string `json:"thread_id"`
	ScheduleID     string `json:"schedule_id"`
	FireKey        string `json:"fire_key"`
	TemplateDigest string `json:"template_digest"`
	StepID         string `json:"step_id"`
	StepIndex      uint32 `json:"step_index"`
	PolicyDigest   string `json:"policy_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

type scheduledRunIntent struct {
	OrgID          string `json:"org_id"`
	SpaceRef       string `json:"space_ref"`
	SubjectID      string `json:"subject_id"`
	ScheduleID     string `json:"schedule_id"`
	FireKey        string `json:"fire_key"`
	TaskID         string `json:"task_id"`
	TemplateDigest string `json:"template_digest"`
	IdempotencyKey string `json:"idempotency_key"`
}

// issueScheduleFireDecision is deliberately worker-only. It re-checks current
// personal-Space authority before a claimed cron fire and issues an expiring,
// target-bound decision for just that fire. A durable schedule's original
// creation decision is never accepted here.
func (s *Server) issueScheduleFireDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request scheduleFireDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid schedule fire intent is required"})
		return
	}
	intent := spaces.ScheduleFireIntent{
		OrgID: request.Intent.OrgID, SpaceRef: request.Intent.SpaceRef, SubjectID: request.Intent.SubjectID,
		ScheduleID: request.Intent.ScheduleID, FireKey: request.Intent.FireKey,
		TemplateDigest: request.Intent.TemplateDigest, IdempotencyKey: request.Intent.IdempotencyKey,
	}
	if err := intent.Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid schedule fire intent is required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(
		c.Request.Context(), intent.SpaceRef, intent.OrgID, intent.SubjectID,
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space schedule authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssueScheduleFireDecision(evidence, intent, decisionRef, nonce, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space schedule fire is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// issueScheduledRunDecision authorizes only preparation of the service-owned
// thread/run for one already-claimed fire. It is not user delegation and the
// returned bearer must travel only on Capability Core's direct Session Core RPC.
func (s *Server) issueScheduledRunDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request scheduledRunDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid scheduled run intent is required"})
		return
	}
	intent := spaces.ScheduledRunIntent{
		OrgID: request.Intent.OrgID, SpaceRef: request.Intent.SpaceRef, SubjectID: request.Intent.SubjectID,
		ScheduleID: request.Intent.ScheduleID, FireKey: request.Intent.FireKey, TaskID: request.Intent.TaskID,
		TemplateDigest: request.Intent.TemplateDigest, IdempotencyKey: request.Intent.IdempotencyKey,
	}
	if err := intent.Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid scheduled run intent is required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(c.Request.Context(), intent.SpaceRef, intent.OrgID, intent.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space scheduled run authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssueScheduledRunDecision(evidence, intent, decisionRef, nonce, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space scheduled run is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"decision": decision, "token": token,
		"system_thread_key": intent.SystemThreadKey(), "run_id": intent.TaskID,
	}})
}

// issueScheduledRunExecutionDecision rechecks current owner authority at the
// Session Core run-creation boundary. Its bearer is direct-hop only and never
// belongs in Temporal history, task rows, or events.
func (s *Server) issueScheduledRunExecutionDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request scheduledRunExecutionDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid scheduled run execution intent is required"})
		return
	}
	intent := spaces.ScheduledRunIntent{
		OrgID: request.Intent.OrgID, SpaceRef: request.Intent.SpaceRef, SubjectID: request.Intent.SubjectID,
		ScheduleID: request.Intent.ScheduleID, FireKey: request.Intent.FireKey, TaskID: request.Intent.TaskID,
		TemplateDigest: request.Intent.TemplateDigest, IdempotencyKey: request.Intent.IdempotencyKey,
	}
	if err := intent.Validate(); err != nil || strings.TrimSpace(request.Intent.ThreadID) == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid scheduled run execution intent is required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(c.Request.Context(), intent.SpaceRef, intent.OrgID, intent.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space scheduled run authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssueScheduledRunExecutionDecision(evidence, intent, request.Intent.ThreadID, decisionRef, nonce, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "Space scheduled run execution is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// issueScheduledStepDecision refreshes the full current Space authority for a
// single turn. The signed envelope is target-bound to Execution Core and is
// returned only to the authenticated Orchestrator workload; it is never a
// user delegation and never authorizes an owner-plane effect by itself.
func (s *Server) issueScheduledStepDecision(c *gin.Context) {
	if s.spaceRepo == nil || s.scheduledStepAuthority == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request scheduledStepDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid scheduled step intent is required"})
		return
	}
	intent := spaces.ScheduledStepIntent{
		OrgID: request.Intent.OrgID, SpaceRef: request.Intent.SpaceRef,
		RunID: request.Intent.RunID, ThreadID: request.Intent.ThreadID, ScheduleID: request.Intent.ScheduleID,
		FireKey: request.Intent.FireKey, TemplateDigest: request.Intent.TemplateDigest,
		StepID: request.Intent.StepID, StepIndex: request.Intent.StepIndex,
		PolicyDigest: request.Intent.PolicyDigest, IdempotencyKey: request.Intent.IdempotencyKey,
	}
	if err := intent.ValidateAuthorityRequest(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid scheduled step intent is required"})
		return
	}
	prepared, err := s.scheduledStepAuthority.ResolveScheduledStepAuthority(c.Request.Context(), intent)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "prepared scheduled step authority unavailable"})
		return
	}
	if prepared.OrgID != intent.OrgID || prepared.SpaceRef != intent.SpaceRef ||
		prepared.RunID != intent.RunID || prepared.ThreadID != intent.ThreadID ||
		prepared.ScheduleID != intent.ScheduleID || prepared.FireKey != intent.FireKey ||
		prepared.TemplateDigest != intent.TemplateDigest || prepared.StepID != intent.StepID ||
		prepared.StepIndex != intent.StepIndex || prepared.PolicyDigest != intent.PolicyDigest ||
		prepared.IdempotencyKey != intent.IdempotencyKey ||
		(strings.TrimSpace(request.Intent.SubjectID) != "" && request.Intent.SubjectID != prepared.SubjectID) {
		c.JSON(http.StatusForbidden, gin.H{"error": "prepared scheduled step authority does not match intent"})
		return
	}
	// SubjectID is now sourced from Session Core's prepared-run metadata, never
	// from the Orchestrator request body.
	intent.SubjectID = prepared.SubjectID
	if err := intent.Validate(); err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "prepared scheduled step authority is invalid"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalThreadDecisionEvidence(c.Request.Context(), intent.SpaceRef, intent.OrgID, intent.SubjectID)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space scheduled step authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssueScheduledStepDecision(evidence, intent, decisionRef, nonce, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "scheduled step is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

// personalImportExecutionIntent is a deliberately constrained wire view of a
// durable Imports Core record. It contains no bearer and omits mutable policy
// values: Control refreshes those from its own authority repository.
type personalImportExecutionIntent struct {
	OrgID            string `json:"org_id"`
	SpaceRef         string `json:"space_ref"`
	SubjectID        string `json:"subject_id"`
	ActionSchemaHash string `json:"action_schema_hash"`
	PayloadDigest    string `json:"payload_digest"`
	IdempotencyKey   string `json:"idempotency_key"`
	SourceType       string `json:"source_type"`
}

// issuePersonalImportExecutionDecision gives Imports Core a new short-lived
// Data-targeted decision for exactly one already-authorized job intent. The
// worker uses it immediately and never stores it. A service credential alone
// cannot substitute different current policy, resource, or audience claims.
func (s *Server) issuePersonalImportExecutionDecision(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var request personalImportExecutionDecisionRequest
	if err := c.ShouldBindJSON(&request); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid import execution intent is required"})
		return
	}
	intent := spaces.PersonalImportExecutionIntent{
		OrgID: request.Intent.OrgID, SpaceRef: request.Intent.SpaceRef,
		SubjectID: request.Intent.SubjectID, ActionSchemaHash: request.Intent.ActionSchemaHash,
		PayloadDigest: request.Intent.PayloadDigest, IdempotencyKey: request.Intent.IdempotencyKey,
		SourceType: request.Intent.SourceType,
	}
	if err := intent.Validate(); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "valid import execution intent is required"})
		return
	}
	evidence, err := s.spaceRepo.ResolvePersonalImportDecisionEvidence(
		c.Request.Context(), intent.SpaceRef, intent.OrgID, intent.SubjectID,
	)
	if errors.Is(err, spaces.ErrNoCurrentMembership) {
		c.JSON(http.StatusForbidden, gin.H{"error": "current personal Space authority required"})
		return
	}
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "personal Space import authority unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signer unavailable"})
		return
	}
	decisionRef, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	nonce, err := randomDecisionPart()
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision entropy unavailable"})
		return
	}
	decision, err := spaces.IssuePersonalImportExecutionDecision(evidence, intent, decisionRef, nonce, time.Now().UTC())
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": "personal Space import execution is not authorized"})
		return
	}
	token, err := spaces.SignDecision(key, decision)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision signing failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"decision": decision, "token": token}})
}

func (s *Server) upsertSpaceEffectPolicy(c *gin.Context) {
	if s.spaceRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	var policy spaces.EffectPolicy
	if err := c.ShouldBindJSON(&policy); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid Space effect policy"})
		return
	}
	updated, err := s.spaceRepo.UpsertEffectPolicy(c.Request.Context(), policy)
	if err != nil {
		if strings.Contains(err.Error(), "is required") {
			c.JSON(http.StatusBadRequest, gin.H{"error": "incomplete Space effect policy"})
			return
		}
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space effect policy unavailable"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"updated": updated}})
}

func randomDecisionPart() (string, error) {
	bytes := make([]byte, 24)
	if _, err := rand.Read(bytes); err != nil {
		return "", err
	}
	return base64.RawURLEncoding.EncodeToString(bytes), nil
}
