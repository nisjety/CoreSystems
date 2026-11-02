package api

import (
	"fmt"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// Role-gated audit reads (WS05-S1).
//
// Before this gate every organization member could read the whole org audit
// trail and usage ledger. The signed v3 delegation already carries the live
// org role the gateway verified, so audit-core decides the read scope itself:
//
//	owner, admin          whole org, any user_id          scope "org"
//	anyone else (or an    own rows only                   scope "self"
//	unknown role)         tool_action with no user_id     scope "org_redacted"
//
// A foreign user_id from a non-admin is a typed 403 (audit_scope_denied).
// Usage routes are admin only.

const (
	auditReadEnforcementEnv = "AUDIT_READ_ROLE_ENFORCEMENT"

	scopeOrg         = "org"
	scopeSelf        = "self"
	scopeOrgRedacted = "org_redacted"

	// auditReadModeEnforce is the default and the only mode production accepts.
	auditReadModeEnforce = "enforce"
	// auditReadModeReport logs the would-deny decision and serves as before.
	auditReadModeReport = "report"
	// auditReadModeOff skips the gate entirely.
	auditReadModeOff = "off"

	toolActionEvent = "tool_action"
)

// IsOrgAdmin reports whether the signed delegation names an org owner or admin.
// Any other value, including an empty or unknown role, is not an admin.
func (a serviceAuthorization) IsOrgAdmin() bool {
	return isComplianceRole(a.UserRole)
}

// auditReadMode resolves AUDIT_READ_ROLE_ENFORCEMENT. Unset means enforce.
// A non-enforce mode is refused when ENVIRONMENT=production.
func auditReadMode(raw, environment string) (string, error) {
	mode := strings.ToLower(strings.TrimSpace(raw))
	switch mode {
	case "", auditReadModeEnforce:
		return auditReadModeEnforce, nil
	case auditReadModeReport, auditReadModeOff:
		if strings.EqualFold(strings.TrimSpace(environment), "production") {
			return "", fmt.Errorf("%s=%s is refused when ENVIRONMENT=production", auditReadEnforcementEnv, mode)
		}
		log.Warn().Str("mode", mode).Msg("audit read role enforcement is not enforcing; any org member may read the org audit trail")
		return mode, nil
	default:
		return "", fmt.Errorf("%s must be enforce, report or off", auditReadEnforcementEnv)
	}
}

func auditReadModeFromEnv() (string, error) {
	return auditReadMode(os.Getenv(auditReadEnforcementEnv), os.Getenv("ENVIRONMENT"))
}

// writeScopeDenied writes the typed audit_scope_denied envelope.
func writeScopeDenied(w http.ResponseWriter, message string) {
	writeJSON(w, http.StatusForbidden, map[string]any{"error": map[string]any{
		"code":    "audit_scope_denied",
		"message": message,
		"details": map[string]string{"scope": scopeSelf},
	}})
}

// requireOrgAdminRead gates the usage routes. It returns true when the caller
// may continue. In report mode a would-deny is logged and the call continues.
func (a *API) requireOrgAdminRead(w http.ResponseWriter, r *http.Request, route string) bool {
	authorization, _ := serviceAuthorizationFromContext(r.Context())
	if a.readMode == auditReadModeOff || authorization.IsOrgAdmin() {
		return true
	}
	if a.readMode == auditReadModeReport {
		logWouldDeny(route, authorization, "")
		return true
	}
	writeScopeDenied(w, "usage reads are limited to organization owners and admins")
	return false
}

func logWouldDeny(route string, authorization serviceAuthorization, requestedUserID string) {
	log.Warn().
		Str("route", route).
		Str("org_id", authorization.OrgID).
		Str("user_id", authorization.UserID).
		Str("role", strings.ToLower(strings.TrimSpace(authorization.UserRole))).
		Str("requested_user_id", requestedUserID).
		Msg("audit read role enforcement (report mode): non-admin read would be narrowed or denied")
}

// resolveAuditScope applies the role rules to a listAudit query. It returns the
// scope to serve, the narrowed filter, and ok=false when it already wrote a 403.
func (a *API) resolveAuditScope(w http.ResponseWriter, r *http.Request, filter store.AuditFilter) (string, store.AuditFilter, bool) {
	authorization, _ := serviceAuthorizationFromContext(r.Context())
	if a.readMode == auditReadModeOff || authorization.IsOrgAdmin() {
		return scopeOrg, filter, true
	}
	if a.readMode == auditReadModeReport {
		logWouldDeny("/v1/audit", authorization, filter.UserID)
		return scopeOrg, filter, true
	}

	caller := strings.TrimSpace(authorization.UserID)
	if caller == "" {
		writeScopeDenied(w, "audit reads need an authenticated user")
		return "", filter, false
	}
	if filter.UserID != "" && filter.UserID != caller {
		writeScopeDenied(w, "you can only read your own audit events")
		return "", filter, false
	}
	if filter.UserID == "" && filter.Event == toolActionEvent {
		// Trust Center: a content-free org-wide tool_action view. Subject and
		// resource filters would let a reader probe columns the projection hides.
		filter.Subject, filter.ResourceID = "", ""
		return scopeOrgRedacted, filter, true
	}
	filter.UserID = caller
	return scopeSelf, filter, true
}

// redactedAuditRow is the only shape a non-admin ever sees org-wide.
type redactedAuditRow struct {
	ID         int64          `json:"id"`
	OccurredAt time.Time      `json:"occurred_at"`
	IngestedAt time.Time      `json:"ingested_at"`
	Plane      string         `json:"plane"`
	Event      string         `json:"event"`
	Outcome    string         `json:"outcome"`
	Details    map[string]any `json:"details"`
}

// redactedToolActionDetails is the whitelist of tool_action details keys.
var redactedToolActionDetails = []string{"tool", "data_category", "zdr", "phase"}

// redactToolActionRows projects tool_action rows to the content-free view. Rows
// of any other event type are dropped: no other event is ever exposed org-wide.
func redactToolActionRows(rows []store.AuditRow) []redactedAuditRow {
	out := make([]redactedAuditRow, 0, len(rows))
	for _, row := range rows {
		if row.Event != toolActionEvent {
			continue
		}
		details := make(map[string]any, len(redactedToolActionDetails))
		for _, key := range redactedToolActionDetails {
			// Scalars only: a whitelisted key never carries a nested value out.
			switch value := row.Details[key].(type) {
			case string, bool:
				details[key] = value
			}
		}
		out = append(out, redactedAuditRow{
			ID:         row.ID,
			OccurredAt: row.OccurredAt,
			IngestedAt: row.IngestedAt,
			Plane:      row.Plane,
			Event:      row.Event,
			Outcome:    row.Outcome,
			Details:    details,
		})
	}
	return out
}
