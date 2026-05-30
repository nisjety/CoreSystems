package handler

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/events"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/validate"
)

type DocumentHandler struct {
	repo      *repo.DocumentRepo
	publisher *events.Publisher
}

func NewDocumentHandler(r *repo.DocumentRepo, p *events.Publisher) *DocumentHandler {
	return &DocumentHandler{repo: r, publisher: p}
}

func (h *DocumentHandler) Get(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	docID := chi.URLParam(r, "documentID")

	doc, err := h.repo.Get(r.Context(), orgID, docID)
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
	sources, total, err := h.repo.SourcesFacet(r.Context(), orgID)
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

	result, err := h.repo.List(r.Context(), model.ListDocumentsInput{
		OrgID:  orgID,
		Type:   docType,
		Limit:  limit,
		Offset: offset,
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

	if err := h.repo.SoftDelete(r.Context(), orgID, docID, ""); err != nil {
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

	var reused int
	var updated int
	for i, input := range req.Documents {
		input.OrgID = orgID
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
