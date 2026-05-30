package handler

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strconv"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/dataplane/services/wiki-store-go/internal/model"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/repo"
	"github.com/triodelab/dataplane/services/wiki-store-go/internal/sanitize"
)

// applySafeHTML populates §16.5.4 fields on a wiki version, doing nothing
// when the version is nil or has no content. Safe to call on every read
// path; bluemonday allocates but is fast enough for per-request use.
func applySafeHTML(v *model.WikiPageVersion) {
	if v == nil || v.Content == nil {
		return
	}
	safe, ok := sanitize.Sanitize(*v.Content)
	v.SafeHTML = &safe
	v.SafeHTMLOK = ok
}

type contextKey string

const orgIDKey contextKey = "org_id"

func OrgIDMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		orgID := r.Header.Get("X-Org-ID")
		if orgID == "" {
			writeError(w, http.StatusBadRequest, "X-Org-ID header required")
			return
		}
		ctx := context.WithValue(r.Context(), orgIDKey, orgID)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func orgIDFrom(ctx context.Context) string {
	v, _ := ctx.Value(orgIDKey).(string)
	return v
}

type WikiHandler struct {
	repo *repo.WikiRepo
}

func NewWikiHandler(r *repo.WikiRepo) *WikiHandler {
	return &WikiHandler{repo: r}
}

func (h *WikiHandler) GetPage(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")

	page, err := h.repo.GetPage(r.Context(), orgID, pageID)
	if err != nil {
		writeError(w, http.StatusNotFound, "page not found")
		return
	}

	var version *model.WikiPageVersion
	if page.CurrentVersionID != nil {
		version, _ = h.repo.GetVersion(r.Context(), *page.CurrentVersionID)
		applySafeHTML(version)
	}

	writeJSON(w, http.StatusOK, map[string]any{"page": page, "version": version})
}

func (h *WikiHandler) GetPageByPath(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	path := r.URL.Query().Get("path")

	page, err := h.repo.GetPageByPath(r.Context(), orgID, path)
	if err != nil {
		writeError(w, http.StatusNotFound, "page not found")
		return
	}

	var version *model.WikiPageVersion
	if page.CurrentVersionID != nil {
		version, _ = h.repo.GetVersion(r.Context(), *page.CurrentVersionID)
		applySafeHTML(version)
	}

	writeJSON(w, http.StatusOK, map[string]any{"page": page, "version": version})
}

func (h *WikiHandler) ListVersions(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
	if limit <= 0 {
		limit = 20
	}

	versions, total, err := h.repo.ListVersions(r.Context(), orgID, pageID, limit, offset)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list versions")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"versions": versions, "total": total})
}

func (h *WikiHandler) CreatePage(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var input model.CreatePageInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID

	if input.WorkspaceID == "" || input.Title == "" || input.Path == "" {
		writeError(w, http.StatusBadRequest, "workspace_id, title, path required")
		return
	}

	page, version, err := h.repo.CreatePage(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create page")
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"page": page, "version": version})
}

func (h *WikiHandler) UpdateVersion(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")

	var input model.UpdateVersionInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.PageID = pageID
	input.OrgID = orgID

	version, err := h.repo.CreateVersion(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create version")
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"version": version})
}

func (h *WikiHandler) SubmitProposal(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")

	var input model.SubmitProposalInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.PageID = pageID
	input.OrgID = orgID

	proposal, err := h.repo.SubmitProposal(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to submit proposal")
		return
	}
	writeJSON(w, http.StatusCreated, map[string]any{"proposal": proposal})
}

func (h *WikiHandler) ReviewProposal(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var input model.ReviewProposalInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID

	proposal, version, err := h.repo.ReviewProposal(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to review proposal")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"proposal": proposal, "new_version": version})
}

func (h *WikiHandler) CreateSourceLog(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")

	var input model.CreateSourceLogInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	input.PageID = pageID

	if input.SourceType == "" || input.SourceRef == "" {
		writeError(w, http.StatusBadRequest, "source_type and source_ref required")
		return
	}
	if input.SyncStatus == "" {
		input.SyncStatus = "synced"
	}

	sl, err := h.repo.CreateSourceLog(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create source log")
		return
	}
	writeJSON(w, http.StatusCreated, sl)
}

func (h *WikiHandler) ListSourceLogs(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
	if limit <= 0 {
		limit = 20
	}

	logs, total, err := h.repo.ListSourceLogs(r.Context(), orgID, pageID, limit, offset)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list source logs")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"logs": logs, "total": total})
}

func (h *WikiHandler) CreateMaintenanceLog(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")

	var input model.CreateMaintenanceLogInput
	if err := json.NewDecoder(r.Body).Decode(&input); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	input.OrgID = orgID
	input.PageID = pageID

	if input.Action == "" || input.Actor == "" {
		writeError(w, http.StatusBadRequest, "action and actor required")
		return
	}

	ml, err := h.repo.CreateMaintenanceLog(r.Context(), input)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to create maintenance log")
		return
	}
	writeJSON(w, http.StatusCreated, ml)
}

func (h *WikiHandler) ListMaintenanceLogs(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))
	if limit <= 0 {
		limit = 20
	}

	logs, total, err := h.repo.ListMaintenanceLogs(r.Context(), orgID, pageID, limit, offset)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list maintenance logs")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"logs": logs, "total": total})
}

func (h *WikiHandler) GetBacklinks(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	pageID := chi.URLParam(r, "pageID")

	pages, err := h.repo.GetBacklinks(r.Context(), orgID, pageID)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to get backlinks")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"pages": pages})
}

func Health(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok", "service": "wiki-store-go"})
}

// DiffVersions returns a line-based diff between two versions of a wiki page.
// Query params: from_version_id, to_version_id (both required).
func (h *WikiHandler) DiffVersions(w http.ResponseWriter, r *http.Request) {
	pageID := chi.URLParam(r, "pageID")
	from := r.URL.Query().Get("from_version_id")
	to := r.URL.Query().Get("to_version_id")
	if from == "" || to == "" {
		writeError(w, http.StatusBadRequest, "from_version_id and to_version_id required")
		return
	}

	fromV, err := h.repo.GetVersion(r.Context(), from)
	if err != nil || fromV == nil || fromV.PageID != pageID {
		writeError(w, http.StatusNotFound, "from version not found for page")
		return
	}
	toV, err := h.repo.GetVersion(r.Context(), to)
	if err != nil || toV == nil || toV.PageID != pageID {
		writeError(w, http.StatusNotFound, "to version not found for page")
		return
	}

	fromContent := ""
	if fromV.Content != nil {
		fromContent = *fromV.Content
	}
	toContent := ""
	if toV.Content != nil {
		toContent = *toV.Content
	}

	diff := lineDiff(fromContent, toContent)

	writeJSON(w, http.StatusOK, map[string]any{
		"page_id":         pageID,
		"from_version_id": from,
		"to_version_id":   to,
		"hunks":           diff.Hunks,
		"added_lines":     diff.Added,
		"removed_lines":   diff.Removed,
		"unchanged_lines": diff.Unchanged,
	})
}

type diffResult struct {
	Hunks     []diffHunk `json:"hunks"`
	Added     int        `json:"added"`
	Removed   int        `json:"removed"`
	Unchanged int        `json:"unchanged"`
}

type diffHunk struct {
	Op    string `json:"op"` // "add" | "remove" | "equal"
	Lines []string `json:"lines"`
}

// lineDiff produces a simple line-level diff using LCS dynamic programming.
// O(n*m) time/space — fine for wiki page content (typically <10k lines).
func lineDiff(a, b string) diffResult {
	aLines := splitLines(a)
	bLines := splitLines(b)
	n, m := len(aLines), len(bLines)

	// Build LCS table
	lcs := make([][]int, n+1)
	for i := range lcs {
		lcs[i] = make([]int, m+1)
	}
	for i := 1; i <= n; i++ {
		for j := 1; j <= m; j++ {
			if aLines[i-1] == bLines[j-1] {
				lcs[i][j] = lcs[i-1][j-1] + 1
			} else if lcs[i-1][j] >= lcs[i][j-1] {
				lcs[i][j] = lcs[i-1][j]
			} else {
				lcs[i][j] = lcs[i][j-1]
			}
		}
	}

	// Walk back to produce diff
	type op struct {
		kind string
		line string
	}
	var ops []op
	i, j := n, m
	for i > 0 && j > 0 {
		if aLines[i-1] == bLines[j-1] {
			ops = append(ops, op{"equal", aLines[i-1]})
			i--
			j--
		} else if lcs[i-1][j] >= lcs[i][j-1] {
			ops = append(ops, op{"remove", aLines[i-1]})
			i--
		} else {
			ops = append(ops, op{"add", bLines[j-1]})
			j--
		}
	}
	for ; i > 0; i-- {
		ops = append(ops, op{"remove", aLines[i-1]})
	}
	for ; j > 0; j-- {
		ops = append(ops, op{"add", bLines[j-1]})
	}
	// Reverse
	for x, y := 0, len(ops)-1; x < y; x, y = x+1, y-1 {
		ops[x], ops[y] = ops[y], ops[x]
	}

	// Group consecutive ops of same kind into hunks
	var result diffResult
	for _, o := range ops {
		switch o.kind {
		case "add":
			result.Added++
		case "remove":
			result.Removed++
		case "equal":
			result.Unchanged++
		}
		if len(result.Hunks) == 0 || result.Hunks[len(result.Hunks)-1].Op != o.kind {
			result.Hunks = append(result.Hunks, diffHunk{Op: o.kind, Lines: []string{o.line}})
		} else {
			h := &result.Hunks[len(result.Hunks)-1]
			h.Lines = append(h.Lines, o.line)
		}
	}
	return result
}

func splitLines(s string) []string {
	if s == "" {
		return nil
	}
	out := []string{}
	start := 0
	for i := 0; i < len(s); i++ {
		if s[i] == '\n' {
			out = append(out, s[start:i])
			start = i + 1
		}
	}
	if start < len(s) {
		out = append(out, s[start:])
	}
	return out
}

func Readyz(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ready", "service": "wiki-store-go"})
}

// ListPages — Wave 3.1 / Wave 11.C-b close.
// Query params:
//   - workspace_id (optional)
//   - status       (optional; e.g. "draft" | "published" | "deprecated")
//   - limit        (default 50, max 200)
//   - offset       (default 0)
func (h *WikiHandler) ListPages(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())
	workspaceID := r.URL.Query().Get("workspace_id")
	status := r.URL.Query().Get("status")
	limit, _ := strconv.Atoi(r.URL.Query().Get("limit"))
	offset, _ := strconv.Atoi(r.URL.Query().Get("offset"))

	pages, total, err := h.repo.ListPages(r.Context(), orgID, workspaceID, status, limit, offset)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "failed to list wiki pages")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"pages": pages,
		"total": total,
	})
}

// MaintenanceSweep ingests lint findings (from data-quality-go's /v1/quality/lint
// or any other detector) and writes one wiki_maintenance_logs row per item.
//
// Closes D4+D5 spec §3.4: `POST /v1/wiki/maintenance/sweep`.
//
// Body shape:
//
//	{
//	  "items": [
//	    {"page_id": "...", "kind": "stale_wiki" | "orphan_wiki" | "weak_citation" |
//	                              "contradiction" | "stale" | "orphan",
//	     "actor": "data-quality-lint", "details": {...}}
//	  ]
//	}
//
// Returns: `{"accepted": N, "rejected": N, "errors": [...]}`. Accepts batch
// writes; individual failures don't fail the request — matches the
// "honest partial success" pattern used by BulkIngest.
func (h *WikiHandler) MaintenanceSweep(w http.ResponseWriter, r *http.Request) {
	orgID := orgIDFrom(r.Context())

	var req struct {
		Items []struct {
			PageID  string          `json:"page_id"`
			Kind    string          `json:"kind"`
			Actor   string          `json:"actor"`
			Details json.RawMessage `json:"details"`
		} `json:"items"`
	}
	if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if len(req.Items) == 0 {
		writeError(w, http.StatusBadRequest, "items[] is required")
		return
	}
	const maxBatch = 1000
	if len(req.Items) > maxBatch {
		writeError(w, http.StatusBadRequest,
			fmt.Sprintf("batch exceeds max %d items", maxBatch))
		return
	}

	var accepted, rejected int
	var errs []string

	for i, it := range req.Items {
		if it.Kind == "" {
			rejected++
			errs = append(errs, fmt.Sprintf("item[%d]: kind required", i))
			continue
		}
		actor := it.Actor
		if actor == "" {
			actor = "wiki-maintenance-sweep"
		}
		// We map spec kinds onto the existing `action` column for backward
		// compat; the migration also adds a dedicated `kind` column.
		_, err := h.repo.CreateMaintenanceLog(r.Context(), model.CreateMaintenanceLogInput{
			OrgID:   orgID,
			PageID:  it.PageID,
			Action:  it.Kind,
			Actor:   actor,
			Details: it.Details,
		})
		if err != nil {
			rejected++
			errs = append(errs, fmt.Sprintf("item[%d]: %s", i, err.Error()))
			continue
		}
		accepted++
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"accepted": accepted,
		"rejected": rejected,
		"errors":   errs,
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
