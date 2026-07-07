package handler

import (
	"encoding/json"
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
	repo      *repo.DocumentRepo
	publisher *events.Publisher
	// authz resolves a viewer's explicit document grants from user-core. May be
	// nil (grant resolution disabled — owner + org/shared visibility still apply).
	authz *userauthz.Client
}

func NewDocumentHandler(r *repo.DocumentRepo, p *events.Publisher, authz *userauthz.Client) *DocumentHandler {
	return &DocumentHandler{repo: r, publisher: p, authz: authz}
}

// viewerID returns the requesting user's id: a verified authctx JWT claim wins;
// otherwise the X-User-Id header forwarded by the gateway (the route is already
// behind the internal-key gate, so the header is from a trusted caller). Empty
// → ownership filtering is skipped (legacy org-scoped behaviour).
func viewerID(r *http.Request) string {
	if claims, ok := authctx.FromContext(r.Context()); ok && claims.UserID != "" {
		return claims.UserID
	}
	return strings.TrimSpace(r.Header.Get("X-User-Id"))
}

// callerHasAdminScope reports whether the request carries a VERIFIED admin scope
// allowing org-global writes. HasScope returns false unless the JWT signature was
// verified, so until authctx verification is enforced this is false for end users
// (fail-closed) — only trusted system callers (no viewer) bypass the org gate.
func callerHasAdminScope(r *http.Request) bool {
	if claims, ok := authctx.FromContext(r.Context()); ok {
		return claims.HasScope("org:data:write_all")
	}
	return false
}

// applyVisibilityPolicy resolves the document's visibility and enforces the
// org-ownership write rules (step 10 + 11):
//   - Default (empty visibility): an interactive end-user create defaults to
//     PRIVATE (their data is private until shared); a system/ingest create (no
//     viewer — Quarry crawls, connectors) defaults to ORG so shared knowledge
//     stays org-visible and doesn't silently vanish.
//   - Admin-gate: only an admin (verified scope) or a trusted system caller (no
//     viewer) may create ORG-visible docs. An end user may create 'private' or
//     'shared' (and share via grants) but never 'org'/tenant-global.
//
// Returns true if the caller is FORBIDDEN from the requested visibility.
func applyVisibilityPolicy(r *http.Request, input *model.CreateDocumentInput) (forbidden bool) {
	viewer := viewerID(r)
	v := strings.ToLower(strings.TrimSpace(input.Visibility))
	if v == "" {
		if viewer != "" {
			v = "private"
		} else {
			v = "org"
		}
	}
	if v == "org" && viewer != "" && !callerHasAdminScope(r) {
		return true
	}
	input.Visibility = v
	return false
}

// grantedDocs resolves the viewer's explicit document grants. Fails OPEN: on any
// error it returns nil so the viewer still sees owned + org/shared docs (never a
// leak — at worst a doc shared specifically to them is briefly hidden).
func (h *DocumentHandler) grantedDocs(r *http.Request, orgID, viewer string) []string {
	if viewer == "" || h.authz == nil {
		return nil
	}
	ids, err := h.authz.ListVisibleDocuments(r.Context(), orgID, viewer)
	if err != nil {
		log.Warn().Err(err).Msg("documents: failed to resolve viewer grants; failing open to owner+org/shared")
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

// Sources implements U1-2 (velion ui-ux-velion-gap.md §10): returns the
// distinct sources for an org with a per-source document count. The
// velion dashboard's "Sources count" stat reads this — Quarry v2 writes
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
	// Stamp the creator as owner when an interactive viewer is present and the
	// caller did not specify one. Server-to-server ingests (no viewer) fall back
	// to created_by / the system account in the repo.
	if input.OwnerID == "" {
		if v := viewerID(r); v != "" {
			input.OwnerID = v
		}
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

	// ZDR enforcement at the receive boundary — Quarry's IngestClient
	// pre-checks too, but defense in depth: reject ANY incoming request
	// that combines a non-empty content body with an ephemeral/ZDR policy.
	if input.IngestPolicy.IsZeroRetention() && input.Content != "" {
		writeError(w, http.StatusForbidden,
			"ingest_policy.zdr_mode=on or ephemeral_only=true rejects requests with non-empty content; submit metadata-only or disable ZDR")
		return
	}

	if err := validate.CreateDocument(&input); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	result, err := h.repo.Create(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create document")
		return
	}

	// Emit the right lifecycle event: created for new docs, updated when an
	// idempotent re-ingest changed the content. A true no-op (Reused) fires
	// nothing so chunking/embedding don't re-run needlessly.
	switch {
	case result.Updated:
		_ = h.publisher.PublishDocumentUpdated(events.DocumentUpdatedEvent{
			DocumentID: result.Document.DocumentID,
			OrgID:      result.Document.OrgID,
			Source:     result.Document.Source,
			Type:       result.Document.Type,
			Title:      result.Document.Title,
		})
	case !result.Reused:
		_ = h.publisher.PublishDocumentCreated(events.DocumentCreatedEvent{
			DocumentID: result.Document.DocumentID,
			OrgID:      result.Document.OrgID,
			Source:     result.Document.Source,
			Type:       result.Document.Type,
			Title:      result.Document.Title,
		})
	}

	status := http.StatusCreated
	if result.Reused || result.Updated {
		status = http.StatusOK
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
	if _, err := h.repo.Get(r.Context(), orgID, docID, viewer, h.grantedDocs(r, orgID, viewer)); err != nil {
		writeError(w, http.StatusNotFound, "document not found")
		return
	}

	if err := h.repo.SoftDelete(r.Context(), orgID, docID, viewer); err != nil {
		writeError(w, http.StatusNotFound, "document not found")
		return
	}

	_ = h.publisher.PublishDocumentDeleted(events.DocumentDeletedEvent{
		DocumentID: docID,
		OrgID:      orgID,
	})

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

	bulkViewer := viewerID(r)
	var reused int
	var updated int
	for i, input := range req.Documents {
		input.OrgID = orgID
		if input.OwnerID == "" && bulkViewer != "" {
			input.OwnerID = bulkViewer
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
		result, err := h.repo.Create(r.Context(), input)
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
			evtPayload, _ := json.Marshal(events.DocumentUpdatedEvent{
				DocumentID: result.Document.DocumentID,
				OrgID:      result.Document.OrgID,
				Source:     result.Document.Source,
				Type:       result.Document.Type,
				Title:      result.Document.Title,
			})
			_ = h.repo.EnqueueOutbox(r.Context(), result.Document.OrgID, events.SubjectDocUpdated, evtPayload)
		default:
			accepted++
			evtPayload, _ := json.Marshal(events.DocumentCreatedEvent{
				DocumentID: result.Document.DocumentID,
				OrgID:      result.Document.OrgID,
				Source:     result.Document.Source,
				Type:       result.Document.Type,
				Title:      result.Document.Title,
			})
			_ = h.repo.EnqueueOutbox(r.Context(), result.Document.OrgID, events.SubjectDocCreated, evtPayload)
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
