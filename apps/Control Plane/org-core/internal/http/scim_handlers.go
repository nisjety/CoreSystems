package http

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"log"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// SCIM Users (WS05-S8). Two groups of routes, both answering 404 until
// ORG_SCIM_ENABLED is on:
//
//   - admin token management, /orgs/:id/scim/*, reached through the gateway on
//     behalf of a signed owner or admin (scope org:identity:admin:self);
//   - the SCIM surface itself, /internal/scim/v2/*, reached only by the stateless
//     gateway edge under the separate principal verevon-scim-edge (scope
//     org:scim:any). The IdP's bearer token travels in Authorization and
//     resolves to the organisation; no header names an organisation.

// scimFlagEnv switches the SCIM routes on. Anything but "1" or "true" leaves
// them unmounted (404), the default.
const scimFlagEnv = "ORG_SCIM_ENABLED"

func scimEnabled(getenv func(string) string) bool {
	switch getenv(scimFlagEnv) {
	case "1", "true":
		return true
	}
	return false
}

// scimEdgeTokenEnv carries the secret of the verevon-scim-edge principal. The
// principal exists only while ORG_SCIM_ENABLED is on and this token is set, so
// a deployment that never enables SCIM keeps its credential registry unchanged.
// The gateway presents the same value as its SCIM edge credential.
const scimEdgeTokenEnv = "ORG_CORE_SCIM_EDGE_TOKEN"

// withSCIMEdgeCredential appends the edge principal (scope org:scim:any) to a
// validated registry. An unusable token (short, duplicate) is logged and the
// registry is returned unchanged, so the edge is refused rather than half-armed.
func withSCIMEdgeCredential(credentials []serviceCredential, token string) []serviceCredential {
	token = strings.TrimSpace(token)
	if token == "" {
		log.Printf("org-core scim: %s is not set; the SCIM edge cannot authenticate", scimEdgeTokenEnv)
		return credentials
	}
	principals := make(map[string]struct{}, len(credentials)+1)
	tokens := make(map[[sha256.Size]byte]struct{}, len(credentials)+1)
	for _, c := range credentials {
		principals[c.Principal] = struct{}{}
		tokens[sha256.Sum256([]byte(c.Token))] = struct{}{}
	}
	edge := serviceCredential{
		Principal: scimEdgePrincipal, Audience: serviceCredentialAudience, Token: token, Scopes: []string{"org:scim:any"},
	}
	if err := validateServiceCredential(edge, principals, tokens); err != nil {
		log.Printf("org-core scim: edge credential rejected: %v", err)
		return credentials
	}
	return append(append([]serviceCredential(nil), credentials...), edge)
}

const (
	scimMaxBodyBytes   = 64 << 10
	scimRequestTimeout = 20 * time.Second
	scimRatePerMinute  = 300
	scimContentType    = "application/scim+json"

	scimSchemaUser     = "urn:ietf:params:scim:schemas:core:2.0:User"
	scimSchemaList     = "urn:ietf:params:scim:api:messages:2.0:ListResponse"
	scimSchemaError    = "urn:ietf:params:scim:api:messages:2.0:Error"
	scimSchemaPatch    = "urn:ietf:params:scim:api:messages:2.0:PatchOp"
	scimSchemaConfig   = "urn:ietf:params:scim:schemas:core:2.0:ServiceProviderConfig"
	scimSchemaResource = "urn:ietf:params:scim:schemas:core:2.0:ResourceType"
	scimSchemaSchema   = "urn:ietf:params:scim:schemas:core:2.0:Schema"
	scimUsersLocation  = "/api/v1/scim/v2/Users/"
)

// scimService is the seam the SCIM routes depend on. *org.Service satisfies it;
// handler tests use a fake.
type scimService interface {
	CreateSCIMToken(ctx context.Context, orgID, actorID, actorRole, name string, expiresInDays int) (*orgcore.SCIMTokenIssued, error)
	ListSCIMTokens(ctx context.Context, orgID string) ([]orgcore.SCIMToken, error)
	RevokeSCIMToken(ctx context.Context, orgID, actorID, actorRole, tokenID string) error
	GetSCIMStatus(ctx context.Context, orgID string) (*orgcore.SCIMStatus, error)
	ResolveSCIMToken(ctx context.Context, bearer string) (*orgcore.SCIMTokenAuth, error)
	ListSCIMUsers(ctx context.Context, auth *orgcore.SCIMTokenAuth, filter *orgcore.SCIMFilter, startIndex, count int) ([]orgcore.SCIMUserRecord, int, error)
	GetSCIMUser(ctx context.Context, auth *orgcore.SCIMTokenAuth, scimID string) (*orgcore.SCIMUserRecord, error)
	CreateSCIMUser(ctx context.Context, auth *orgcore.SCIMTokenAuth, in orgcore.SCIMUserInput) (*orgcore.SCIMUserRecord, error)
	SetSCIMUserActive(ctx context.Context, auth *orgcore.SCIMTokenAuth, scimID string, active bool) (*orgcore.SCIMUserRecord, error)
	DeleteSCIMUser(ctx context.Context, auth *orgcore.SCIMTokenAuth, scimID string) error
}

// SetSCIM wires the SCIM service. A server without it answers the routes 503.
func (s *Server) SetSCIM(service scimService) { s.scim = service }

func (s *Server) requireSCIMEnabled() gin.HandlerFunc {
	return func(c *gin.Context) {
		if !s.scimOn {
			c.AbortWithStatusJSON(http.StatusNotFound, gin.H{"error": gin.H{"code": "not_found", "message": "not found"}})
			return
		}
		c.Next()
	}
}

// --- admin token management ---

func (s *Server) scimAdminRequest(c *gin.Context) (orgID, actorID, actorRole string, ok bool) {
	c.Header("Cache-Control", "no-store")
	orgID = strings.TrimSpace(c.Param("id"))
	if orgID == "" {
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "organization id is required", nil)
		return "", "", "", false
	}
	actorID, actorRole, ok = s.authorizeIdentityAdmin(c, orgID)
	if !ok {
		return "", "", "", false
	}
	if s.scim == nil {
		writeIdentityError(c, http.StatusServiceUnavailable, "scim_unavailable", "scim administration unavailable", nil)
		return "", "", "", false
	}
	return orgID, actorID, actorRole, true
}

func writeSCIMAdminError(c *gin.Context, orgID, operation string, err error) {
	var bad *orgcore.SCIMRequestError
	switch {
	case errors.As(err, &bad):
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "the request is not valid", gin.H{"reason": bad.Reason})
	case errors.Is(err, orgcore.ErrSCIMNoVerifiedDomain):
		writeIdentityError(c, http.StatusUnprocessableEntity, "no_verified_domain", "verify a domain before creating a scim token", nil)
	case errors.Is(err, orgcore.ErrSCIMTokenLimit):
		writeIdentityError(c, http.StatusConflict, "scim_token_limit", "an organization can have at most 3 active scim tokens", gin.H{"max": orgcore.MaxActiveSCIMTokens})
	case errors.Is(err, orgcore.ErrSCIMTokenNotFound):
		writeIdentityError(c, http.StatusNotFound, "scim_token_not_found", "scim token not found", nil)
	case errors.Is(err, orgcore.ErrSCIMUnavailable):
		writeIdentityError(c, http.StatusServiceUnavailable, "scim_unavailable", "scim administration unavailable", nil)
	default:
		log.Printf("scim admin: %s failed for org=%s: %v", operation, orgID, err)
		writeIdentityError(c, http.StatusInternalServerError, "internal_error", "failed to "+operation, nil)
	}
}

// createSCIMToken serves POST /orgs/:id/scim/tokens. The bearer token is in this
// response only; it is never logged and cannot be read again.
func (s *Server) createSCIMToken(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.scimAdminRequest(c)
	if !ok {
		return
	}
	var req struct {
		Name          string `json:"name"`
		ExpiresInDays int    `json:"expires_in_days"`
	}
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, identityMaxBodyBytes))
	decoder.DisallowUnknownFields()
	err := decoder.Decode(&req)
	if err == nil {
		err = ensureJSONEOF(decoder)
	}
	if err != nil {
		writeIdentityError(c, http.StatusBadRequest, "invalid_request", "invalid request body", nil)
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	issued, err := s.scim.CreateSCIMToken(ctx, orgID, actorID, actorRole, req.Name, req.ExpiresInDays)
	if err != nil {
		writeSCIMAdminError(c, orgID, "create scim token", err)
		return
	}
	log.Printf("scim admin: org=%s token=%s created by actor=%s", orgID, issued.Token.ID, actorID)
	c.JSON(http.StatusCreated, gin.H{"data": issued})
}

// listSCIMTokens serves GET /orgs/:id/scim/tokens.
func (s *Server) listSCIMTokens(c *gin.Context) {
	orgID, _, _, ok := s.scimAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	tokens, err := s.scim.ListSCIMTokens(ctx, orgID)
	if err != nil {
		writeSCIMAdminError(c, orgID, "list scim tokens", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"tokens": tokens}})
}

// revokeSCIMToken serves DELETE /orgs/:id/scim/tokens/:tokenId.
func (s *Server) revokeSCIMToken(c *gin.Context) {
	orgID, actorID, actorRole, ok := s.scimAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	if err := s.scim.RevokeSCIMToken(ctx, orgID, actorID, actorRole, c.Param("tokenId")); err != nil {
		writeSCIMAdminError(c, orgID, "revoke scim token", err)
		return
	}
	log.Printf("scim admin: org=%s token=%s revoked by actor=%s", orgID, c.Param("tokenId"), actorID)
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"status": "revoked"}})
}

// getSCIMStatus serves GET /orgs/:id/scim/status.
func (s *Server) getSCIMStatus(c *gin.Context) {
	orgID, _, _, ok := s.scimAdminRequest(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
	defer cancel()
	status, err := s.scim.GetSCIMStatus(ctx, orgID)
	if err != nil {
		writeSCIMAdminError(c, orgID, "read scim status", err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": status})
}

// --- the SCIM surface ---

func writeSCIMError(c *gin.Context, status int, scimType, detail string) {
	body := gin.H{"schemas": []string{scimSchemaError}, "status": strconv.Itoa(status), "detail": detail}
	if scimType != "" {
		body["scimType"] = scimType
	}
	writeSCIMJSON(c, status, body)
}

func writeSCIMJSON(c *gin.Context, status int, body any) {
	encoded, err := json.Marshal(body)
	if err != nil {
		c.Status(http.StatusInternalServerError)
		return
	}
	c.Data(status, scimContentType, encoded)
}

// scimRateLimiter is a fixed one-minute window per token. It is per process: with
// several org-core replicas the effective ceiling is the replica count times the
// limit (a depth item), which still bounds a runaway IdP.
type scimRateLimiter struct {
	mu      sync.Mutex
	windows map[string]*scimWindow
	limit   int
	now     func() time.Time
}

type scimWindow struct {
	start time.Time
	count int
}

func newSCIMRateLimiter(limit int, now func() time.Time) *scimRateLimiter {
	return &scimRateLimiter{windows: map[string]*scimWindow{}, limit: limit, now: now}
}

// allow reports whether the token may proceed, or how long to wait.
func (l *scimRateLimiter) allow(tokenID string) (bool, time.Duration) {
	l.mu.Lock()
	defer l.mu.Unlock()
	now := l.now()
	if len(l.windows) > 4096 {
		for id, w := range l.windows {
			if now.Sub(w.start) >= time.Minute {
				delete(l.windows, id)
			}
		}
	}
	w := l.windows[tokenID]
	if w == nil || now.Sub(w.start) >= time.Minute {
		l.windows[tokenID] = &scimWindow{start: now, count: 1}
		return true, 0
	}
	if w.count >= l.limit {
		return false, time.Minute - now.Sub(w.start)
	}
	w.count++
	return true, 0
}

const scimAuthKey = "scim_auth"

// scimAuthenticate resolves the IdP bearer token and applies the rate limit.
func (s *Server) scimAuthenticate() gin.HandlerFunc {
	return func(c *gin.Context) {
		c.Header("Cache-Control", "no-store")
		if s.scim == nil {
			writeSCIMError(c, http.StatusServiceUnavailable, "", "SCIM is unavailable.")
			c.Abort()
			return
		}
		header := strings.TrimSpace(c.GetHeader("Authorization"))
		bearer := ""
		if len(header) > 7 && strings.EqualFold(header[:7], "bearer ") {
			bearer = strings.TrimSpace(header[7:])
		}
		ctx, cancel := context.WithTimeout(c.Request.Context(), identityRequestTimeout)
		defer cancel()
		auth, err := s.scim.ResolveSCIMToken(ctx, bearer)
		switch {
		case errors.Is(err, orgcore.ErrSCIMUnauthorized):
			c.Header("WWW-Authenticate", `Bearer realm="scim"`)
			writeSCIMError(c, http.StatusUnauthorized, "", "A valid SCIM bearer token is required.")
			c.Abort()
			return
		case errors.Is(err, orgcore.ErrSCIMUnavailable):
			writeSCIMError(c, http.StatusServiceUnavailable, "", "SCIM is unavailable.")
			c.Abort()
			return
		case err != nil:
			log.Printf("scim: token resolution failed: %v", err)
			writeSCIMError(c, http.StatusServiceUnavailable, "", "SCIM is unavailable.")
			c.Abort()
			return
		}
		if allowed, wait := s.scimLimiter.allow(auth.TokenID); !allowed {
			seconds := int(wait.Seconds()) + 1
			c.Header("Retry-After", strconv.Itoa(seconds))
			writeSCIMError(c, http.StatusTooManyRequests, "", "Rate limit exceeded.")
			c.Abort()
			return
		}
		c.Set(scimAuthKey, auth)
		c.Next()
	}
}

func scimAuthOf(c *gin.Context) *orgcore.SCIMTokenAuth {
	value, _ := c.Get(scimAuthKey)
	auth, _ := value.(*orgcore.SCIMTokenAuth)
	return auth
}

func scimUserResource(rec orgcore.SCIMUserRecord) gin.H {
	meta := gin.H{
		"resourceType": "User",
		"created":      rec.CreatedAt.UTC().Format(time.RFC3339),
		"lastModified": rec.UpdatedAt.UTC().Format(time.RFC3339),
		"location":     scimUsersLocation + rec.SCIMID,
	}
	out := gin.H{
		"schemas": []string{scimSchemaUser}, "id": rec.SCIMID, "userName": rec.UserName,
		"active": rec.Active, "meta": meta,
	}
	if rec.ExternalID != "" {
		out["externalId"] = rec.ExternalID
	}
	return out
}

func writeSCIMUserError(c *gin.Context, operation string, err error) {
	var bad *orgcore.SCIMRequestError
	switch {
	case errors.As(err, &bad):
		writeSCIMError(c, http.StatusBadRequest, "invalidValue", "The "+bad.Reason+" value is not valid.")
	case errors.Is(err, orgcore.ErrSCIMDomainNotVerified):
		writeSCIMError(c, http.StatusBadRequest, "invalidValue", "The userName domain is not verified for this organization.")
	case errors.Is(err, orgcore.ErrSCIMInvalidFilter):
		writeSCIMError(c, http.StatusBadRequest, "invalidFilter", "Only 'userName eq' and 'externalId eq' filters are supported.")
	case errors.Is(err, orgcore.ErrSCIMUserNotFound):
		writeSCIMError(c, http.StatusNotFound, "", "User not found.")
	case errors.Is(err, orgcore.ErrSCIMUniqueness):
		writeSCIMError(c, http.StatusConflict, "uniqueness", "A user with this userName already exists.")
	case errors.Is(err, orgcore.ErrSCIMLastOwner):
		writeSCIMError(c, http.StatusConflict, "mutability", "The only organization owner cannot be deactivated.")
	case errors.Is(err, orgcore.ErrSCIMMembershipConflict):
		c.Header("Retry-After", "5")
		writeSCIMError(c, http.StatusServiceUnavailable, "", "Membership changed concurrently; retry.")
	case errors.Is(err, orgcore.ErrSCIMUnavailable), errors.Is(err, orgcore.ErrSSOUnavailable):
		c.Header("Retry-After", "30")
		writeSCIMError(c, http.StatusServiceUnavailable, "", "SCIM is unavailable.")
	default:
		log.Printf("scim: %s failed: %v", operation, err)
		writeSCIMError(c, http.StatusInternalServerError, "", "The request could not be completed.")
	}
}

func scimContext(c *gin.Context) (context.Context, context.CancelFunc) {
	return context.WithTimeout(c.Request.Context(), scimRequestTimeout)
}

// parseSCIMFilter accepts `userName eq "x"` and `externalId eq "x"` only.
func parseSCIMFilter(raw string) (*orgcore.SCIMFilter, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return nil, nil
	}
	parts := strings.SplitN(raw, " ", 3)
	if len(parts) != 3 || !strings.EqualFold(parts[1], "eq") {
		return nil, orgcore.ErrSCIMInvalidFilter
	}
	value := strings.TrimSpace(parts[2])
	if len(value) < 2 || value[0] != '"' || value[len(value)-1] != '"' {
		return nil, orgcore.ErrSCIMInvalidFilter
	}
	var unquoted string
	if err := json.Unmarshal([]byte(value), &unquoted); err != nil {
		return nil, orgcore.ErrSCIMInvalidFilter
	}
	switch strings.ToLower(parts[0]) {
	case "username":
		return &orgcore.SCIMFilter{Attribute: "userName", Value: unquoted}, nil
	case "externalid":
		return &orgcore.SCIMFilter{Attribute: "externalId", Value: unquoted}, nil
	}
	return nil, orgcore.ErrSCIMInvalidFilter
}

func (s *Server) scimListUsers(c *gin.Context) {
	filter, err := parseSCIMFilter(c.Query("filter"))
	if err != nil {
		writeSCIMUserError(c, "list", err)
		return
	}
	startIndex, _ := strconv.Atoi(c.DefaultQuery("startIndex", "1"))
	count, convErr := strconv.Atoi(c.DefaultQuery("count", "100"))
	if convErr != nil {
		count = 100
	}
	ctx, cancel := scimContext(c)
	defer cancel()
	records, total, err := s.scim.ListSCIMUsers(ctx, scimAuthOf(c), filter, startIndex, count)
	if err != nil {
		writeSCIMUserError(c, "list", err)
		return
	}
	resources := make([]gin.H, 0, len(records))
	for _, rec := range records {
		resources = append(resources, scimUserResource(rec))
	}
	if startIndex < 1 {
		startIndex = 1
	}
	writeSCIMJSON(c, http.StatusOK, gin.H{
		"schemas": []string{scimSchemaList}, "totalResults": total, "startIndex": startIndex,
		"itemsPerPage": len(resources), "Resources": resources,
	})
}

func (s *Server) scimGetUser(c *gin.Context) {
	ctx, cancel := scimContext(c)
	defer cancel()
	rec, err := s.scim.GetSCIMUser(ctx, scimAuthOf(c), c.Param("id"))
	if err == nil && rec.DeletedAt != nil {
		err = orgcore.ErrSCIMUserNotFound
	}
	if err != nil {
		writeSCIMUserError(c, "get", err)
		return
	}
	writeSCIMJSON(c, http.StatusOK, scimUserResource(*rec))
}

// scimBool reads an `active` value an IdP may send as a JSON bool or as "True"/"False".
func scimBool(raw json.RawMessage) (bool, bool) {
	var b bool
	if err := json.Unmarshal(raw, &b); err == nil {
		return b, true
	}
	var str string
	if err := json.Unmarshal(raw, &str); err == nil {
		switch strings.ToLower(strings.TrimSpace(str)) {
		case "true":
			return true, true
		case "false":
			return false, true
		}
	}
	return false, false
}

func decodeSCIMBody(c *gin.Context, into any) bool {
	decoder := json.NewDecoder(http.MaxBytesReader(c.Writer, c.Request.Body, scimMaxBodyBytes))
	// Unknown fields are expected: IdPs send the whole profile. They are ignored
	// and never stored.
	if err := decoder.Decode(into); err != nil {
		writeSCIMError(c, http.StatusBadRequest, "invalidSyntax", "The request body is not valid JSON.")
		return false
	}
	return true
}

func (s *Server) scimCreateUser(c *gin.Context) {
	var req struct {
		UserName   string          `json:"userName"`
		ExternalID string          `json:"externalId"`
		Active     json.RawMessage `json:"active"`
	}
	if !decodeSCIMBody(c, &req) {
		return
	}
	in := orgcore.SCIMUserInput{UserName: req.UserName, ExternalID: req.ExternalID}
	if len(req.Active) > 0 {
		active, ok := scimBool(req.Active)
		if !ok {
			writeSCIMError(c, http.StatusBadRequest, "invalidValue", "The active value is not valid.")
			return
		}
		in.Active = &active
	}
	ctx, cancel := scimContext(c)
	defer cancel()
	rec, err := s.scim.CreateSCIMUser(ctx, scimAuthOf(c), in)
	if err != nil {
		writeSCIMUserError(c, "create user", err)
		return
	}
	c.Header("Location", scimUsersLocation+rec.SCIMID)
	writeSCIMJSON(c, http.StatusCreated, scimUserResource(*rec))
}

// applyActive moves the user to the requested active state, or returns the user
// unchanged when no state was requested. Every other attribute is accepted and
// ignored: the identity key is immutable, so an IdP retry never fails on it.
func (s *Server) applyActive(c *gin.Context, desired *bool) {
	ctx, cancel := scimContext(c)
	defer cancel()
	auth := scimAuthOf(c)
	var rec *orgcore.SCIMUserRecord
	var err error
	if desired != nil {
		rec, err = s.scim.SetSCIMUserActive(ctx, auth, c.Param("id"), *desired)
	} else {
		rec, err = s.scim.GetSCIMUser(ctx, auth, c.Param("id"))
		if err == nil && rec.DeletedAt != nil {
			err = orgcore.ErrSCIMUserNotFound
		}
	}
	if err != nil {
		writeSCIMUserError(c, "update user", err)
		return
	}
	writeSCIMJSON(c, http.StatusOK, scimUserResource(*rec))
}

func (s *Server) scimPatchUser(c *gin.Context) {
	var req struct {
		Operations []struct {
			Op    string          `json:"op"`
			Path  string          `json:"path"`
			Value json.RawMessage `json:"value"`
		} `json:"Operations"`
	}
	if !decodeSCIMBody(c, &req) {
		return
	}
	var desired *bool
	for _, op := range req.Operations {
		switch strings.ToLower(strings.TrimSpace(op.Op)) {
		case "replace", "add":
		default:
			continue
		}
		path := strings.ToLower(strings.TrimSpace(op.Path))
		switch {
		case path == "active":
			if active, ok := scimBool(op.Value); ok {
				desired = &active
			}
		case path == "":
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(op.Value, &fields); err == nil {
				for key, raw := range fields {
					if strings.EqualFold(key, "active") {
						if active, ok := scimBool(raw); ok {
							desired = &active
						}
					}
				}
			}
		}
	}
	s.applyActive(c, desired)
}

func (s *Server) scimReplaceUser(c *gin.Context) {
	var req struct {
		Active json.RawMessage `json:"active"`
	}
	if !decodeSCIMBody(c, &req) {
		return
	}
	var desired *bool
	if len(req.Active) > 0 {
		active, ok := scimBool(req.Active)
		if !ok {
			writeSCIMError(c, http.StatusBadRequest, "invalidValue", "The active value is not valid.")
			return
		}
		desired = &active
	}
	s.applyActive(c, desired)
}

func (s *Server) scimDeleteUser(c *gin.Context) {
	ctx, cancel := scimContext(c)
	defer cancel()
	if err := s.scim.DeleteSCIMUser(ctx, scimAuthOf(c), c.Param("id")); err != nil {
		writeSCIMUserError(c, "delete user", err)
		return
	}
	c.Status(http.StatusNoContent)
}

func scimNotImplemented(c *gin.Context) {
	writeSCIMError(c, http.StatusNotImplemented, "", "Groups are not supported.")
}

func scimServiceProviderConfig(c *gin.Context) {
	writeSCIMJSON(c, http.StatusOK, gin.H{
		"schemas":               []string{scimSchemaConfig},
		"patch":                 gin.H{"supported": true},
		"bulk":                  gin.H{"supported": false, "maxOperations": 0, "maxPayloadSize": 0},
		"filter":                gin.H{"supported": true, "maxResults": 100},
		"changePassword":        gin.H{"supported": false},
		"sort":                  gin.H{"supported": false},
		"etag":                  gin.H{"supported": false},
		"authenticationSchemes": []gin.H{{"type": "oauthbearertoken", "name": "Bearer token", "description": "A SCIM bearer token created by an organization owner or admin."}},
	})
}

func scimResourceTypes(c *gin.Context) {
	user := gin.H{
		"schemas": []string{scimSchemaResource}, "id": "User", "name": "User",
		"endpoint": "/Users", "schema": scimSchemaUser,
	}
	writeSCIMJSON(c, http.StatusOK, gin.H{
		"schemas": []string{scimSchemaList}, "totalResults": 1, "startIndex": 1, "itemsPerPage": 1,
		"Resources": []gin.H{user},
	})
}

func scimSchemas(c *gin.Context) {
	attr := func(name, typ string, required bool, uniqueness string) gin.H {
		return gin.H{"name": name, "type": typ, "multiValued": false, "required": required, "mutability": "readWrite", "returned": "default", "uniqueness": uniqueness}
	}
	user := gin.H{
		"schemas": []string{scimSchemaSchema}, "id": scimSchemaUser, "name": "User",
		"description": "Identity keys only: userName, externalId and active. Other attributes are accepted and ignored.",
		"attributes": []gin.H{
			attr("userName", "string", true, "server"),
			attr("externalId", "string", false, "none"),
			attr("active", "boolean", false, "none"),
		},
	}
	writeSCIMJSON(c, http.StatusOK, gin.H{
		"schemas": []string{scimSchemaList}, "totalResults": 1, "startIndex": 1, "itemsPerPage": 1,
		"Resources": []gin.H{user},
	})
}
