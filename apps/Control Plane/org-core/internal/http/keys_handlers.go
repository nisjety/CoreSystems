package http

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/orgkeys"
	"github.com/gin-gonic/gin"
)

// Key custody routes (WS17.E2, X-8) under /internal/orgs/:orgId/keys. They are
// registered only when ORG_CORE_KEYS_ENABLED=1 (default 0, the image ships dark).
// Machine principals only, no acting user:
//
//	POST   .../keys/data-keys         org:keys:wrap:any     new DEK, plaintext + wrapped
//	POST   .../keys/data-keys/unwrap  org:keys:unwrap:any   plaintext DEK of a wrapped one
//	POST   .../keys/data-keys/rewrap  org:keys:wrap:any     wrapped DEK moved to the active KEK
//	POST   .../keys/rotate            org:keys:admin:any    new active KEK version
//	GET    .../keys                   org:keys:admin:any    versions and states, no key material
//	DELETE .../keys                   org:keys:destroy:any  crypto-shred every KEK of the org
//
// The admin scope (org:keys:admin:any, rotate and metadata) is not in
// orgServiceScopeAllowlist, so no credential can hold it and those two routes
// answer 403 until a reviewed registry change adds it (for the operator). The
// destroy scope (org:keys:destroy:any, DELETE) is allowlisted and pinned to
// quarry-edge alone, for the org erasure saga's shred hop (Q2-WS17.E2a; WS17.E3
// and U41b).
//
// Bodies are never logged, every answer is Cache-Control: no-store, and no error
// message carries key material.

const (
	orgKeysMaxBodyBytes = 8 << 10
	orgKeysTimeout      = 5 * time.Second

	orgKeysCodeOrgNotFound    = "org_not_found"
	orgKeysCodeKeyNotFound    = "key_not_found"
	orgKeysCodeDestroyed      = "key_destroyed"
	orgKeysCodeInvalidContext = "invalid_context"
	orgKeysCodeInvalidBody    = "invalid_body"
	orgKeysCodeUnavailable    = "keys_unavailable"
)

// orgKeyService is the seam the routes depend on; *orgkeys.Service satisfies it.
type orgKeyService interface {
	GenerateDataKey(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string) (orgkeys.DataKey, error)
	Unwrap(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string, keyID string, wrapped []byte) ([]byte, int, error)
	Rewrap(ctx context.Context, orgID, principal, purpose string, keyContext map[string]string, keyID string, wrapped []byte) (orgkeys.DataKey, error)
	Rotate(ctx context.Context, orgID, principal string) (orgkeys.VersionMetadata, error)
	Metadata(ctx context.Context, orgID string) (orgkeys.Metadata, error)
	Destroy(ctx context.Context, orgID, principal string) (int, error)
}

// ValidateOrgKeysConfig is the boot assertion for cmd/server: with
// ORG_CORE_KEYS_ENABLED=1 the master keys must parse, or org-core refuses to start.
func ValidateOrgKeysConfig(getenv func(string) string) error {
	return orgkeys.ValidateConfig(getenv)
}

// SetOrgKeys wires the key service. Like SetSIEMDestinations it is a setter so
// NewServer keeps its signature; with the routes on and no service they answer 503.
func (s *Server) SetOrgKeys(service *orgkeys.Service) {
	if service != nil {
		s.orgKeys = service
	}
}

func (s *Server) registerOrgKeyRoutes(internal *gin.RouterGroup) {
	if !s.keysOn {
		return
	}
	keys := internal.Group("/orgs/:orgId/keys")
	keys.POST("/data-keys", s.createOrgDataKey)
	keys.POST("/data-keys/unwrap", s.unwrapOrgDataKey)
	keys.POST("/data-keys/rewrap", s.rewrapOrgDataKey)
	keys.POST("/rotate", s.rotateOrgKeys)
	keys.GET("", s.getOrgKeysMetadata)
	keys.DELETE("", s.destroyOrgKeys)
}

type orgKeyRequest struct {
	Purpose       string            `json:"purpose"`
	Context       map[string]string `json:"context"`
	KeyID         string            `json:"key_id"`
	WrappedDEKB64 string            `json:"wrapped_dek_b64"`
}

func writeOrgKeysError(c *gin.Context, status int, code, message string) {
	c.Header("Cache-Control", "no-store")
	c.AbortWithStatusJSON(status, gin.H{"error": gin.H{"code": code, "message": message, "details": gin.H{}}})
}

// writeOrgKeysServiceError maps the typed errors. Only the code and the org id
// are logged; the request body never is.
func writeOrgKeysServiceError(c *gin.Context, op, orgID string, err error) {
	switch {
	case errors.Is(err, orgkeys.ErrOrgNotFound):
		writeOrgKeysError(c, http.StatusNotFound, orgKeysCodeOrgNotFound, "organization not found")
	case errors.Is(err, orgkeys.ErrNoKey):
		writeOrgKeysError(c, http.StatusNotFound, orgKeysCodeKeyNotFound, "key version not found")
	case errors.Is(err, orgkeys.ErrDestroyed):
		writeOrgKeysError(c, http.StatusGone, orgKeysCodeDestroyed, "the organization's keys were destroyed")
	case errors.Is(err, orgkeys.ErrInvalidContext):
		writeOrgKeysError(c, http.StatusUnprocessableEntity, orgKeysCodeInvalidContext, "purpose, context or wrapped key is not valid for this organization")
	default:
		// ErrUnavailable texts name a master key id or a store fault, never a key.
		log.Printf("org keys: %s org=%s unavailable: %v", op, orgID, err)
		writeOrgKeysError(c, http.StatusServiceUnavailable, orgKeysCodeUnavailable, "keys unavailable")
	}
}

// orgKeysPreamble checks the service, the org id and the principal. It writes the
// refusal itself and returns ok=false when the request must stop.
func (s *Server) orgKeysPreamble(c *gin.Context) (orgID, principal string, ok bool) {
	c.Header("Cache-Control", "no-store")
	orgID = c.Param("orgId")
	if orgID == "" || orgID != strings.TrimSpace(orgID) || len(orgID) > 128 {
		writeOrgKeysError(c, http.StatusBadRequest, orgKeysCodeInvalidBody, "organization id is required")
		return "", "", false
	}
	if s.orgKeys == nil {
		log.Printf("org keys: no key service configured for org=%s", orgID)
		writeOrgKeysError(c, http.StatusServiceUnavailable, orgKeysCodeUnavailable, "keys unavailable")
		return "", "", false
	}
	principal = c.GetString("service_id")
	if principal == "" {
		writeOrgKeysError(c, http.StatusForbidden, "forbidden", "service principal required")
		return "", "", false
	}
	return orgID, principal, true
}

// readOrgKeyRequest decodes a bounded body with no unknown fields. wantWrapped
// says whether key_id and wrapped_dek_b64 are required (unwrap, rewrap) or
// forbidden (data-keys).
func readOrgKeyRequest(c *gin.Context, wantWrapped bool) (orgKeyRequest, []byte, bool) {
	var request orgKeyRequest
	body, err := io.ReadAll(io.LimitReader(c.Request.Body, orgKeysMaxBodyBytes+1))
	if err != nil || len(body) > orgKeysMaxBodyBytes {
		writeOrgKeysError(c, http.StatusBadRequest, orgKeysCodeInvalidBody, "request body is too large or unreadable")
		return request, nil, false
	}
	decoder := json.NewDecoder(strings.NewReader(string(body)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil || decoder.More() {
		writeOrgKeysError(c, http.StatusBadRequest, orgKeysCodeInvalidBody, "request body must be one JSON object with purpose and context")
		return request, nil, false
	}
	if request.Purpose == "" || request.Context == nil {
		writeOrgKeysError(c, http.StatusBadRequest, orgKeysCodeInvalidBody, "purpose and context are required")
		return request, nil, false
	}
	if !wantWrapped {
		if request.KeyID != "" || request.WrappedDEKB64 != "" {
			writeOrgKeysError(c, http.StatusBadRequest, orgKeysCodeInvalidBody, "key_id and wrapped_dek_b64 are not accepted here")
			return request, nil, false
		}
		return request, nil, true
	}
	if request.KeyID == "" || request.WrappedDEKB64 == "" {
		writeOrgKeysError(c, http.StatusBadRequest, orgKeysCodeInvalidBody, "key_id and wrapped_dek_b64 are required")
		return request, nil, false
	}
	wrapped, err := base64.StdEncoding.DecodeString(request.WrappedDEKB64)
	if err != nil {
		writeOrgKeysError(c, http.StatusUnprocessableEntity, orgKeysCodeInvalidContext, "wrapped_dek_b64 is not base64")
		return request, nil, false
	}
	return request, wrapped, true
}

func zeroBytes(b []byte) {
	for i := range b {
		b[i] = 0
	}
}

func (s *Server) createOrgDataKey(c *gin.Context) {
	orgID, principal, ok := s.orgKeysPreamble(c)
	if !ok {
		return
	}
	request, _, ok := readOrgKeyRequest(c, false)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgKeysTimeout)
	defer cancel()
	key, err := s.orgKeys.GenerateDataKey(ctx, orgID, principal, request.Purpose, request.Context)
	if err != nil {
		writeOrgKeysServiceError(c, "data-keys", orgID, err)
		return
	}
	dek := base64.StdEncoding.EncodeToString(key.DEK)
	zeroBytes(key.DEK)
	c.JSON(http.StatusCreated, gin.H{"data": gin.H{
		"key_id":          key.KeyID,
		"kek_version":     key.KEKVersion,
		"alg":             orgkeys.Algorithm,
		"dek_b64":         dek,
		"wrapped_dek_b64": base64.StdEncoding.EncodeToString(key.WrappedDEK),
	}})
}

func (s *Server) unwrapOrgDataKey(c *gin.Context) {
	orgID, principal, ok := s.orgKeysPreamble(c)
	if !ok {
		return
	}
	request, wrapped, ok := readOrgKeyRequest(c, true)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgKeysTimeout)
	defer cancel()
	dek, current, err := s.orgKeys.Unwrap(ctx, orgID, principal, request.Purpose, request.Context, request.KeyID, wrapped)
	if err != nil {
		writeOrgKeysServiceError(c, "unwrap", orgID, err)
		return
	}
	encoded := base64.StdEncoding.EncodeToString(dek)
	zeroBytes(dek)
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"key_id":              request.KeyID,
		"dek_b64":             encoded,
		"current_kek_version": current,
	}})
}

func (s *Server) rewrapOrgDataKey(c *gin.Context) {
	orgID, principal, ok := s.orgKeysPreamble(c)
	if !ok {
		return
	}
	request, wrapped, ok := readOrgKeyRequest(c, true)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgKeysTimeout)
	defer cancel()
	key, err := s.orgKeys.Rewrap(ctx, orgID, principal, request.Purpose, request.Context, request.KeyID, wrapped)
	if err != nil {
		writeOrgKeysServiceError(c, "rewrap", orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"key_id":          key.KeyID,
		"kek_version":     key.KEKVersion,
		"alg":             orgkeys.Algorithm,
		"wrapped_dek_b64": base64.StdEncoding.EncodeToString(key.WrappedDEK),
	}})
}

func versionMetadataJSON(version orgkeys.VersionMetadata) gin.H {
	out := gin.H{
		"kek_version":   version.Version,
		"state":         version.State,
		"master_key_id": version.MasterKeyID,
		"created_at":    version.CreatedAt.UTC().Format(time.RFC3339),
	}
	if version.RetiredAt != nil {
		out["retired_at"] = version.RetiredAt.UTC().Format(time.RFC3339)
	}
	if version.DestroyedAt != nil {
		out["destroyed_at"] = version.DestroyedAt.UTC().Format(time.RFC3339)
	}
	return out
}

func (s *Server) rotateOrgKeys(c *gin.Context) {
	orgID, principal, ok := s.orgKeysPreamble(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgKeysTimeout)
	defer cancel()
	version, err := s.orgKeys.Rotate(ctx, orgID, principal)
	if err != nil {
		writeOrgKeysServiceError(c, "rotate", orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": versionMetadataJSON(version)})
}

func (s *Server) getOrgKeysMetadata(c *gin.Context) {
	orgID, _, ok := s.orgKeysPreamble(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgKeysTimeout)
	defer cancel()
	meta, err := s.orgKeys.Metadata(ctx, orgID)
	if err != nil {
		writeOrgKeysServiceError(c, "metadata", orgID, err)
		return
	}
	versions := make([]gin.H, 0, len(meta.Versions))
	for _, version := range meta.Versions {
		versions = append(versions, versionMetadataJSON(version))
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"org_id":             meta.OrgID,
		"state":              meta.State,
		"active_kek_version": meta.ActiveVersion,
		"versions":           versions,
	}})
}

func (s *Server) destroyOrgKeys(c *gin.Context) {
	orgID, principal, ok := s.orgKeysPreamble(c)
	if !ok {
		return
	}
	ctx, cancel := context.WithTimeout(c.Request.Context(), orgKeysTimeout)
	defer cancel()
	count, err := s.orgKeys.Destroy(ctx, orgID, principal)
	if err != nil {
		writeOrgKeysServiceError(c, "destroy", orgID, err)
		return
	}
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"org_id":             orgID,
		"state":              orgkeys.StateDestroyed,
		"versions_destroyed": count,
	}})
}
