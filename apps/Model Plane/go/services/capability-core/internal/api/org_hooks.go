// Package api — durable org hook rules (build plan WS25-S8, DEC-14).
//
// capability-core is the system of record for an org's hook rules: the deny and ask rules the gateway
// forwards to execution-core on every Do run once HOOKS_DURABLE is on. An org admin writes them and
// every member of the org reads them. A rule narrows a run and never widens one, so nothing here stores
// an allow. A post_tool rule may only deny, because a post-tool ask would reach a person after the tool
// had already acted (WS25-D7).
//
// Every write also bumps the org's revision in org_agent_policy, in the same statement as the rule
// change, so the gateway caches the rules and the policy document under one revision. A rule is never
// deleted: revoking sets revoked_at and the row stays as history.
//
// Who may write: an org admin (the signed scopes, verifiedIsAdmin), acting as a person, with the write
// scope and an explicit non-ZDR posture. A ZDR principal reads but never writes, because a rule is
// durable state. This package has no switch of its own: the gateway's HOOKS_DURABLE decides whether a
// replica reads the rules at all.
package api

import (
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

const (
	orgHooksPath = "/api/v1/org-hooks"

	maxOrgHookBodyBytes   = 8 << 10
	maxOrgHookRules       = 200
	maxOrgHookReasonRunes = 500
)

// orgHookRefusal is one closed refusal code and the status it answers with.
type orgHookRefusal struct {
	code   string
	status int
}

var (
	orgHookRefuseInvalidEvent    = orgHookRefusal{"invalid_event", http.StatusBadRequest}
	orgHookRefuseInvalidDecision = orgHookRefusal{"invalid_decision", http.StatusBadRequest}
	orgHookRefuseInvalidMatcher  = orgHookRefusal{"invalid_matcher", http.StatusBadRequest}
	orgHookRefuseInvalidReason   = orgHookRefusal{"invalid_reason", http.StatusBadRequest}
	orgHookRefusePostToolAsk     = orgHookRefusal{"post_tool_must_deny", http.StatusUnprocessableEntity}
	orgHookRefuseDuplicate       = orgHookRefusal{"duplicate_rule", http.StatusConflict}
	orgHookRefuseLimit           = orgHookRefusal{"rule_limit_reached", http.StatusUnprocessableEntity}
	orgHookRefuseNotFound        = orgHookRefusal{"rule_not_found", http.StatusNotFound}
)

// normaliseOrgHookMatcher returns the stored form of a tool matcher, under execution-core's grammar
// (hook/mod.rs tool_matches): "*" for any tool, which an empty matcher also means; a prefix ending in
// one "*"; or an exact tool name. Anything else is refused, because the engine would match it as an
// exact name and never fire on the calls its author meant.
func normaliseOrgHookMatcher(raw string) (string, bool) {
	matcher := strings.TrimSpace(raw)
	if matcher == "" || matcher == "*" {
		return "*", true
	}
	if !mcpIdentifier.MatchString(strings.TrimSuffix(matcher, "*")) {
		return "", false
	}
	return matcher, true
}

// orgHookRule is one rule as the handler stores it, after validation.
type orgHookRule struct {
	event    string
	matcher  string
	decision string
	reason   string
	priority int32
	enabled  bool
}

// orgHookRuleBody is a create or an update request. An omitted enabled means true, so a rule written
// without the field applies.
type orgHookRuleBody struct {
	Event       string `json:"event"`
	ToolMatcher string `json:"tool_matcher"`
	Decision    string `json:"decision"`
	Reason      string `json:"reason"`
	Priority    int32  `json:"priority"`
	Enabled     *bool  `json:"enabled"`
}

// validateOrgHookRule checks a written rule and returns it in the form the store keeps. The decision is
// case-folded, the matcher and the reason are trimmed, and a post_tool rule must deny: a post-tool ask
// would reach a person after the tool had already acted, whereas a denial there withholds the result.
func validateOrgHookRule(body orgHookRuleBody) (orgHookRule, *orgHookRefusal) {
	if body.Event != "pre_tool" && body.Event != "post_tool" {
		return orgHookRule{}, &orgHookRefuseInvalidEvent
	}
	decision := strings.ToLower(strings.TrimSpace(body.Decision))
	if decision != "deny" && decision != "ask" {
		return orgHookRule{}, &orgHookRefuseInvalidDecision
	}
	if body.Event == "post_tool" && decision != "deny" {
		return orgHookRule{}, &orgHookRefusePostToolAsk
	}
	matcher, ok := normaliseOrgHookMatcher(body.ToolMatcher)
	if !ok {
		return orgHookRule{}, &orgHookRefuseInvalidMatcher
	}
	reason := strings.TrimSpace(body.Reason)
	if reason == "" || utf8.RuneCountInString(reason) > maxOrgHookReasonRunes {
		return orgHookRule{}, &orgHookRefuseInvalidReason
	}
	enabled := true
	if body.Enabled != nil {
		enabled = *body.Enabled
	}
	return orgHookRule{
		event:    body.Event,
		matcher:  matcher,
		decision: decision,
		reason:   reason,
		priority: body.Priority,
		enabled:  enabled,
	}, nil
}

// orgHookRuleView is a rule as the API returns it.
type orgHookRuleView struct {
	ID          string    `json:"id"`
	Event       string    `json:"event"`
	ToolMatcher string    `json:"tool_matcher"`
	Decision    string    `json:"decision"`
	Reason      string    `json:"reason"`
	Priority    int32     `json:"priority"`
	Enabled     bool      `json:"enabled"`
	CreatedBy   string    `json:"created_by"`
	UpdatedBy   string    `json:"updated_by"`
	CreatedAt   time.Time `json:"created_at"`
	UpdatedAt   time.Time `json:"updated_at"`
}

// orgHookWriteView is the answer to a create or an update: the rule and the policy revision the write
// produced.
type orgHookWriteView struct {
	orgHookRuleView
	Revision int64 `json:"revision"`
}

// orgHookListSQL reads an org's active rules, highest priority first, then by id, the order the gateway
// serialises them in.
const orgHookListSQL = `
SELECT id, event, tool_matcher, decision, reason, priority, enabled, created_by, updated_by, created_at, updated_at
FROM org_hook_rules
WHERE org_id = $1 AND revoked_at IS NULL
ORDER BY priority DESC, id ASC
`

const orgHookRevisionSQL = `SELECT revision FROM org_agent_policy WHERE org_id = $1`

// orgHookBumpSQL is the tail every rule write shares. It follows `changed`, the CTE holding the rule row
// the write changed, so a write that changed no row bumps nothing. The bump keeps the document and digest
// already in force; only an org with no policy row takes the defaults, at revision 1, which is what the
// policy read answers for an org with no row. Parameters: $1 org, $2 actor, $3 rule id, $4 history id,
// $5 audit id, $6 audit diff, $7 default document, $8 default digest.
const orgHookBumpSQL = `,
bump AS (
    INSERT INTO org_agent_policy (org_id, revision, doc, digest, updated_by)
    SELECT $1::text, 1, $7::jsonb, $8::text, $2::text FROM changed
    ON CONFLICT (org_id) DO UPDATE
        SET revision = org_agent_policy.revision + 1, updated_by = EXCLUDED.updated_by, updated_at = now()
    RETURNING revision, doc, digest
), hist AS (
    INSERT INTO org_agent_policy_history (id, org_id, revision, doc, digest, actor)
    SELECT $4::text, $1::text, bump.revision, bump.doc, bump.digest, $2::text FROM bump
    RETURNING revision
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $5::text, 'org_hook_rules', $3::text, 'org_hook_changed', $2::text, $1::text,
           $6::jsonb || jsonb_build_object('revision', bump.revision)
    FROM bump
    RETURNING 1
)
SELECT changed.id, changed.created_by, changed.created_at, changed.updated_at, bump.revision
FROM changed, bump
`

// orgHookCreateSQL inserts a rule while the org holds fewer than maxOrgHookRules active ones, then bumps
// the revision. Parameters: $1 org, $2 actor, $3 rule id, $9 event, $10 matcher, $11 decision, $12 reason,
// $13 priority, $14 enabled, $15 the rule limit. An org at its limit returns no row.
const orgHookCreateSQL = `
WITH room AS (
    SELECT 1 WHERE (SELECT count(*) FROM org_hook_rules WHERE org_id = $1::text AND revoked_at IS NULL) < $15::bigint
), changed AS (
    INSERT INTO org_hook_rules (id, org_id, event, tool_matcher, decision, reason, priority, enabled, created_by, updated_by)
    SELECT $3::text, $1::text, $9::text, $10::text, $11::text, $12::text, $13::integer, $14::boolean, $2::text, $2::text
    FROM room
    RETURNING id, created_by, created_at, updated_at
)` + orgHookBumpSQL

// orgHookUpdateSQL replaces an active rule's content in place, so its id and created_by survive. Parameters:
// $1 org, $2 actor, $3 rule id, $9 event, $10 matcher, $11 decision, $12 reason, $13 priority, $14 enabled.
const orgHookUpdateSQL = `
WITH changed AS (
    UPDATE org_hook_rules
    SET event = $9::text, tool_matcher = $10::text, decision = $11::text, reason = $12::text,
        priority = $13::integer, enabled = $14::boolean, updated_by = $2::text, updated_at = now()
    WHERE id = $3::text AND org_id = $1::text AND revoked_at IS NULL
    RETURNING id, created_by, created_at, updated_at
)` + orgHookBumpSQL

// orgHookRevokeSQL retires an active rule. Nothing is deleted: the row keeps its content and gains
// revoked_at, which also frees its (event, matcher, decision) key for a new rule.
const orgHookRevokeSQL = `
WITH changed AS (
    UPDATE org_hook_rules
    SET revoked_at = now(), updated_by = $2::text, updated_at = now()
    WHERE id = $3::text AND org_id = $1::text AND revoked_at IS NULL
    RETURNING id, created_by, created_at, updated_at
)` + orgHookBumpSQL

// orgHookWriteResult is what a rule write returns from the database.
type orgHookWriteResult struct {
	id        string
	createdBy string
	createdAt time.Time
	updatedAt time.Time
	revision  int64
}

// runOrgHookWrite runs one rule write statement. The bool is false when the statement changed no row: a
// create at the org's rule limit, or an update or revoke of a rule the org does not hold.
func (h *OrgHooksHandler) runOrgHookWrite(ctx context.Context, statement string, args ...any) (orgHookWriteResult, bool, error) {
	var result orgHookWriteResult
	err := h.pool.QueryRow(ctx, statement, args...).Scan(&result.id, &result.createdBy, &result.createdAt, &result.updatedAt, &result.revision)
	if errors.Is(err, pgx.ErrNoRows) {
		return orgHookWriteResult{}, false, nil
	}
	if err != nil {
		return orgHookWriteResult{}, false, err
	}
	return result, true, nil
}

// orgHookAuditDiff is the content-free diff a rule write records: the operation and, for a create or an
// update, the event, decision and enabled state. The matcher and the reason stay out: the matcher names
// the tools the admin chose and the reason is the admin's own free text.
func orgHookAuditDiff(op string, rule *orgHookRule) (string, error) {
	diff := map[string]any{"op": op}
	if rule != nil {
		diff["event"] = rule.event
		diff["decision"] = rule.decision
		diff["enabled"] = rule.enabled
	}
	raw, err := json.Marshal(diff)
	if err != nil {
		return "", err
	}
	return string(raw), nil
}

// orgHookWriteArgs are the parameters every rule write shares, numbered as the statements use them: the
// org, the actor, the rule id, the history and audit ids, the audit diff, and the default policy document
// and digest for an org that has no policy row yet.
func orgHookWriteArgs(orgID, actor, ruleID, auditDiff string) ([]any, error) {
	digest, doc, err := orgAgentPolicyDigest(orgAgentPolicyDefaults())
	if err != nil {
		return nil, err
	}
	return []any{
		orgID, actor, ruleID,
		"oaph_" + uuid.NewString(), "ral_" + uuid.NewString(), auditDiff,
		string(doc), digest,
	}, nil
}

// OrgHooksHandler serves /api/v1/org-hooks and /api/v1/org-hooks/{id}.
type OrgHooksHandler struct {
	pool registryDatabase
}

// NewOrgHooksHandler constructs the handler.
func NewOrgHooksHandler(pool registryDatabase) *OrgHooksHandler {
	return &OrgHooksHandler{pool: pool}
}

// Register mounts the routes on the mux behind the blanket JWT middleware.
func (h *OrgHooksHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc(orgHooksPath, func(w http.ResponseWriter, r *http.Request) {
		switch r.Method {
		case http.MethodGet:
			h.list(w, r)
		case http.MethodPost:
			h.create(w, r)
		default:
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
	mux.HandleFunc(orgHooksPath+"/", func(w http.ResponseWriter, r *http.Request) {
		id := strings.TrimPrefix(r.URL.Path, orgHooksPath+"/")
		if id == "" || strings.Contains(id, "/") {
			http.NotFound(w, r)
			return
		}
		switch r.Method {
		case http.MethodPut:
			h.update(w, r, id)
		case http.MethodDelete:
			h.revoke(w, r, id)
		default:
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
}

// orgHookWriter answers the refusals every rule write needs, before the body is read, and returns the
// verified admin.
func orgHookWriter(w http.ResponseWriter, r *http.Request) (authctx.Principal, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return principal, false
	}
	if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		jsonErr(w, "org_hook_write_under_zdr_refused", http.StatusForbidden)
		return principal, false
	}
	if !principal.HasScope(authz.WriteScope) {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return principal, false
	}
	if principal.PrincipalType != "user" {
		jsonErr(w, "only a person may write org hook rules", http.StatusForbidden)
		return principal, false
	}
	if !verifiedIsAdmin(r) {
		jsonErr(w, "only an admin may write org hook rules", http.StatusForbidden)
		return principal, false
	}
	return principal, true
}

// decodeOrgHookRule reads and validates a create or an update body. Unknown fields are refused, so a
// misspelt field can never be dropped from a rule without a word.
func decodeOrgHookRule(w http.ResponseWriter, r *http.Request) (orgHookRule, bool) {
	r.Body = http.MaxBytesReader(w, r.Body, maxOrgHookBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var body orgHookRuleBody
	if err := decoder.Decode(&body); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return orgHookRule{}, false
	}
	rule, refusal := validateOrgHookRule(body)
	if refusal != nil {
		jsonErr(w, refusal.code, refusal.status)
		return orgHookRule{}, false
	}
	return rule, true
}

// orgHookWriteFailed answers a rule write the database refused. A unique violation is the duplicate
// (event, matcher, decision) key; anything else is a database failure.
func orgHookWriteFailed(w http.ResponseWriter, err error) {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation {
		jsonErr(w, orgHookRefuseDuplicate.code, orgHookRefuseDuplicate.status)
		return
	}
	slog.Error("write org hook rule failed", "error", err)
	jsonErr(w, "database unavailable", http.StatusInternalServerError)
}

// answerOrgHookWrite builds the answer to a create or an update from the validated rule and the write's result.
func answerOrgHookWrite(rule orgHookRule, result orgHookWriteResult, actor string) orgHookWriteView {
	return orgHookWriteView{
		orgHookRuleView: orgHookRuleView{
			ID:          result.id,
			Event:       rule.event,
			ToolMatcher: rule.matcher,
			Decision:    rule.decision,
			Reason:      rule.reason,
			Priority:    rule.priority,
			Enabled:     rule.enabled,
			CreatedBy:   result.createdBy,
			UpdatedBy:   actor,
			CreatedAt:   result.createdAt,
			UpdatedAt:   result.updatedAt,
		},
		Revision: result.revision,
	}
}

// list is GET /api/v1/org-hooks: any member of the org reads the active rules and the policy revision
// they are at.
func (h *OrgHooksHandler) list(w http.ResponseWriter, r *http.Request) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	var revision int64
	if err := h.pool.QueryRow(r.Context(), orgHookRevisionSQL, orgID).Scan(&revision); err != nil && !errors.Is(err, pgx.ErrNoRows) {
		slog.Error("read org hook revision failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	rows, err := h.pool.Query(r.Context(), orgHookListSQL, orgID)
	if err != nil {
		slog.Error("list org hook rules failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	// Initialised, not `var`: an org with no rules must marshal as [] and not null.
	rules := []orgHookRuleView{}
	for rows.Next() {
		var rule orgHookRuleView
		if err := rows.Scan(&rule.ID, &rule.Event, &rule.ToolMatcher, &rule.Decision, &rule.Reason,
			&rule.Priority, &rule.Enabled, &rule.CreatedBy, &rule.UpdatedBy, &rule.CreatedAt, &rule.UpdatedAt); err != nil {
			slog.Error("scan org hook rule failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		rules = append(rules, rule)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list org hook rules failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"org_id": orgID, "revision": revision, "rules": rules})
}

// create is POST /api/v1/org-hooks: an org admin adds a rule. An org may hold at most maxOrgHookRules
// active rules.
func (h *OrgHooksHandler) create(w http.ResponseWriter, r *http.Request) {
	principal, ok := orgHookWriter(w, r)
	if !ok {
		return
	}
	rule, ok := decodeOrgHookRule(w, r)
	if !ok {
		return
	}
	ruleID := "ohr_" + uuid.NewString()
	auditDiff, err := orgHookAuditDiff("create", &rule)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	args, err := orgHookWriteArgs(principal.OrganizationID, principal.ActorID, ruleID, auditDiff)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	args = append(args, rule.event, rule.matcher, rule.decision, rule.reason, rule.priority, rule.enabled, maxOrgHookRules)
	result, found, err := h.runOrgHookWrite(r.Context(), orgHookCreateSQL, args...)
	if err != nil {
		orgHookWriteFailed(w, err)
		return
	}
	if !found {
		jsonErr(w, orgHookRefuseLimit.code, orgHookRefuseLimit.status)
		return
	}
	writeJSON(w, answerOrgHookWrite(rule, result, principal.ActorID))
}

// update is PUT /api/v1/org-hooks/{id}: an org admin replaces an active rule's content.
func (h *OrgHooksHandler) update(w http.ResponseWriter, r *http.Request, ruleID string) {
	principal, ok := orgHookWriter(w, r)
	if !ok {
		return
	}
	rule, ok := decodeOrgHookRule(w, r)
	if !ok {
		return
	}
	auditDiff, err := orgHookAuditDiff("update", &rule)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	args, err := orgHookWriteArgs(principal.OrganizationID, principal.ActorID, ruleID, auditDiff)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	args = append(args, rule.event, rule.matcher, rule.decision, rule.reason, rule.priority, rule.enabled)
	result, found, err := h.runOrgHookWrite(r.Context(), orgHookUpdateSQL, args...)
	if err != nil {
		orgHookWriteFailed(w, err)
		return
	}
	if !found {
		jsonErr(w, orgHookRefuseNotFound.code, orgHookRefuseNotFound.status)
		return
	}
	writeJSON(w, answerOrgHookWrite(rule, result, principal.ActorID))
}

// revoke is DELETE /api/v1/org-hooks/{id}: an org admin retires an active rule. The row stays as history.
func (h *OrgHooksHandler) revoke(w http.ResponseWriter, r *http.Request, ruleID string) {
	principal, ok := orgHookWriter(w, r)
	if !ok {
		return
	}
	auditDiff, err := orgHookAuditDiff("revoke", nil)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	args, err := orgHookWriteArgs(principal.OrganizationID, principal.ActorID, ruleID, auditDiff)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	result, found, err := h.runOrgHookWrite(r.Context(), orgHookRevokeSQL, args...)
	if err != nil {
		orgHookWriteFailed(w, err)
		return
	}
	if !found {
		jsonErr(w, orgHookRefuseNotFound.code, orgHookRefuseNotFound.status)
		return
	}
	writeJSON(w, map[string]any{"id": result.id, "revoked": true, "revision": result.revision})
}
