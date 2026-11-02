package handler

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/userauthz"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

// WS14-S4: document sharing routes (R12). documents-api owns the document, so
// it is the service that proves the ACTOR owns it before asking Control to
// write a grant (owner-asserted mode, CONTROL_OWNER_ASSERTED_GRANTS). Control
// admits only this registered owner service, only with the actor's verified
// bearer and a v2 delegation naming the exact operation and document, and takes
// granted_by from the delegated user.
//
//	GET    /v1/documents/{id}/shares               owner; user and Space rows
//	POST   /v1/documents/{id}/shares               owner; {subject_id, role:"view"}
//	DELETE /v1/documents/{id}/shares/{subject_id}  owner; removes a USER grant
//	GET    /v1/documents/shared-with-me            documents granted to the viewer
//
// Every share route except shared-with-me is behind DOCUMENTS_SHARE_WRITES
// (strict literal true, off by default; a rollout control, never an
// authorization input). A document the actor does not own, or cannot see, is a
// 404 so existence is not revealed. Space-subject rows are listed read-only and
// are never removable here (the room-file delete path owns them).

const (
	maxSharesPerDocument = 200
	maxShareBodyBytes    = 4 << 10
	maxSubjectIDLen      = 200
)

// ShareGrantClient is the Control surface the share routes need;
// userauthz.Client implements it.
type ShareGrantClient interface {
	Grant(ctx context.Context, orgID, userID, authorization, documentID, subjectID, role string) (*userauthz.Grant, error)
	Revoke(ctx context.Context, orgID, userID, authorization, documentID, subjectID string) error
	ListGrants(ctx context.Context, orgID, userID, authorization, documentID string) ([]userauthz.Grant, error)
}

// SharesConfig carries the DOCUMENTS_SHARE_WRITES switch and the Control
// client. The zero value is "off".
type SharesConfig struct {
	Enabled bool
	Grants  ShareGrantClient
}

// WithShares wires the share routes. It returns the handler for chaining.
func (h *DocumentHandler) WithShares(cfg SharesConfig) *DocumentHandler {
	h.shares = cfg
	return h
}

type shareView struct {
	GrantID     string `json:"grant_id,omitempty"`
	SubjectType string `json:"subject_type"`
	SubjectID   string `json:"subject_id"`
	Role        string `json:"role"`
	GrantedBy   string `json:"granted_by"`
	GrantedAt   string `json:"granted_at"`
}

func toShareView(g userauthz.Grant) shareView {
	return shareView{GrantID: g.GrantID, SubjectType: g.SubjectType, SubjectID: g.SubjectID, Role: g.Role, GrantedBy: g.GrantedBy, GrantedAt: g.GrantedAt}
}

type shareActor struct {
	orgID         string
	userID        string
	authorization string
	documentID    string
	ownerID       string
}

// shareOwner runs the checks every share route shares and, on success, returns
// the verified actor with the document's owner proven. It writes the response
// and returns false otherwise.
func (h *DocumentHandler) shareOwner(w http.ResponseWriter, r *http.Request, write bool) (*shareActor, bool) {
	if !h.shares.Enabled {
		writeCodedError(w, http.StatusNotFound, "shares_disabled", "document sharing is not enabled", nil)
		return nil, false
	}
	claims := verifiedClaims(r)
	if claims == nil || claims.IsService() || strings.TrimSpace(claims.UserID) == "" {
		writeError(w, http.StatusForbidden, "an end-user token is required to manage shares")
		return nil, false
	}
	authorization, ok := authctx.AuthorizationHeader(r.Context())
	if !ok {
		writeError(w, http.StatusForbidden, "an end-user token is required to manage shares")
		return nil, false
	}
	if write {
		// Fails closed: a grant is durable state, so a missing or true
		// retention posture refuses it like any other durable write.
		if reason := persistentZDRReason(claims, nil); reason != "" {
			writeError(w, http.StatusForbidden, reason)
			return nil, false
		}
	}
	if h.shares.Grants == nil {
		writeCodedError(w, http.StatusServiceUnavailable, "sharing_unavailable", "sharing is temporarily unavailable", nil)
		return nil, false
	}
	orgID := OrgIDFrom(r.Context())
	docID := strings.TrimSpace(chi.URLParam(r, "documentID"))
	if docID == "" {
		writeError(w, http.StatusNotFound, "document not found")
		return nil, false
	}
	// The owner proof. Get applies the viewer predicate (owner, org-visible or
	// granted); canDeleteDocument then requires ownership (or the verified
	// org:data:write_all scope). Anything else is the same 404 as a missing row.
	document, err := h.repo.Get(r.Context(), orgID, docID, viewerID(r), nil)
	if err != nil || !canDeleteDocument(document, claims) {
		writeError(w, http.StatusNotFound, "document not found")
		return nil, false
	}
	return &shareActor{
		orgID: orgID, userID: claims.UserID, authorization: authorization,
		documentID: docID, ownerID: document.OwnerID,
	}, true
}

// writeShareControlError maps a Control failure. A 400 is the actor's mistake
// and keeps Control's code; a 403 or 404 means the owner-asserted mode is not
// open (rollout order) and is reported as unavailable, never as permission.
func writeShareControlError(w http.ResponseWriter, err error, operation string) {
	var statusErr *userauthz.StatusError
	if errors.As(err, &statusErr) && statusErr.Status == http.StatusBadRequest {
		code := statusErr.Code
		if code == "" {
			code = "share_invalid"
		}
		writeCodedError(w, http.StatusBadRequest, code, "the share request was refused", nil)
		return
	}
	log.Warn().Err(err).Str("operation", operation).Msg("documents: share call to Control failed")
	writeCodedError(w, http.StatusServiceUnavailable, "sharing_unavailable", "sharing is temporarily unavailable", nil)
}

// ListShares: GET /v1/documents/{documentID}/shares
func (h *DocumentHandler) ListShares(w http.ResponseWriter, r *http.Request) {
	actor, ok := h.shareOwner(w, r, false)
	if !ok {
		return
	}
	grants, err := h.shares.Grants.ListGrants(r.Context(), actor.orgID, actor.userID, actor.authorization, actor.documentID)
	if err != nil {
		writeShareControlError(w, err, "list_grants")
		return
	}
	out := make([]shareView, 0, len(grants))
	for _, g := range grants {
		out = append(out, toShareView(g))
	}
	writeJSON(w, http.StatusOK, map[string]any{"shares": out})
}

type createShareRequest struct {
	SubjectID string `json:"subject_id"`
	Role      string `json:"role"`
}

// CreateShare: POST /v1/documents/{documentID}/shares
func (h *DocumentHandler) CreateShare(w http.ResponseWriter, r *http.Request) {
	actor, ok := h.shareOwner(w, r, true)
	if !ok {
		return
	}
	var req createShareRequest
	decoder := json.NewDecoder(io.LimitReader(r.Body, maxShareBodyBytes))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&req); err != nil {
		writeCodedError(w, http.StatusBadRequest, "invalid_request", "invalid request body", nil)
		return
	}
	subject := strings.TrimSpace(req.SubjectID)
	if subject == "" || len(subject) > maxSubjectIDLen || strings.ContainsAny(subject, "\r\n\t ") {
		writeCodedError(w, http.StatusBadRequest, "invalid_request", "subject_id is required", nil)
		return
	}
	// Documents enforce no mutating role: `edit` is refused rather than stored
	// as access nothing checks (decision D-5). Control refuses it too.
	role := strings.ToLower(strings.TrimSpace(req.Role))
	if role == "" {
		role = "view"
	}
	if role != "view" {
		writeCodedError(w, http.StatusBadRequest, "role_not_supported", "role is not supported for documents", nil)
		return
	}
	if subject == actor.userID || subject == actor.ownerID {
		writeCodedError(w, http.StatusConflict, "self_share", "a document cannot be shared with its owner", nil)
		return
	}
	existing, err := h.shares.Grants.ListGrants(r.Context(), actor.orgID, actor.userID, actor.authorization, actor.documentID)
	if err != nil {
		writeShareControlError(w, err, "list_grants")
		return
	}
	userGrants, already := 0, false
	for _, g := range existing {
		if g.SubjectType != "user" {
			continue
		}
		userGrants++
		if g.SubjectID == subject {
			already = true
		}
	}
	if !already && userGrants >= maxSharesPerDocument {
		writeCodedError(w, http.StatusConflict, "share_limit_reached", "this document has reached its share limit",
			map[string]any{"max_shares": maxSharesPerDocument})
		return
	}
	grant, err := h.shares.Grants.Grant(r.Context(), actor.orgID, actor.userID, actor.authorization, actor.documentID, subject, role)
	if err != nil {
		writeShareControlError(w, err, "grant")
		return
	}
	writeJSON(w, http.StatusOK, toShareView(*grant))
}

// DeleteShare: DELETE /v1/documents/{documentID}/shares/{subjectID}
func (h *DocumentHandler) DeleteShare(w http.ResponseWriter, r *http.Request) {
	actor, ok := h.shareOwner(w, r, true)
	if !ok {
		return
	}
	subject := strings.TrimSpace(chi.URLParam(r, "subjectID"))
	if subject == "" || len(subject) > maxSubjectIDLen {
		writeCodedError(w, http.StatusBadRequest, "invalid_request", "subject_id is required", nil)
		return
	}
	// Only a user grant is removed here; Control is asked for subject_type=user
	// explicitly, so a Space row can never be reached through this route.
	if err := h.shares.Grants.Revoke(r.Context(), actor.orgID, actor.userID, actor.authorization, actor.documentID, subject); err != nil {
		writeShareControlError(w, err, "revoke")
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"success": true})
}

// SharedWithMe: GET /v1/documents/shared-with-me. Read only: the documents the
// viewer holds an explicit user grant on, minus their own. It uses the grant
// read that already existed, so it is not behind DOCUMENTS_SHARE_WRITES.
func (h *DocumentHandler) SharedWithMe(w http.ResponseWriter, r *http.Request) {
	claims := verifiedClaims(r)
	if claims == nil || claims.IsService() || strings.TrimSpace(claims.UserID) == "" {
		writeError(w, http.StatusForbidden, "an end-user token is required")
		return
	}
	authorization, ok := authctx.AuthorizationHeader(r.Context())
	if !ok || h.authz == nil {
		writeCodedError(w, http.StatusServiceUnavailable, "sharing_unavailable", "sharing is temporarily unavailable", nil)
		return
	}
	orgID := OrgIDFrom(r.Context())
	ids, err := h.authz.ListVisibleDocuments(r.Context(), orgID, claims.UserID, authorization)
	if err != nil {
		log.Warn().Err(err).Msg("documents: shared-with-me could not resolve grants")
		writeCodedError(w, http.StatusServiceUnavailable, "sharing_unavailable", "sharing is temporarily unavailable", nil)
		return
	}
	documents, err := h.repo.ListSharedWith(r.Context(), orgID, claims.UserID, ids, 100)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list shared documents")
		return
	}
	shared := make([]string, 0, len(documents))
	for _, d := range documents {
		shared = append(shared, d.DocumentID)
	}
	writeJSON(w, http.StatusOK, map[string]any{"ids": shared, "documents": documents, "all_org": false})
}
