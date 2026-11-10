package api

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"strconv"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/services/capability-core/internal/procedures"
)

// The saved-procedure routes (plan 3.4 E4.2 step 3, decision B2(b)): capability-core's durable store of
// BrowserProcedure v2 bodies, kept as immutable numbered versions with movable tags. This file only moves
// HTTP to the store in internal/procedures; the validation, the digest and the owner scoping live there.
//
// Every route is the caller's own. The org and the person come from the verified token, never from a
// header or a body field, and a procedure that is not the caller's answers 404. While PROCEDURES_API is
// off, every route answers 404 procedures_disabled before anything is read or written. Zero Data Retention
// is refused at the gateway, not here: the store holds org configuration (actions and locators), never
// conversation content. The authz middleware already refuses every write from a ZDR principal.

const (
	proceduresAPIEnv = "PROCEDURES_API"
	// maxProcedureRequestBytes bounds a request body before it is read. It is the store's body cap, so the
	// envelope (name, description, source run id) and the body share it.
	maxProcedureRequestBytes = procedures.MaxBodyBytes
)

// ProceduresAPIFromLookup reads PROCEDURES_API strictly: only the literal "true" turns the saved-procedure
// routes on, so a typo or a stray "1" leaves every route answering 404 procedures_disabled.
func ProceduresAPIFromLookup(lookup func(string) string) bool {
	return lookup(proceduresAPIEnv) == "true"
}

// procedureDatabase is the pool slice the store and the audit rows use. The pgx pool satisfies it.
type procedureDatabase interface {
	Begin(ctx context.Context) (pgx.Tx, error)
	Exec(context.Context, string, ...any) (pgconn.CommandTag, error)
	Query(context.Context, string, ...any) (pgx.Rows, error)
	QueryRow(context.Context, string, ...any) pgx.Row
}

// ProceduresHandler serves the saved-procedure routes under /api/v1/procedures.
type ProceduresHandler struct {
	store   *procedures.Store
	db      procedureDatabase
	enabled bool
}

// NewProceduresHandler constructs the handler over the capability-core pool. enabled comes from
// ProceduresAPIFromLookup.
func NewProceduresHandler(db procedureDatabase, enabled bool) *ProceduresHandler {
	store, err := procedures.NewStore(db)
	if err != nil {
		slog.Error("saved procedure store not wired", "error", err)
	}
	return &ProceduresHandler{store: store, db: db, enabled: enabled}
}

// Register mounts the saved-procedure routes. Method-qualified patterns let the mux answer 405 for a method
// a path does not take; every handler answers the switch before anything else.
func (h *ProceduresHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc("POST /api/v1/procedures", h.create)
	mux.HandleFunc("GET /api/v1/procedures", h.list)
	mux.HandleFunc("GET /api/v1/procedures/{id}", h.get)
	mux.HandleFunc("DELETE /api/v1/procedures/{id}", h.delete)
	mux.HandleFunc("POST /api/v1/procedures/{id}/versions", h.addVersion)
	mux.HandleFunc("GET /api/v1/procedures/{id}/versions/{version}", h.getVersion)
	mux.HandleFunc("PUT /api/v1/procedures/{id}/tags/{tag}", h.setTag)
	mux.HandleFunc("DELETE /api/v1/procedures/{id}/tags/{tag}", h.deleteTag)
}

// procedureCreateBody is the body of POST /api/v1/procedures. procedure is a BrowserProcedure v2 body, kept as
// raw JSON until the store validates it.
type procedureCreateBody struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	SourceRunID string          `json:"source_run_id"`
	Procedure   json.RawMessage `json:"procedure"`
}

// procedureVersionBody is the body of POST /api/v1/procedures/{id}/versions.
type procedureVersionBody struct {
	Procedure json.RawMessage `json:"procedure"`
}

// procedureTagBody is the body of PUT /api/v1/procedures/{id}/tags/{tag}: the version the tag points at.
type procedureTagBody struct {
	Version int `json:"version"`
}

// procedureWritten answers a created procedure or an added version: its id, the version written and the
// digest of the stored body.
type procedureWritten struct {
	ProcedureID string `json:"procedure_id"`
	Version     int    `json:"version"`
	Digest      string `json:"digest"`
}

// procedureTagSet answers a tag write: the tag and the version it now points at.
type procedureTagSet struct {
	ProcedureID string `json:"procedure_id"`
	Tag         string `json:"tag"`
	Version     int    `json:"version"`
}

// procedureListItem is one procedure in a listing: its metadata and its newest version number. It never
// carries a body.
type procedureListItem struct {
	ID            string    `json:"id"`
	Name          string    `json:"name"`
	Description   string    `json:"description"`
	SourceRunID   string    `json:"source_run_id"`
	LatestVersion int       `json:"latest_version"`
	CreatedAt     time.Time `json:"created_at"`
	UpdatedAt     time.Time `json:"updated_at"`
}

// procedureListView is one page of GET /api/v1/procedures. next_after_id is set only while older procedures
// remain; send it back as after_id to read the next page.
type procedureListView struct {
	Procedures  []procedureListItem `json:"procedures"`
	NextAfterID string              `json:"next_after_id,omitempty"`
}

// procedureVersionSummary describes one version without its body.
type procedureVersionSummary struct {
	Version   int       `json:"version"`
	Digest    string    `json:"digest"`
	StepCount int       `json:"step_count"`
	SlotNames []string  `json:"slot_names"`
	CreatedAt time.Time `json:"created_at"`
}

// procedureTagView is one tag and the version it points at.
type procedureTagView struct {
	Tag     string `json:"tag"`
	Version int    `json:"version"`
}

// procedureDetailView is GET /api/v1/procedures/{id}: the procedure's metadata, the summary of its newest
// version and its tags. It carries no body.
type procedureDetailView struct {
	ID            string                  `json:"id"`
	Name          string                  `json:"name"`
	Description   string                  `json:"description"`
	SourceRunID   string                  `json:"source_run_id"`
	LatestVersion int                     `json:"latest_version"`
	CreatedAt     time.Time               `json:"created_at"`
	UpdatedAt     time.Time               `json:"updated_at"`
	Latest        procedureVersionSummary `json:"latest"`
	Tags          []procedureTagView      `json:"tags"`
}

// procedureVersionView is GET /api/v1/procedures/{id}/versions/{version}: one version and its full body.
// procedure is the canonical BrowserProcedure v2 JSON that the version's digest was computed over.
type procedureVersionView struct {
	ProcedureID string          `json:"procedure_id"`
	Version     int             `json:"version"`
	Digest      string          `json:"digest"`
	StepCount   int             `json:"step_count"`
	SlotNames   []string        `json:"slot_names"`
	CreatedAt   time.Time       `json:"created_at"`
	Procedure   json.RawMessage `json:"procedure"`
}

// procedureOwner reads the caller from the verified principal. The org and the person are the signed token's,
// never a request header or a body field. A principal with no person id, such as a workload identity, owns no
// procedures and is refused.
func procedureOwner(w http.ResponseWriter, r *http.Request) (procedures.Owner, bool) {
	orgID := verifiedOrganizationID(r)
	if orgID == "" {
		writeTypedError(w, http.StatusUnauthorized, "authentication_required", "Sign in to use saved procedures.")
		return procedures.Owner{}, false
	}
	actorID := verifiedActorID(r)
	if actorID == "" {
		writeTypedError(w, http.StatusForbidden, "forbidden", "Saved procedures belong to a person, and this caller has no person id.")
		return procedures.Owner{}, false
	}
	return procedures.Owner{OrgID: orgID, UserID: actorID}, true
}

// gate answers the refusals every route shares: 404 while the switch is off, and 503 when no store was wired.
// It reports whether the route may go on.
func (h *ProceduresHandler) gate(w http.ResponseWriter) bool {
	if !h.enabled {
		writeTypedError(w, http.StatusNotFound, "procedures_disabled", "Saved procedures are not enabled on this deployment.")
		return false
	}
	if h.store == nil {
		writeTypedError(w, http.StatusServiceUnavailable, "database_unavailable", "Saved procedures are unavailable.")
		return false
	}
	return true
}

// decodeProcedureBody reads one JSON object of at most maxProcedureRequestBytes into dst. Unknown fields are
// refused, so a misspelt field, or an org or user id sent in a body, is never dropped without a word.
func decodeProcedureBody(w http.ResponseWriter, r *http.Request, dst any) bool {
	r.Body = http.MaxBytesReader(w, r.Body, maxProcedureRequestBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(dst); err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeTypedError(w, http.StatusRequestEntityTooLarge, string(procedures.CodeBodyTooLarge), "The request is larger than a saved procedure may be.")
			return false
		}
		writeTypedError(w, http.StatusBadRequest, "invalid_request", "The body must be one JSON object with the documented fields.")
		return false
	}
	if err := ensureJSONEOF(decoder); err != nil {
		writeTypedError(w, http.StatusBadRequest, "invalid_request", "The body must be one JSON object with the documented fields.")
		return false
	}
	return true
}

// writeProceduresStoreError answers a store refusal. A validation refusal carries its closed code, and its
// message never repeats a value from the body. Anything else is a database failure, logged without content.
func writeProceduresStoreError(w http.ResponseWriter, err error) {
	if refusal, ok := procedures.AsValidation(err); ok {
		writeTypedError(w, http.StatusBadRequest, string(refusal.Code), refusal.Message)
		return
	}
	switch {
	case errors.Is(err, procedures.ErrNotFound):
		writeTypedError(w, http.StatusNotFound, "procedure_not_found", "The saved procedure was not found.")
	case errors.Is(err, procedures.ErrVersionLimit):
		writeTypedError(w, http.StatusConflict, "procedure_version_limit", "The saved procedure has reached its version limit.")
	case errors.Is(err, procedures.ErrTagLimit):
		writeTypedError(w, http.StatusConflict, "procedure_tag_limit", "The saved procedure has reached its tag limit.")
	default:
		slog.Error("saved procedure store failed", "error", err)
		writeTypedError(w, http.StatusInternalServerError, "database_unavailable", "The saved procedure could not be served.")
	}
}

// writeProceduresJSON answers with a JSON body and the given status.
func writeProceduresJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

// appendProcedureAudit writes the registry_audit_log row for one saved-procedure call. The diff holds ids and
// counts only: never a name, a description, a body, a locator or a typed value.
func (h *ProceduresHandler) appendProcedureAudit(ctx context.Context, action, procedureID string, who procedures.Owner, diff map[string]any) error {
	raw, err := json.Marshal(diff)
	if err != nil {
		return err
	}
	_, err = h.db.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1, 'procedure', $2, $3, $4, $5, $6::jsonb)
	`, "ral_"+uuid.NewString(), procedureID, action, who.UserID, who.OrgID, string(raw))
	return err
}

// auditProcedureRead records a read before it is answered. A read changes nothing, so a read that nobody
// recorded is refused: a failed audit answers 500 and no body.
func (h *ProceduresHandler) auditProcedureRead(w http.ResponseWriter, r *http.Request, action, procedureID string, who procedures.Owner, diff map[string]any) bool {
	if err := h.appendProcedureAudit(r.Context(), action, procedureID, who, diff); err != nil {
		slog.Error("saved procedure read audit failed", "action", action, "error", err)
		writeTypedError(w, http.StatusInternalServerError, "database_unavailable", "The saved procedure could not be served.")
		return false
	}
	return true
}

// auditProcedureWrite records a write after it has committed. The write is durable already, so a failed audit
// is logged with the ids and the answer does not change: a refused answer would invite a retry that writes again.
func (h *ProceduresHandler) auditProcedureWrite(r *http.Request, action, procedureID string, who procedures.Owner, diff map[string]any) {
	if err := h.appendProcedureAudit(context.WithoutCancel(r.Context()), action, procedureID, who, diff); err != nil {
		slog.Error("saved procedure write audit failed", "action", action, "procedure_id", procedureID, "error", err)
	}
}

// procedureVersionDiff is the audit diff of a written version: its number, its digest and its counts.
func procedureVersionDiff(v *procedures.Version) map[string]any {
	return map[string]any{"version": v.Version, "digest": v.Digest, "step_count": v.StepCount, "slot_count": len(v.SlotNames)}
}

// procedureListItemOf maps a stored procedure to its listing fields.
func procedureListItemOf(p procedures.Procedure) procedureListItem {
	return procedureListItem{ID: p.ID, Name: p.Name, Description: p.Description, SourceRunID: p.SourceRunID, LatestVersion: p.LatestVersion, CreatedAt: p.CreatedAt, UpdatedAt: p.UpdatedAt}
}

// procedureVersionSummaryOf describes a version without its body.
func procedureVersionSummaryOf(v *procedures.Version) procedureVersionSummary {
	return procedureVersionSummary{Version: v.Version, Digest: v.Digest, StepCount: v.StepCount, SlotNames: nonNilStrings(v.SlotNames), CreatedAt: v.CreatedAt}
}

// procedureDetailOf builds GET /api/v1/procedures/{id}: the metadata, the newest version's summary and the tags.
func procedureDetailOf(p *procedures.Procedure, latest *procedures.Version) procedureDetailView {
	tags := make([]procedureTagView, 0, len(p.Tags))
	for _, t := range p.Tags {
		tags = append(tags, procedureTagView{Tag: t.Tag, Version: t.Version})
	}
	return procedureDetailView{ID: p.ID, Name: p.Name, Description: p.Description, SourceRunID: p.SourceRunID, LatestVersion: latest.Version, CreatedAt: p.CreatedAt, UpdatedAt: p.UpdatedAt, Latest: procedureVersionSummaryOf(latest), Tags: tags}
}

// procedureVersionViewOf builds GET /api/v1/procedures/{id}/versions/{version} from a version and its canonical body.
func procedureVersionViewOf(v *procedures.Version, body json.RawMessage) procedureVersionView {
	return procedureVersionView{ProcedureID: v.ProcedureID, Version: v.Version, Digest: v.Digest, StepCount: v.StepCount, SlotNames: nonNilStrings(v.SlotNames), CreatedAt: v.CreatedAt, Procedure: body}
}

// create is POST /api/v1/procedures: a new procedure with its version 1. Answers 201 with the id, the version
// and the digest of the stored body.
func (h *ProceduresHandler) create(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	var body procedureCreateBody
	if !decodeProcedureBody(w, r, &body) {
		return
	}
	meta := procedures.Meta{Name: body.Name, Description: body.Description, SourceRunID: body.SourceRunID}
	created, version, err := h.store.Create(r.Context(), who, meta, body.Procedure)
	if err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	h.auditProcedureWrite(r, "created", created.ID, who, procedureVersionDiff(version))
	writeProceduresJSON(w, http.StatusCreated, procedureWritten{ProcedureID: created.ID, Version: version.Version, Digest: version.Digest})
}

// list is GET /api/v1/procedures: the caller's procedures, newest first, one page at a time. Query: limit
// (default 20, at most 100) and after_id (the next_after_id of the previous page).
func (h *ProceduresHandler) list(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	limit := 0
	if raw := r.URL.Query().Get("limit"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 {
			writeTypedError(w, http.StatusBadRequest, "invalid_limit", "limit must be a positive whole number.")
			return
		}
		limit = min(parsed, procedures.MaxListLimit)
	}
	afterID := r.URL.Query().Get("after_id")
	if afterID != "" && !procedures.ValidID(afterID) {
		writeTypedError(w, http.StatusBadRequest, "invalid_after_id", "after_id is not a procedure id.")
		return
	}
	page, err := h.store.List(r.Context(), who, limit, afterID)
	if err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	items := make([]procedureListItem, 0, len(page.Items))
	for _, p := range page.Items {
		items = append(items, procedureListItemOf(p))
	}
	if !h.auditProcedureRead(w, r, "listed", "", who, map[string]any{"returned": len(items)}) {
		return
	}
	writeProceduresJSON(w, http.StatusOK, procedureListView{Procedures: items, NextAfterID: page.NextAfterID})
}

// get is GET /api/v1/procedures/{id}: the metadata, the newest version's summary and the tags. No body.
func (h *ProceduresHandler) get(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	id := r.PathValue("id")
	p, err := h.store.Get(r.Context(), who, id)
	if err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	latest, err := h.store.GetVersion(r.Context(), who, id, procedures.Ref{})
	if err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	if !h.auditProcedureRead(w, r, "read", id, who, map[string]any{"version": latest.Version, "tags": len(p.Tags)}) {
		return
	}
	writeProceduresJSON(w, http.StatusOK, procedureDetailOf(p, latest))
}

// getVersion is GET /api/v1/procedures/{id}/versions/{version}: one version with its full body. The body is
// checked against the digest the version recorded before it is answered, so a stored body that no longer
// matches its digest is refused, not served.
func (h *ProceduresHandler) getVersion(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	id := r.PathValue("id")
	number, err := strconv.Atoi(r.PathValue("version"))
	if err != nil || number < 1 {
		writeTypedError(w, http.StatusBadRequest, "invalid_version", "A version is a whole number of 1 or more.")
		return
	}
	v, err := h.store.GetVersion(r.Context(), who, id, procedures.Ref{Version: number})
	if err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	canonical, digest, err := procedures.Canonicalize(v.Body)
	if err != nil || digest != v.Digest {
		slog.Error("saved procedure version failed its digest check", "procedure_id", id, "version", number)
		writeTypedError(w, http.StatusInternalServerError, "procedure_digest_mismatch", "The saved procedure failed its integrity check.")
		return
	}
	if !h.auditProcedureRead(w, r, "version_read", id, who, map[string]any{"version": number, "step_count": v.StepCount}) {
		return
	}
	writeProceduresJSON(w, http.StatusOK, procedureVersionViewOf(v, canonical))
}

// addVersion is POST /api/v1/procedures/{id}/versions: the next immutable version of a procedure the caller
// owns. Answers 201 with the version and its digest.
func (h *ProceduresHandler) addVersion(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	var body procedureVersionBody
	if !decodeProcedureBody(w, r, &body) {
		return
	}
	id := r.PathValue("id")
	v, err := h.store.AddVersion(r.Context(), who, id, body.Procedure)
	if err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	h.auditProcedureWrite(r, "version_added", id, who, procedureVersionDiff(v))
	writeProceduresJSON(w, http.StatusCreated, procedureWritten{ProcedureID: id, Version: v.Version, Digest: v.Digest})
}

// setTag is PUT /api/v1/procedures/{id}/tags/{tag}: points the tag at a version of a procedure the caller owns,
// creating the tag or moving it. Answers 200 with the tag and its version.
func (h *ProceduresHandler) setTag(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	var body procedureTagBody
	if !decodeProcedureBody(w, r, &body) {
		return
	}
	if body.Version < 1 {
		writeTypedError(w, http.StatusBadRequest, "invalid_version", "A version is a whole number of 1 or more.")
		return
	}
	id, tag := r.PathValue("id"), r.PathValue("tag")
	if err := h.store.SetTag(r.Context(), who, id, tag, body.Version); err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	h.auditProcedureWrite(r, "tag_set", id, who, map[string]any{"tag": tag, "version": body.Version})
	writeProceduresJSON(w, http.StatusOK, procedureTagSet{ProcedureID: id, Tag: tag, Version: body.Version})
}

// deleteTag is DELETE /api/v1/procedures/{id}/tags/{tag}: removes the tag. The version it pointed at stays.
// Answers 204.
func (h *ProceduresHandler) deleteTag(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	id, tag := r.PathValue("id"), r.PathValue("tag")
	if err := h.store.DeleteTag(r.Context(), who, id, tag); err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	h.auditProcedureWrite(r, "tag_deleted", id, who, map[string]any{"tag": tag})
	w.WriteHeader(http.StatusNoContent)
}

// delete is DELETE /api/v1/procedures/{id}: the procedure with all of its versions and tags. Answers 204.
func (h *ProceduresHandler) delete(w http.ResponseWriter, r *http.Request) {
	if !h.gate(w) {
		return
	}
	who, ok := procedureOwner(w, r)
	if !ok {
		return
	}
	id := r.PathValue("id")
	if err := h.store.Delete(r.Context(), who, id); err != nil {
		writeProceduresStoreError(w, err)
		return
	}
	h.auditProcedureWrite(r, "deleted", id, who, map[string]any{})
	w.WriteHeader(http.StatusNoContent)
}
