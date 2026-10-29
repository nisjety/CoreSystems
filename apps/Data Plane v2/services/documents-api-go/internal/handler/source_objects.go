package handler

import (
	"encoding/json"
	"net/http"
	"strconv"
	"strings"

	"github.com/triodelab/dataplane/services/documents-api-go/internal/events"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/model"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/repo"
	"github.com/triodelab/dataplane/services/documents-api-go/internal/validate"
	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

type SourceObjectHandler struct {
	repo    *repo.SourceObjectRepo
	subject events.SourceObjectSubjects
}

func NewSourceObjectHandler(sourceObjects *repo.SourceObjectRepo) *SourceObjectHandler {
	return &SourceObjectHandler{
		repo:    sourceObjects,
		subject: events.DefaultSourceObjectSubjects(),
	}
}

func (h *SourceObjectHandler) Upsert(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	if reason := sourceObjectPersistenceZDRReason(verifiedClaims(r)); reason != "" {
		writeError(w, http.StatusForbidden, reason)
		return
	}
	var input model.UpsertSourceObjectInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	normalizeSourceObjectInput(&input)

	if err := validate.OrgID(orgID); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if err := validate.UpsertSourceObject(&input); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	result, err := h.repo.UpsertWithOutbox(
		r.Context(),
		input,
		h.subject.Changed,
		eventUserID(r),
	)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to upsert source object")
		return
	}

	status := http.StatusOK
	if result.Inserted {
		status = http.StatusCreated
	}
	writeJSON(w, status, result.SourceObject)
}

func sourceObjectPersistenceZDRReason(claims *authctx.Claims) string {
	if claims == nil || !claims.Verified || !claims.ZDRPresent {
		return "verified retention posture is required for durable source-object persistence"
	}
	if claims.ZDR {
		return "verified token zdr=true forbids durable source-object persistence"
	}
	return ""
}

func (h *SourceObjectHandler) Delete(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	var input model.DeleteSourceObjectInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	input.SourceObjectID = strings.TrimSpace(input.SourceObjectID)
	input.Connector = strings.TrimSpace(input.Connector)
	input.ExternalID = strings.TrimSpace(input.ExternalID)

	if err := validate.OrgID(orgID); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	if err := validate.DeleteSourceObject(&input); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	_, err := h.repo.SoftDeleteWithOutbox(
		r.Context(),
		input,
		h.subject.Deleted,
		eventUserID(r),
	)
	if err != nil {
		writeError(w, http.StatusNotFound, "source object not found")
		return
	}

	writeJSON(w, http.StatusOK, map[string]bool{"success": true})
}

func (h *SourceObjectHandler) Duplicates(w http.ResponseWriter, r *http.Request) {
	orgID := OrgIDFrom(r.Context())
	if err := validate.OrgID(orgID); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	query := r.URL.Query()
	input := model.ListSourceObjectDuplicatesInput{
		OrgID:        orgID,
		Source:       strings.TrimSpace(query.Get("source")),
		MinCount:     boundedIntQuery(query.Get("min_count"), 2, 2, 1000),
		MinSizeBytes: boundedInt64Query(query.Get("min_size"), 0),
		MaxGroups:    boundedIntQuery(query.Get("max_groups"), 100, 1, 500),
	}

	groups, err := h.repo.Duplicates(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list source object duplicates")
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"success": true,
		"data": map[string]any{
			"min_count":  input.MinCount,
			"min_size":   input.MinSizeBytes,
			"max_groups": input.MaxGroups,
			"source":     input.Source,
			"count":      len(groups),
			"groups":     groups,
		},
	})
}

func normalizeSourceObjectInput(input *model.UpsertSourceObjectInput) {
	input.Connector = strings.TrimSpace(input.Connector)
	input.Source = strings.TrimSpace(input.Source)
	input.ExternalID = strings.TrimSpace(input.ExternalID)
	input.SiteID = strings.TrimSpace(input.SiteID)
	input.DriveID = strings.TrimSpace(input.DriveID)
	input.ItemID = strings.TrimSpace(input.ItemID)
	input.ParentID = strings.TrimSpace(input.ParentID)
	input.Path = strings.TrimSpace(input.Path)
	input.Name = strings.TrimSpace(input.Name)
	input.MimeType = strings.TrimSpace(input.MimeType)
	input.ETag = strings.TrimSpace(input.ETag)
	input.CTag = strings.TrimSpace(input.CTag)
	input.QuickXorHash = strings.TrimSpace(input.QuickXorHash)
	input.SHA1Hash = strings.TrimSpace(input.SHA1Hash)
	input.ContentHash = strings.TrimSpace(input.ContentHash)
}

func boundedIntQuery(raw string, def, lo, hi int) int {
	if raw == "" {
		return def
	}
	v, err := strconv.Atoi(raw)
	if err != nil {
		return def
	}
	if v < lo {
		return lo
	}
	if v > hi {
		return hi
	}
	return v
}

func boundedInt64Query(raw string, def int64) int64 {
	if raw == "" {
		return def
	}
	v, err := strconv.ParseInt(raw, 10, 64)
	if err != nil || v < 0 {
		return def
	}
	return v
}
