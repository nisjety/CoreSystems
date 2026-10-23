package api

import (
	"bytes"
	"crypto/rand"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

const (
	complianceBase = "/v1/compliance/audit"

	complianceDefaultLimit = 200
	complianceMaxLimit     = 1000
	complianceMaxFilterLen = 128

	// A verify run is bounded so one request cannot hold a snapshot over an
	// unbounded chain; a capped run reports through_seq and through_hash to
	// resume from.
	complianceVerifyDefaultRows = 250_000
	complianceVerifyMaxRows     = 1_000_000
	complianceVerifyBodyMax     = 4 << 10

	selfAuditProducer = "audit-core"
	selfAuditPlane    = "control"
	selfAuditSource   = "internal:audit-core"

	eventComplianceRead   = "compliance_audit_read"
	eventComplianceVerify = "compliance_audit_verify"
)

// isComplianceRoute reports whether a request is one of the compliance read
// API's routes, which need their own scope rather than audit:read:self.
func isComplianceRoute(method, path string) bool {
	switch {
	case method == http.MethodGet:
		switch path {
		case complianceBase + "/events", complianceBase + "/manifest",
			complianceBase + "/keys", complianceBase + "/export-status":
			return true
		}
	case method == http.MethodPost:
		return path == complianceBase+"/verify"
	}
	return false
}

func (a *API) mountCompliance(r chi.Router) {
	r.Route("/compliance/audit", func(r chi.Router) {
		r.Get("/events", a.complianceEvents)
		r.Get("/manifest", a.complianceManifest)
		r.Post("/verify", a.complianceVerify)
		r.Get("/keys", a.complianceKeys)
		r.Get("/export-status", a.complianceExportStatus)
	})
}

func writeComplianceError(w http.ResponseWriter, status int, code, message string) {
	writeJSON(w, status, map[string]any{
		"data": nil,
		"meta": nil,
		"error": map[string]string{
			"code": code, "message": message,
		},
	})
}

// complianceQuery rejects what the route does not define: an unknown or repeated
// query parameter is a client bug, and silently ignoring it would return a
// wider range than the caller asked for.
func complianceQuery(w http.ResponseWriter, r *http.Request, allowed ...string) (url.Values, bool) {
	query := r.URL.Query()
	permitted := make(map[string]struct{}, len(allowed))
	for _, name := range allowed {
		permitted[name] = struct{}{}
	}
	for name, values := range query {
		if _, ok := permitted[name]; !ok || len(values) != 1 {
			writeComplianceError(w, http.StatusBadRequest, "invalid_query",
				"unsupported or repeated query parameter: "+name)
			return nil, false
		}
	}
	return query, true
}

func complianceTime(w http.ResponseWriter, query url.Values, name string) (time.Time, bool) {
	raw := query.Get(name)
	if raw == "" {
		return time.Time{}, true
	}
	t, err := time.Parse(time.RFC3339, raw)
	if err != nil {
		writeComplianceError(w, http.StatusBadRequest, "invalid_time", name+" must be an RFC 3339 timestamp")
		return time.Time{}, false
	}
	return t, true
}

func complianceWindow(w http.ResponseWriter, query url.Values) (since, until time.Time, ok bool) {
	if since, ok = complianceTime(w, query, "since"); !ok {
		return
	}
	if until, ok = complianceTime(w, query, "until"); !ok {
		return
	}
	if !since.IsZero() && !until.IsZero() && since.After(until) {
		writeComplianceError(w, http.StatusBadRequest, "invalid_time", "since must not be after until")
		return time.Time{}, time.Time{}, false
	}
	return since, until, true
}

type complianceCursor struct {
	Version int    `json:"v"`
	OrgID   string `json:"o"`
	Seq     int64  `json:"s"`
}

// encodeComplianceCursor is opaque to clients. It names the org so a cursor from
// one org's listing cannot be replayed against another; the keyset is the last
// chain_seq returned, which stays valid however many rows are appended.
func encodeComplianceCursor(orgID string, seq int64) string {
	raw, _ := json.Marshal(complianceCursor{Version: 1, OrgID: orgID, Seq: seq})
	return base64.RawURLEncoding.EncodeToString(raw)
}

func decodeComplianceCursor(orgID, raw string) (int64, bool) {
	if raw == "" {
		return 0, true
	}
	decoded, err := base64.RawURLEncoding.DecodeString(raw)
	if err != nil {
		return 0, false
	}
	var cursor complianceCursor
	if json.Unmarshal(decoded, &cursor) != nil || cursor.Version != 1 || cursor.OrgID != orgID || cursor.Seq < 1 {
		return 0, false
	}
	return cursor.Seq, true
}

func (a *API) complianceEvents(w http.ResponseWriter, r *http.Request) {
	authorization, ok := requireComplianceRole(w, r)
	if !ok {
		return
	}
	query, ok := complianceQuery(w, r, "org_id", "event", "user_id", "plane", "outcome", "since", "until", "limit", "cursor")
	if !ok {
		return
	}
	since, until, ok := complianceWindow(w, query)
	if !ok {
		return
	}
	limit := complianceDefaultLimit
	if raw := query.Get("limit"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || parsed < 1 {
			writeComplianceError(w, http.StatusBadRequest, "invalid_limit", "limit must be a positive integer")
			return
		}
		limit = min(parsed, complianceMaxLimit)
	}
	after, ok := decodeComplianceCursor(authorization.OrgID, query.Get("cursor"))
	if !ok {
		writeComplianceError(w, http.StatusBadRequest, "invalid_cursor", "cursor is not valid for this organization")
		return
	}
	filter := store.ComplianceFilter{
		OrgID: authorization.OrgID, Event: query.Get("event"), UserID: query.Get("user_id"),
		Plane: query.Get("plane"), Outcome: query.Get("outcome"),
		Since: since, Until: until, AfterSeq: after, Limit: limit,
	}
	for _, value := range []string{filter.Event, filter.UserID, filter.Plane, filter.Outcome} {
		if len(value) > complianceMaxFilterLen {
			writeComplianceError(w, http.StatusBadRequest, "invalid_query", "a filter value is too long")
			return
		}
	}

	page, err := a.store.ComplianceEvents(r.Context(), filter)
	if err != nil {
		log.Error().Err(err).Str("org_id", authorization.OrgID).Msg("complianceEvents failed")
		writeComplianceError(w, http.StatusInternalServerError, "query_failed", "query failed")
		return
	}
	if !a.recordComplianceRead(w, r, authorization, eventComplianceRead, map[string]any{
		"route":      "events",
		"filters":    complianceFilterDetails(query),
		"count":      len(page.Events),
		"cursor_seq": after,
	}) {
		return
	}

	var next any
	if page.NextSeq > 0 {
		next = encodeComplianceCursor(authorization.OrgID, page.NextSeq)
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": page.Events, "meta": map[string]any{
		"count": len(page.Events), "next_cursor": next, "limit": limit, "head_seq": page.HeadSeq,
	}, "error": nil})
}

// complianceFilterDetails is what the self-audit row records of the query: the
// filter names and values, which are identifiers and enumerations, never
// content.
func complianceFilterDetails(query url.Values) map[string]string {
	filters := map[string]string{}
	for _, name := range []string{"event", "user_id", "plane", "outcome", "since", "until"} {
		if value := query.Get(name); value != "" {
			filters[name] = value
		}
	}
	return filters
}

func (a *API) complianceManifest(w http.ResponseWriter, r *http.Request) {
	authorization, ok := requireComplianceRole(w, r)
	if !ok {
		return
	}
	query, ok := complianceQuery(w, r, "org_id", "since", "until")
	if !ok {
		return
	}
	since, until, ok := complianceWindow(w, query)
	if !ok {
		return
	}
	manifest, err := a.store.ComplianceManifest(r.Context(), authorization.OrgID, since, until)
	if err != nil {
		log.Error().Err(err).Str("org_id", authorization.OrgID).Msg("complianceManifest failed")
		writeComplianceError(w, http.StatusInternalServerError, "query_failed", "query failed")
		return
	}
	if !a.recordComplianceRead(w, r, authorization, eventComplianceRead, map[string]any{
		"route":   "manifest",
		"filters": complianceFilterDetails(query),
		"count":   manifest.Count,
	}) {
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": manifest, "error": nil})
}

type complianceVerifyRequest struct {
	FromSeq  int64         `json:"from_seq"`
	FromHash verifier.Hash `json:"from_hash"`
	MaxRows  int64         `json:"max_rows"`
}

func (a *API) complianceVerify(w http.ResponseWriter, r *http.Request) {
	authorization, ok := requireComplianceRole(w, r)
	if !ok {
		return
	}
	if _, ok := complianceQuery(w, r, "org_id"); !ok {
		return
	}
	var request complianceVerifyRequest
	body, err := io.ReadAll(io.LimitReader(r.Body, complianceVerifyBodyMax+1))
	if err != nil || len(body) > complianceVerifyBodyMax {
		writeComplianceError(w, http.StatusBadRequest, "invalid_body", "request body is unreadable or too large")
		return
	}
	if len(bytes.TrimSpace(body)) > 0 {
		decoder := json.NewDecoder(bytes.NewReader(body))
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&request); err != nil || decoder.More() {
			writeComplianceError(w, http.StatusBadRequest, "invalid_body", "body must be a JSON object of from_seq, from_hash and max_rows")
			return
		}
	}
	if request.FromSeq < 0 || request.MaxRows < 0 || request.MaxRows > complianceVerifyMaxRows {
		writeComplianceError(w, http.StatusBadRequest, "invalid_body", "from_seq and max_rows are out of range")
		return
	}
	if request.MaxRows == 0 {
		request.MaxRows = complianceVerifyDefaultRows
	}

	report, err := a.store.VerifyOrg(r.Context(), authorization.OrgID, verifier.Options{
		FromSeq: request.FromSeq, FromHash: request.FromHash, MaxRows: request.MaxRows,
	})
	if err != nil {
		log.Error().Err(err).Str("org_id", authorization.OrgID).Msg("complianceVerify failed")
		writeComplianceError(w, http.StatusInternalServerError, "verify_failed", "verification failed")
		return
	}
	if !a.recordComplianceRead(w, r, authorization, eventComplianceVerify, map[string]any{
		"route":            "verify",
		"status":           string(report.Status),
		"reason":           report.Reason,
		"rows_verified":    report.RowsVerified,
		"first_broken_seq": report.FirstBrokenSeq,
		"from_seq":         request.FromSeq,
		"max_rows":         request.MaxRows,
	}) {
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": report, "error": nil})
}

func (a *API) complianceKeys(w http.ResponseWriter, r *http.Request) {
	authorization, ok := requireComplianceRole(w, r)
	if !ok {
		return
	}
	if _, ok := complianceQuery(w, r, "org_id"); !ok {
		return
	}
	keys, err := a.store.ComplianceKeys(r.Context(), authorization.OrgID)
	if err != nil {
		log.Error().Err(err).Str("org_id", authorization.OrgID).Msg("complianceKeys failed")
		writeComplianceError(w, http.StatusInternalServerError, "query_failed", "query failed")
		return
	}
	if keys == nil {
		keys = []store.ComplianceKey{}
	}
	if !a.recordComplianceRead(w, r, authorization, eventComplianceRead, map[string]any{
		"route": "keys", "count": len(keys),
	}) {
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": keys, "meta": map[string]any{"count": len(keys)}, "error": nil})
}

// complianceExportStatus is empty until the SIEM exporter (S8) exists: there
// are no destinations to report on and nothing has been exported.
func (a *API) complianceExportStatus(w http.ResponseWriter, r *http.Request) {
	authorization, ok := requireComplianceRole(w, r)
	if !ok {
		return
	}
	if _, ok := complianceQuery(w, r, "org_id"); !ok {
		return
	}
	if !a.recordComplianceRead(w, r, authorization, eventComplianceRead, map[string]any{"route": "export_status"}) {
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]any{
		"exporter_enabled": false,
		"destinations":     []any{},
	}, "error": nil})
}

// recordComplianceRead writes the self-audit row for a compliance read through
// the normal insert path, so it joins the org's chain like any other event. The
// read fails closed: when the row cannot be written, no data is returned and
// the response is a 500, because an unaudited read of the audit log is exactly
// what this API must not allow. It reports whether the caller may respond.
func (a *API) recordComplianceRead(w http.ResponseWriter, r *http.Request, authorization serviceAuthorization, event string, details map[string]any) bool {
	suffix := make([]byte, 16)
	if _, err := rand.Read(suffix); err != nil {
		writeComplianceError(w, http.StatusInternalServerError, "audit_write_failed", "the read could not be audited")
		return false
	}
	_, err := a.store.InsertAuditFromSource(r.Context(), &events.AuditEvent{
		OccurredAt: time.Now().UTC(),
		OrgID:      authorization.OrgID,
		UserID:     authorization.UserID,
		ActorRole:  authorization.UserRole,
		Plane:      selfAuditPlane,
		Producer:   selfAuditProducer,
		Event:      event,
		Subject:    "audit_compliance_api",
		Outcome:    "ok",
		Details:    details,
		EventID:    fmt.Sprintf("%s:%s", event, hex.EncodeToString(suffix)),
	}, selfAuditSource)
	if err != nil {
		log.Error().Err(err).Str("org_id", authorization.OrgID).Str("event", event).Msg("compliance self-audit failed")
		writeComplianceError(w, http.StatusInternalServerError, "audit_write_failed", "the read could not be audited")
		return false
	}
	return true
}
