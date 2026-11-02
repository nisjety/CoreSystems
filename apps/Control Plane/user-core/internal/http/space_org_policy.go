package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strconv"
	"strings"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"
)

// Org-admin Space policy routes (WS03-S3). The gateway forwards an org
// admin's request with a signed v1 delegation; Control re-resolves the
// canonical org role from auth-core on every call (never from the gateway,
// never from the local membership projection) and fails closed.

const (
	spacePolicyAdminReadScope  = "spaces:policy:admin:read"
	spacePolicyAdminWriteScope = "spaces:policy:admin:write"
	spacePolicyAdminMaxBody    = 16 << 10
)

// spaceOrgPolicyStore is the narrow storage surface of the org-policy routes,
// so tests can inject a fake. *spaces.Repository implements it.
type spaceOrgPolicyStore interface {
	GetOrgPolicy(ctx context.Context, orgID string) (spaces.OrgPolicyView, error)
	ApplyOrgEntitlements(ctx context.Context, req spaces.ApplyOrgEntitlementsRequest) (spaces.ApplyOrgEntitlementsResult, error)
	ListOrgPolicyEvents(ctx context.Context, orgID string, limit int, cursor int64) ([]spaces.OrgPolicyEvent, int64, error)
	RecordOrgPolicyWriteDenied(ctx context.Context, orgID string, actor spaces.EffectPolicyActor, reason string) error
}

// spaceOrgPolicyConfig groups the Server state of these routes.
type spaceOrgPolicyConfig struct {
	store spaceOrgPolicyStore
	// roleResolver returns the canonical org role of (user, org). Nil means
	// the auth-core membership authority (resolveMembershipFromAuthCore).
	roleResolver func(ctx context.Context, userID, orgID string) (string, error)
	// writesEnabled is SPACE_POLICY_ADMIN_WRITES, set once at startup. It
	// gates only the write route; it is never an authorization input.
	writesEnabled bool
}

// SetSpacePolicyAdminWrites wires SPACE_POLICY_ADMIN_WRITES (strict literal
// true; off by default). Reads and history stay available when it is off.
func (s *Server) SetSpacePolicyAdminWrites(enabled bool) {
	s.spaceOrgPolicy.writesEnabled = enabled
}

// orgAdminRole is the canonical org role from auth-core. A non-member is
// ErrMembershipNotFound; any other error means the authority is unreachable.
func (s *Server) orgAdminRole(ctx context.Context, userID, orgID string) (string, error) {
	if s.spaceOrgPolicy.roleResolver != nil {
		return s.spaceOrgPolicy.roleResolver(ctx, userID, orgID)
	}
	_, role, err := s.resolveMembershipFromAuthCore(ctx, userID, orgID)
	return role, err
}

// requireSpaceOrgPolicyAdmin admits only the gateway workload with a verified
// delegation, the route's scope and a canonical owner or admin role. For a
// write it also requires the Control write switch.
func (s *Server) requireSpaceOrgPolicyAdmin(write bool) gin.HandlerFunc {
	scope := spacePolicyAdminReadScope
	if write {
		scope = spacePolicyAdminWriteScope
	}
	return func(c *gin.Context) {
		userID, orgID := strings.TrimSpace(c.GetString("user_id")), strings.TrimSpace(c.GetString("org_id"))
		if c.GetString("auth_method") != "service_principal" ||
			c.GetString("service_id") != verevonGatewayPrincipal ||
			!c.GetBool("delegation_verified") ||
			!hasServiceScope(c, scope) ||
			userID == "" || orgID == "" {
			abortTypedError(c, http.StatusForbidden, "space_org_admin_required",
				"a signed gateway delegation for an organization admin is required", nil)
			return
		}
		if s.spaceOrgPolicy.store == nil {
			abortTypedError(c, http.StatusServiceUnavailable, "space_policy_unavailable",
				"Space policy is unavailable", nil)
			return
		}
		role, err := s.orgAdminRole(c.Request.Context(), userID, orgID)
		if errors.Is(err, ErrMembershipNotFound) {
			abortTypedError(c, http.StatusForbidden, "space_org_membership_required",
				"organization membership is required", nil)
			return
		}
		if err != nil {
			log.Error().Err(err).Msg("org Space policy: canonical membership authority unavailable")
			abortTypedError(c, http.StatusServiceUnavailable, "space_policy_membership_authority_unavailable",
				"organization membership could not be verified", nil)
			return
		}
		if role != "owner" && role != "admin" {
			abortTypedError(c, http.StatusForbidden, "space_org_admin_required",
				"an organization owner or admin is required", nil)
			return
		}
		if write && !s.spaceOrgPolicy.writesEnabled {
			actor := spaces.EffectPolicyActor{Type: "user", ID: userID, OrgRole: role}
			if err := s.spaceOrgPolicy.store.RecordOrgPolicyWriteDenied(
				c.Request.Context(), orgID, actor, "space_policy_admin_writes_disabled"); err != nil {
				log.Error().Err(err).Msg("org Space policy: write-denied audit failed")
			}
			abortTypedError(c, http.StatusForbidden, "space_policy_admin_writes_disabled",
				"Space policy changes are not enabled", nil)
			return
		}
		c.Set("space_org_policy_role", role)
		c.Next()
	}
}

func (s *Server) orgPolicyCapabilities(view spaces.OrgPolicyView) spaces.OrgPolicyCapabilities {
	switch {
	case !s.spaceOrgPolicy.writesEnabled:
		reason := "writes_disabled"
		return spaces.OrgPolicyCapabilities{WriteDeniedReason: &reason}
	case view.State != spaces.OrgPolicyStateProvisioned:
		reason := "not_provisioned"
		return spaces.OrgPolicyCapabilities{WriteDeniedReason: &reason}
	}
	return spaces.OrgPolicyCapabilities{Write: true}
}

// writeOrgPolicyError maps a store error to the typed envelope.
func writeOrgPolicyError(c *gin.Context, err error) {
	var policyErr *spaces.OrgPolicyError
	if errors.As(err, &policyErr) {
		status := http.StatusUnprocessableEntity
		switch policyErr.Code {
		case spaces.OrgPolicyCodeInvalidRequest:
			status = http.StatusBadRequest
		case spaces.OrgPolicyCodeRevisionConflict, spaces.OrgPolicyCodeIdempotencyConflict, spaces.OrgPolicyCodeNotProvisioned:
			status = http.StatusConflict
		}
		abortTypedError(c, status, policyErr.Code, policyErr.Message, policyErr.Details)
		return
	}
	if errors.Is(err, spaces.ErrNoOrgEffectPolicy) {
		abortTypedError(c, http.StatusConflict, spaces.OrgPolicyCodeNotProvisioned,
			"this organization has no Space policy yet", nil)
		return
	}
	log.Error().Err(err).Msg("org Space policy: store error")
	abortTypedError(c, http.StatusServiceUnavailable, "space_policy_unavailable", "Space policy is unavailable", nil)
}

func (s *Server) getSpaceOrgPolicy(c *gin.Context) {
	view, err := s.spaceOrgPolicy.store.GetOrgPolicy(c.Request.Context(), c.GetString("org_id"))
	if err != nil {
		writeOrgPolicyError(c, err)
		return
	}
	view.Capabilities = s.orgPolicyCapabilities(view)
	c.JSON(http.StatusOK, gin.H{"data": view})
}

type putSpaceOrgPolicyEntitlementsRequest struct {
	IdempotencyKey              string          `json:"idempotency_key"`
	ExpectedEntitlementRevision *int64          `json:"expected_entitlement_revision"`
	PresetID                    *string         `json:"preset_id"`
	Entitlements                map[string]bool `json:"entitlements"`
}

func (s *Server) putSpaceOrgPolicyEntitlements(c *gin.Context) {
	raw, err := io.ReadAll(io.LimitReader(c.Request.Body, spacePolicyAdminMaxBody+1))
	if err != nil || len(raw) > spacePolicyAdminMaxBody {
		abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
			"request body is unreadable or too large", map[string]any{"field": "body"})
		return
	}
	// No org, actor or role field is accepted: those come from the delegation.
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var body putSpaceOrgPolicyEntitlementsRequest
	if err := decoder.Decode(&body); err != nil {
		abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
			"request body is not valid", map[string]any{"field": "body"})
		return
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
			"request body is not valid", map[string]any{"field": "body"})
		return
	}
	if body.ExpectedEntitlementRevision == nil {
		abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
			"expected_entitlement_revision is required", map[string]any{"field": "expected_entitlement_revision"})
		return
	}
	hasPreset := body.PresetID != nil && strings.TrimSpace(*body.PresetID) != ""
	hasEntitlements := body.Entitlements != nil
	if hasPreset == hasEntitlements {
		abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
			"send exactly one of preset_id or entitlements", map[string]any{"field": "preset_id"})
		return
	}
	req := spaces.ApplyOrgEntitlementsRequest{
		OrgID: strings.TrimSpace(c.GetString("org_id")),
		Actor: spaces.EffectPolicyActor{
			Type: "user", ID: strings.TrimSpace(c.GetString("user_id")), OrgRole: c.GetString("space_org_policy_role"),
		},
		IdempotencyKey:              body.IdempotencyKey,
		ExpectedEntitlementRevision: *body.ExpectedEntitlementRevision,
		Entitlements:                body.Entitlements,
	}
	if hasPreset {
		req.PresetID = strings.TrimSpace(*body.PresetID)
	}
	result, err := s.spaceOrgPolicy.store.ApplyOrgEntitlements(c.Request.Context(), req)
	if err != nil {
		writeOrgPolicyError(c, err)
		return
	}
	result.Policy.Capabilities = s.orgPolicyCapabilities(result.Policy)
	c.JSON(http.StatusOK, gin.H{"data": result})
}

func (s *Server) listSpaceOrgPolicyEvents(c *gin.Context) {
	limit := spaces.OrgPolicyEventsDefaultLimit
	if raw := strings.TrimSpace(c.Query("limit")); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 || parsed > spaces.OrgPolicyEventsMaxLimit {
			abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
				"limit must be 1 to 100", map[string]any{"field": "limit"})
			return
		}
		limit = parsed
	}
	var cursor int64
	if raw := strings.TrimSpace(c.Query("cursor")); raw != "" {
		parsed, err := strconv.ParseInt(raw, 10, 64)
		if err != nil || parsed < 1 {
			abortTypedError(c, http.StatusBadRequest, spaces.OrgPolicyCodeInvalidRequest,
				"cursor is not valid", map[string]any{"field": "cursor"})
			return
		}
		cursor = parsed
	}
	events, next, err := s.spaceOrgPolicy.store.ListOrgPolicyEvents(c.Request.Context(), c.GetString("org_id"), limit, cursor)
	if err != nil {
		writeOrgPolicyError(c, err)
		return
	}
	if events == nil {
		events = []spaces.OrgPolicyEvent{}
	}
	var nextCursor any
	if next > 0 {
		nextCursor = strconv.FormatInt(next, 10)
	}
	c.JSON(http.StatusOK, gin.H{"data": events, "meta": gin.H{"limit": limit, "next_cursor": nextCursor}})
}
