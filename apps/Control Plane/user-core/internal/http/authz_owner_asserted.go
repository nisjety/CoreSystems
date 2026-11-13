package http

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/rs/zerolog/log"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/authztaxonomy"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/users"
)

// WS14-S2: owner-asserted grants and Space-subject grants.
//
// Two reviewed modes sit behind requireVerifiedAuthzDelegation, each behind its
// own flag (strict literal true, off by default, never an authorization input):
//
//   - CONTROL_OWNER_ASSERTED_GRANTS: the authz grant write surface
//     (POST/DELETE /authz/grant, GET /authz/grants). Control cannot see the
//     owning plane's rows, so it admits only the ONE service registered as the
//     owner of the resource type (authztaxonomy.OwnerService), acting for a
//     verified end user whose delegation names this exact operation, resource
//     and tenant. The owning service must itself have proved the actor owns the
//     resource before calling. The gateway principal can never pass: it is not a
//     registered owner service and holds no authz:* scope.
//   - CONTROL_SPACE_SUBJECT_GRANTS: file-grant (the only writer of a Space
//     subject row, proved by a Control-signed import decision) and
//     GET /authz/space-visible (the read-time join). Off: both answer 404.

const (
	authzGrantPath        = "/api/v1/internal/authz/grant"
	authzGrantsPath       = "/api/v1/internal/authz/grants"
	authzSpaceVisiblePath = "/api/v1/internal/authz/space-visible"

	ownerAssertedContextKey = "owner_asserted_grant"

	// documentsAPIPrincipal is the Data Plane service that owns documents and
	// writes room-file grants. Its credential holds spaces:file-grant:write.
	documentsAPIPrincipal = "documents-api"
	scopeSpaceFileGrant   = "spaces:file-grant:write"

	grantMaxBodyBytes = 1 << 20
	fileGrantMaxIDLen = 200

	// conversationSnapshotType is the share snapshot resource type (ADR
	// conversation-share-model). session-core is its one owner service, it is never
	// Space-grantable, and its grants are view only.
	conversationSnapshotType = "conversation_snapshot"
)

// SetOwnerAssertedGrants wires CONTROL_OWNER_ASSERTED_GRANTS (strict literal true).
func (s *Server) SetOwnerAssertedGrants(enabled bool) { s.ownerAssertedGrants = enabled }

// SetSpaceSubjectGrants wires CONTROL_SPACE_SUBJECT_GRANTS (strict literal true).
func (s *Server) SetSpaceSubjectGrants(enabled bool) { s.spaceSubjectGrants = enabled }

func abortGrantRefusal(c *gin.Context, status int, code, message string) {
	c.AbortWithStatusJSON(status, gin.H{"error": message, "code": code})
}

func writeGrantRefusal(c *gin.Context, status int, code, message string) {
	c.JSON(status, gin.H{"error": message, "code": code})
}

// requireOwnerAssertedGrant admits a grant write or grant listing only in
// owner-asserted mode (see the package comment above). Every claim is the
// signed delegation's, compared with the request's own selectors.
func (s *Server) requireOwnerAssertedGrant(c *gin.Context) {
	fenced := func() {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified resource-owner delegation required"})
	}
	if !s.ownerAssertedGrants {
		fenced()
		return
	}
	var wantOperation string
	switch {
	case c.Request.URL.Path == authzGrantPath && c.Request.Method == http.MethodPost:
		wantOperation = "authz:grant"
	case c.Request.URL.Path == authzGrantPath && c.Request.Method == http.MethodDelete:
		wantOperation = "authz:revoke"
	case c.Request.URL.Path == authzGrantsPath && c.Request.Method == http.MethodGet:
		wantOperation = "authz:grants"
	default:
		fenced()
		return
	}
	if c.GetString("delegation_operation") != wantOperation {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated operation mismatch"})
		return
	}

	var orgID, resourceType, resourceID string
	if c.Request.Method == http.MethodPost {
		body, err := io.ReadAll(io.LimitReader(c.Request.Body, grantMaxBodyBytes+1))
		if err != nil || len(body) > grantMaxBodyBytes {
			abortGrantRefusal(c, http.StatusBadRequest, "invalid_request", "invalid request body")
			return
		}
		// The handler binds the body again.
		c.Request.Body = io.NopCloser(bytes.NewReader(body))
		var selectors struct {
			OrgID        string `json:"org_id"`
			ResourceType string `json:"resource_type"`
			ResourceID   string `json:"resource_id"`
		}
		if err := json.Unmarshal(body, &selectors); err != nil {
			abortGrantRefusal(c, http.StatusBadRequest, "invalid_request", "invalid request body")
			return
		}
		orgID, resourceType, resourceID = selectors.OrgID, selectors.ResourceType, selectors.ResourceID
	} else {
		orgID, resourceType, resourceID = c.Query("org_id"), c.Query("resource_type"), c.Query("resource_id")
	}
	orgID, resourceType, resourceID = strings.TrimSpace(orgID), strings.TrimSpace(resourceType), strings.TrimSpace(resourceID)

	if resourceType == "" || resourceID == "" ||
		resourceType != c.GetString("delegation_resource_type") || resourceID != c.GetString("delegation_resource_id") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated resource mismatch"})
		return
	}
	if orgID == "" || orgID != c.GetString("org_id") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated tenant mismatch"})
		return
	}
	owner, registered := authztaxonomy.OwnerService(resourceType)
	if !registered || c.GetString("service_id") != owner {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "registered resource owner service required"})
		return
	}
	c.Set(ownerAssertedContextKey, true)
	c.Next()
}

// applyOwnerAssertedGrantRules enforces the rules that only owner-asserted
// grant writes carry. refused=true means the response is already written.
// Otherwise it returns the role to store and the granter, taken from the
// delegated user and never from the body.
func (s *Server) applyOwnerAssertedGrantRules(c *gin.Context, resourceType, subjectID, subjectType, role string) (storedRole, grantedBy string, refused bool) {
	if t := strings.TrimSpace(subjectType); t != "" && t != "user" {
		// A Space subject is written by file-grant alone; a team is deferred.
		writeGrantRefusal(c, http.StatusBadRequest, "subject_type_not_supported", "subject_type is not supported for this grant")
		return "", "", true
	}
	normalized, err := users.NormalizeGrantRole(role)
	if err != nil {
		writeGrantRefusal(c, http.StatusBadRequest, "role_not_supported", "role is not supported for this grant")
		return "", "", true
	}
	if normalized == "edit" && !authztaxonomy.EditEnforced(resourceType) {
		// `edit` is stored but unenforced for this type: refuse it rather than
		// promise access nothing checks.
		writeGrantRefusal(c, http.StatusBadRequest, "role_not_supported", "role is not supported for this resource type")
		return "", "", true
	}
	if resourceType == conversationSnapshotType && normalized != "view" {
		// A snapshot is an immutable, read-only copy: view is its only role, whether
		// or not an owner ever enforces edit for it.
		writeGrantRefusal(c, http.StatusBadRequest, "role_not_supported", "role is not supported for this resource type")
		return "", "", true
	}
	// Org-only for every owner-asserted type, conversation_snapshot included (ADR
	// decision 2): the grantee must be an active member of the granter's org.
	member, err := s.aclRepo.IsActiveOrgMember(c.Request.Context(), c.GetString("org_id"), strings.TrimSpace(subjectID))
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "grant subject could not be verified"})
		return "", "", true
	}
	if !member {
		writeGrantRefusal(c, http.StatusBadRequest, spaces.CodeMemberNotInOrganization, "grant subject is not a member of the organization")
		return "", "", true
	}
	log.Info().Str("service", c.GetString("service_id")).Str("org_id", c.GetString("org_id")).
		Str("resource_type", resourceType).Str("granted_by", c.GetString("user_id")).
		Msg("owner-asserted resource grant")
	return normalized, c.GetString("user_id"), false
}

// requireSpaceVisibleDelegation gates GET /authz/space-visible: flag on, the
// signed delegation names this operation, resource type, tenant and user.
func (s *Server) requireSpaceVisibleDelegation(c *gin.Context) {
	if !s.spaceSubjectGrants {
		c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": "not found"})
		return
	}
	if c.Request.Method != http.MethodGet {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "verified resource-owner delegation required"})
		return
	}
	spaceRef := strings.TrimSpace(c.Query("space_ref"))
	if c.GetString("delegation_operation") != "authz:space-visible" ||
		strings.TrimSpace(c.Query("resource_type")) != c.GetString("delegation_resource_type") ||
		spaceRef == "" {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated operation or resource mismatch"})
		return
	}
	if bound := c.GetString("delegation_resource_id"); bound != "" && bound != spaceRef {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated resource mismatch"})
		return
	}
	if strings.TrimSpace(c.Query("org_id")) != c.GetString("org_id") || strings.TrimSpace(c.Query("subject_id")) != c.GetString("user_id") {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "delegated tenant or subject mismatch"})
		return
	}
	c.Next()
}

// authzSpaceVisible: GET /api/v1/internal/authz/space-visible?org_id&subject_id&resource_type=document&space_ref
// Returns { ids, space_ref, audience_revision }: the resources attached to the
// Space that the delegated user may see through it NOW (current member AND
// current audience). A user who does not resolve gets an empty set, never an
// error that tells whether the Space exists. Fails closed on any storage error.
func (s *Server) authzSpaceVisible(c *gin.Context) {
	resourceType := strings.TrimSpace(c.Query("resource_type"))
	spaceRef := strings.TrimSpace(c.Query("space_ref"))
	if !authztaxonomy.SpaceGrantable(resourceType) {
		writeGrantRefusal(c, http.StatusBadRequest, "resource_type_not_supported", "resource_type cannot be attached to a Space")
		return
	}
	if s.aclRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "authz facade not configured"})
		return
	}
	ids, revision, _, err := s.aclRepo.ListVisibleForSpace(c.Request.Context(), c.GetString("org_id"), resourceType, spaceRef, c.GetString("user_id"))
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space-visible resources unavailable"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"ids": ids, "space_ref": spaceRef, "audience_revision": revision})
}

// requireSpaceFileGrantWriter admits only Data Plane's documents-api holding
// spaces:file-grant:write, with the Space-subject flag on. It takes no user
// delegation: the import worker has no user bearer, so the write is proved by
// the Control-signed decision in the body instead.
func (s *Server) requireSpaceFileGrantWriter(c *gin.Context) {
	if !s.spaceSubjectGrants {
		c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": "not found"})
		return
	}
	if c.GetString("auth_method") != "service_principal" ||
		c.GetString("service_id") != documentsAPIPrincipal ||
		!hasServiceScope(c, scopeSpaceFileGrant) {
		c.AbortWithStatusJSON(http.StatusForbidden, gin.H{"error": "documents-api file grant principal required"})
		return
	}
	c.Next()
}

type spaceFileGrantRequest struct {
	DecisionToken string `json:"decision_token"`
	DocumentID    string `json:"document_id"`
}

// issueSpaceFileGrant: POST /api/v1/internal/spaces/file-grant
// Body { decision_token, document_id }. Writes (idempotently) the one
// subject_type='space' view grant that attaches a document to the room the
// import decision was issued for. The document id is the owning service's
// claim; Control proves WHO may attach (the signed decision) and re-resolves
// current membership, audience, role, entitlement and ZDR before writing.
func (s *Server) issueSpaceFileGrant(c *gin.Context) {
	var request spaceFileGrantRequest
	decoder := json.NewDecoder(io.LimitReader(c.Request.Body, grantMaxBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		writeGrantRefusal(c, http.StatusBadRequest, "invalid_request", "decision_token and document_id are required")
		return
	}
	request.DecisionToken = strings.TrimSpace(request.DecisionToken)
	request.DocumentID = strings.TrimSpace(request.DocumentID)
	if request.DecisionToken == "" || request.DocumentID == "" || len(request.DocumentID) > fileGrantMaxIDLen {
		writeGrantRefusal(c, http.StatusBadRequest, "invalid_request", "decision_token and document_id are required")
		return
	}
	if s.spaceRepo == nil || s.aclRepo == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority repository unavailable"})
		return
	}
	key, err := spaces.LoadSigningKeyFromEnv(os.Getenv)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space decision verifier unavailable"})
		return
	}
	decision, err := spaces.VerifyIssuedDecision(key, request.DecisionToken, time.Now().UTC())
	if err != nil {
		writeGrantRefusal(c, http.StatusForbidden, "decision_invalid", "a valid Control import decision is required")
		return
	}
	if err := decision.ValidateForFileGrant(); err != nil {
		if denial, ok := spaces.AsDenial(err); ok {
			c.JSON(http.StatusForbidden, spaceDenialBody("room file attach is not authorized", denial))
			return
		}
		writeGrantRefusal(c, http.StatusForbidden, "decision_invalid", "a valid Control import decision is required")
		return
	}
	authority, err := s.spaceRepo.ResolveFileGrantAuthority(c.Request.Context(), *decision)
	if err != nil {
		switch {
		case errors.Is(err, spaces.ErrNoCurrentMembership):
			c.JSON(http.StatusForbidden, gin.H{
				"error": "current Space membership required", "code": string(spaces.DenialMembershipRequired),
				"resolvable_by": spaces.ResolvableByNone, "details": gin.H{},
			})
		default:
			if denial, ok := spaces.AsDenial(err); ok {
				c.JSON(http.StatusForbidden, spaceDenialBody("room file attach is not authorized", denial))
				return
			}
			c.JSON(http.StatusServiceUnavailable, gin.H{"error": "Space authority unavailable"})
		}
		return
	}
	grant, created, err := s.aclRepo.GrantSpaceSubject(c.Request.Context(), authority.OrgID, "document", request.DocumentID, authority.SpaceRef, authority.SubjectID)
	if err != nil {
		if errors.Is(err, users.ErrSpaceNotGrantable) {
			writeGrantRefusal(c, http.StatusConflict, "space_not_grantable", "the Space cannot hold this grant")
			return
		}
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "room file grant could not be written"})
		return
	}
	if s.publisher != nil {
		s.publisher.PublishResourceGrantsChanged(c.Request.Context(),
			grant.GrantID, grant.OrgID, grant.ResourceType, grant.ResourceID,
			grant.SubjectType, grant.SubjectID, grant.Role, "grant")
	}
	log.Info().Str("org_id", grant.OrgID).Str("space_ref", grant.SubjectID).Str("granted_by", grant.GrantedBy).
		Bool("created", created).Msg("space file grant")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"grant_id": grant.GrantID, "created": created}})
}

func init() {
	registerSpaceFeatureRoute(func(s *Server, spacesGroup *gin.RouterGroup) {
		spacesGroup.POST("/file-grant", s.requireSpaceFileGrantWriter, s.issueSpaceFileGrant)
	})
}
