package handler

import (
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/events"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/userauthz"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/validate"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

type DocumentHandler struct {
	repo *repo.DocumentRepo
	// authz resolves a viewer's explicit document grants from user-core. May be
	// nil (grant resolution disabled — owner + org/shared visibility still apply).
	authz *userauthz.Client
}

func NewDocumentHandler(r *repo.DocumentRepo, authz *userauthz.Client) *DocumentHandler {
	return &DocumentHandler{repo: r, authz: authz}
}

// viewerID returns identity only from cryptographically verified claims. An
// explicitly scoped org-wide reader has no per-user filter; all other callers
// retain private-until-shared visibility. Forwarded identity headers are never
// authorization inputs.
func viewerID(r *http.Request) string {
	if claims := verifiedClaims(r); claims != nil {
		if claims.HasScope("org:data:read_all") {
			return ""
		}
		return claims.PrincipalID()
	}
	return ""
}

func verifiedClaims(r *http.Request) *authctx.Claims {
	if claims, ok := authctx.FromContext(r.Context()); ok && claims.Verified {
		return claims
	}
	return nil
}

func principalID(r *http.Request) string {
	if claims := verifiedClaims(r); claims != nil {
		return claims.PrincipalID()
	}
	return ""
}

func eventUserID(r *http.Request) string {
	if claims := verifiedClaims(r); claims != nil && !claims.IsService() {
		return claims.UserID
	}
	return ""
}

func rejectsPersistentZDRContent(input *model.CreateDocumentInput) bool {
	return input != nil && input.IngestPolicy.IsZeroRetention()
}

func persistentZDRReason(claims *authctx.Claims, input *model.CreateDocumentInput) string {
	if claims == nil || !claims.Verified || !claims.ZDRPresent {
		return "verified retention posture is required for durable document persistence"
	}
	if claims.ZDR {
		return "verified token zdr=true forbids durable document persistence"
	}
	if rejectsPersistentZDRContent(input) {
		return "ingest_policy.zdr_mode=on or ephemeral_only=true forbids durable document persistence"
	}
	return ""
}

func lifecycleEventFactory(userID string) repo.OutboxEventFactory {
	return func(document *model.Document, updated bool) (string, []byte, error) {
		if updated {
			payload, err := json.Marshal(events.DocumentUpdatedEvent{
				DocumentID: document.DocumentID, OrgID: document.OrgID,
				Source: document.Source, Type: document.Type, Title: document.Title,
				UserID: userID, Visibility: document.Visibility, ZDR: false,
			})
			return events.SubjectDocUpdated, payload, err
		}
		payload, err := json.Marshal(events.DocumentCreatedEvent{
			DocumentID: document.DocumentID, OrgID: document.OrgID,
			Source: document.Source, Type: document.Type, Title: document.Title,
			UserID: userID, Visibility: document.Visibility, ZDR: false,
		})
		return events.SubjectDocCreated, payload, err
	}
}

// pinDocumentOwner prevents callers from assigning durable content to another
// principal. It returns true when a conflicting owner was supplied.
func pinDocumentOwner(input *model.CreateDocumentInput, principal string) bool {
	if input == nil || principal == "" {
		return true
	}
	if input.OwnerID != "" && input.OwnerID != principal {
		return true
	}
	input.OwnerID = principal
	input.CreatedBy = principal
	return false
}

func canDeleteDocument(document *model.Document, claims *authctx.Claims) bool {
	principal := claims.PrincipalID()
	if document == nil || principal == "" {
		return false
	}
	return document.OwnerID == principal || claims.HasScope("org:data:write_all")
}

// callerHasAdminScope reports whether the request carries a VERIFIED admin scope
// allowing org-global writes. HasScope returns false unless the JWT signature was
// verified, so until authctx verification is enforced this is false for end users
// (fail-closed) — only trusted system callers (no viewer) bypass the org gate.
func callerHasAdminScope(r *http.Request) bool {
	if claims := verifiedClaims(r); claims != nil {
		return claims.HasScope("org:data:write_all")
	}
	return false
}

// applyVisibilityPolicy resolves the document's visibility and enforces the
// org-ownership write rules (step 10 + 11):
//   - Default (empty visibility): a create that carries a viewer defaults to
//     PRIVATE (an end user's data is private until shared); a create with no
//     viewer at all defaults to ORG so shared knowledge doesn't silently vanish.
//     A connector that knows the source ACL should send `visibility` explicitly
//     rather than lean on either default — see the service rule below.
//   - Admin-gate: ORG-visible documents may be created by an admin (verified
//     scope), a caller with no viewer, or a verified SERVICE principal. An
//     interactive end user may create 'private' or 'shared' (and share via
//     grants) but never 'org'/tenant-global.
//
// The service carve-out exists because `viewerID` only blanks out for holders
// of `org:data:read_all`. Every other service principal — the SharePoint
// connector, Quarry — therefore looks exactly like an interactive end user
// here, so it was silently forced to `private` and forbidden from ever saying
// otherwise. That is what stranded connector-synced content: invisible to
// graph extraction and to every human in the org, owned by a service account
// nobody can act as. `IsService` is the distinction this policy always meant
// to draw (it requires a *verified* `principal_type == "service"`, i.e. a
// registered principal, not something a browser can assert).
//
// Returns true if the caller is FORBIDDEN from the requested visibility.
func applyVisibilityPolicy(r *http.Request, input *model.CreateDocumentInput) (forbidden bool) {
	viewer := viewerID(r)
	isService := verifiedClaims(r).IsService()
	v := strings.ToLower(strings.TrimSpace(input.Visibility))
	// Recorded before the defaulting below, so "the connector told us" stays
	// distinguishable from "we picked a default".
	input.VisibilityFromSource = isService && v != ""
	if v == "" {
		if viewer != "" {
			v = "private"
		} else {
			v = "org"
		}
	}
	if v == "org" && viewer != "" && !isService && !callerHasAdminScope(r) {
		return true
	}
	input.Visibility = v
	return false
}

// grantedDocs resolves the viewer's explicit document grants. It fails closed
// for grant-only content: on any error the viewer still sees owned + org-visible
// documents, while specifically shared documents remain hidden.
func (h *DocumentHandler) grantedDocs(r *http.Request, orgID, viewer string) []string {
	if viewer == "" || h.authz == nil {
		return nil
	}
	authorization, ok := authctx.AuthorizationHeader(r.Context())
	if !ok {
		return nil
	}
	ids, err := h.authz.ListVisibleDocuments(r.Context(), orgID, viewer, authorization)
	if err != nil {
		log.Warn().Err(err).Msg("documents: failed to resolve viewer grants; grant-only documents remain hidden")
		return nil
	}
	return ids
}

func (h *DocumentHandler) Get(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	docID := chi.URLParam(r, "documentID")

	viewer := viewerID(r)
	doc, err := h.repo.Get(r.Context(), orgID, docID, viewer, h.grantedDocs(r, orgID, viewer))
	if err != nil {
		writeError(w, http.StatusNotFound, "document not found")
		return
	}
	writeJSON(w, http.StatusOK, doc)
}

// Sources implements U1-2 (verevon ui-ux-verevon-gap.md §10): returns the
// distinct sources for an org with a per-source document count. The
// verevon dashboard's "Sources count" stat reads this — Quarry v2 writes
// every scrape into Data Plane v2 with its source URL, so the answer to
// "how many sources do I have" lives here, not in Quarry.
func (h *DocumentHandler) Sources(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	// Scope the facet to what this viewer may actually read (owned + org-visible
	// + explicitly granted), mirroring Get/List. Without this the facet counted
	// every org document regardless of ownership — a private-until-shared leak.
	viewer := viewerID(r)
	sources, total, err := h.repo.SourcesFacet(r.Context(), orgID, viewer, h.grantedDocs(r, orgID, viewer))
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to facet sources")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"sources": sources,
		"total":   total,
	})
}

func (h *DocumentHandler) List(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
	docType := r.URL.Query().Get("type")

	viewer := viewerID(r)
	result, err := h.repo.List(r.Context(), model.ListDocumentsInput{
		OrgID:      orgID,
		Type:       docType,
		Limit:      limit,
		Offset:     offset,
		ViewerID:   viewer,
		GrantedIDs: h.grantedDocs(r, orgID, viewer),
	})
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list documents")
		return
	}
	writeJSON(w, http.StatusOK, result)
}

func (h *DocumentHandler) Create(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())

	var input model.CreateDocumentInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	spaceImportAuthority, err := verifySpaceImportDecision(r)
	if err != nil {
		writeError(w, http.StatusForbidden, "invalid Space import authority")
		return
	}
	if spaceImportAuthority != nil && !spaceImportAuthority.matchesDocumentInputType(input.Type) {
		writeError(w, http.StatusForbidden, "Space import authority does not match document source type")
		return
	}
	if pinDocumentOwner(&input, principalID(r)) {
		writeError(w, http.StatusForbidden, "owner_id must match the verified principal")
		return
	}

	// Step 10+11: default visibility (private for users, org for system ingest)
	// and admin-gate org-global writes.
	if applyVisibilityPolicy(r, &input) {
		writeError(w, http.StatusForbidden,
			"only an admin can create org-visible documents; create as 'private' or 'shared' and share via grants")
		return
	}

	if err := validate.OrgID(orgID); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	// ZDR enforcement at the receive boundary: this durable-document API cannot
	// create even a metadata row under an ephemeral-only policy.
	if reason := persistentZDRReason(verifiedClaims(r), &input); reason != "" {
		writeError(w, http.StatusForbidden, reason)
		return
	}

	if err := validate.CreateDocument(&input); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	result, err := h.repo.CreateWithOutbox(r.Context(), input, lifecycleEventFactory(eventUserID(r)))
	if err != nil {
		if errors.Is(err, repo.ErrIdempotencyOwnershipConflict) {
			writeError(w, http.StatusConflict, "idempotency key is unavailable")
			return
		}
		writeError(w, http.StatusInternalServerError, "failed to create document")
		return
	}

	status := http.StatusCreated
	if result.Reused || result.Updated {
		status = http.StatusOK
	}
	if spaceImportAuthority != nil {
		w.Header().Set(spaceImportAcceptedHeader, "true")
	}
	writeJSON(w, status, result.Document)
}

func (h *DocumentHandler) Delete(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	docID := chi.URLParam(r, "documentID")

	// Ownership gate: a caller may only delete a document they can actually see.
	// Without this, any org member could delete a private doc they cannot read by
	// guessing its id (confused deputy). A no-viewer (service) caller keeps the
	// legacy org-scoped behaviour. deletedBy records who performed the deletion.
	viewer := viewerID(r)
	document, err := h.repo.Get(r.Context(), orgID, docID, viewer, h.grantedDocs(r, orgID, viewer))
	if err != nil {
		writeError(w, http.StatusNotFound, "document not found")
		return
	}
	claims := verifiedClaims(r)
	if !canDeleteDocument(document, claims) {
		writeError(w, http.StatusNotFound, "document not found")
		return
	}

	deleteEvent := events.DocumentDeletedEvent{
		DocumentID: docID,
		OrgID:      orgID,
		UserID:     eventUserID(r),
		Visibility: document.Visibility,
		ZDR:        false,
	}
	payload, err := json.Marshal(deleteEvent)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to prepare deletion")
		return
	}
	if err := h.repo.SoftDeleteWithOutbox(
		r.Context(), orgID, docID, claims.PrincipalID(), events.SubjectDocDeleted, payload,
	); err != nil {
		writeError(w, http.StatusNotFound, "document not found")
		return
	}

	writeJSON(w, http.StatusOK, map[string]bool{"success": true})
}

// BulkIngest — §16.2.6 outbox pattern.
//
// The repo.Create call commits the document row in its own tx. We then
// write to the `documents_outbox` table BEFORE publishing the NATS event.
// On crash between commit and publish the outbox row stays unpublished;
// the background publisher loop re-emits on the next tick. Net effect:
// at-least-once delivery without orphaned document rows.
func (h *DocumentHandler) BulkIngest(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())

	var req struct {
		Documents []model.CreateDocumentInput `json:"documents"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	if err := validate.OrgID(orgID); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if err := validate.BulkBatchSize(len(req.Documents)); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	var accepted int
	var rejected int
	var ids []string
	rejectionReasons := []string{}

	bulkPrincipal := principalID(r)
	bulkClaims := verifiedClaims(r)
	var reused int
	var updated int
	for i, input := range req.Documents {
		input.OrgID = orgID
		if reason := persistentZDRReason(bulkClaims, &input); reason != "" {
			rejected++
			rejectionReasons = append(rejectionReasons, fmt.Sprintf("doc[%d]: %s", i, reason))
			continue
		}
		if pinDocumentOwner(&input, bulkPrincipal) {
			rejected++
			rejectionReasons = append(rejectionReasons, fmt.Sprintf("doc[%d]: owner_id must match the verified principal", i))
			continue
		}
		// Step 10+11: default visibility + admin-gate org-global writes (per doc).
		if applyVisibilityPolicy(r, &input) {
			rejected++
			rejectionReasons = append(rejectionReasons, fmt.Sprintf("doc[%d]: only an admin can create org-visible documents", i))
			continue
		}
		if err := validate.CreateDocument(&input); err != nil {
			rejected++
			rejectionReasons = append(rejectionReasons, fmt.Sprintf("doc[%d]: %s", i, err.Error()))
			continue
		}
		result, err := h.repo.CreateWithOutbox(r.Context(), input, lifecycleEventFactory(eventUserID(r)))
		if err != nil {
			rejected++
			continue
		}
		ids = append(ids, result.Document.DocumentID)

		// §16.2.6 — write to documents_outbox instead of publishing directly.
		// The outbox publisher loop (started in main) drains within ~500ms.
		// Trade: small latency hit; gain: at-least-once delivery even if
		// NATS is down at this moment.
		switch {
		case result.Reused:
			reused++
		case result.Updated:
			updated++
		default:
			accepted++
		}
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"accepted":          accepted,
		"updated":           updated,
		"rejected":          rejected,
		"reused":            reused,
		"document_ids":      ids,
		"rejection_reasons": rejectionReasons,
	})
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}

func writeError(w http.ResponseWriter, status int, msg string) {
	writeJSON(w, status, map[string]string{"error": msg})
}
