// Package api — per-tool allow / ask / never policies (WS05-S5).
//
// A person can set a policy on a capability's tools for themselves, and an org
// admin can set one for the whole org:
//
//   - ask and never tighten. They apply to every capability, built-in or MCP, and
//     the policy engine honours the strictest one in force whichever scope set it.
//   - allow relaxes, and is deliberately narrow. It is accepted only for an
//     `mcp_tool` row whose definition is approved, whose class and approval floor
//     the scope may relax (policy.CanRelax: a person up to an external write, an
//     admin up to a destructive tool, nobody an owner effect), and it must carry
//     the digest of the definition the person reviewed. The engine re-checks the
//     digest on every call, so an allow stops applying the moment the definition
//     changes and the tool is asked about again.
//
// The policies are stored here and read by the policy engine only when
// TOOL_POLICY_ENFORCED is "true" on capability-core; the API itself is not gated,
// so a person can set policies before the switch is turned on.
//
// The list carries an "enforced" bit beside the policies, so a settings page can
// tell a person whether the level they set is honoured yet (EP-10 S7).
//
// The two connected-app capabilities are tighten-only levels a person sees in
// settings: the read takes ask or never, and the provider write takes never. An
// ask on the write is refused as already_always_ask, because every provider
// write already pauses for the person.
//
// Who may write: a person (never a workload), carrying capability:write, with an
// explicit non-ZDR posture; an org-scope write also needs an admin scope from the
// signed principal. A ZDR principal can read and be evaluated but never writes:
// a policy is durable state. Every write and every revocation appends a
// registry_audit_log row carrying the scope, capability, decision and digest and
// never the person's free-text reason.
package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
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
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

const (
	toolPoliciesPath       = "/api/v1/tool-policies"
	maxToolPolicyBodyBytes = 4 << 10
	maxToolPolicyReasonLen = 500
	maxToolPoliciesListed  = 500

	auditActionToolPolicySet     = "tool_policy_set"
	auditActionToolPolicyRevoked = "tool_policy_revoked"
	auditEntityToolPolicy        = "tool_policy"

	// A confirmed "Always" rule (build plan WS13 S7, K3-10) is an allow with an expiry:
	// ninety days by default, and no confirmation may ask for longer than a year.
	ruleExpiryDefault = 90 * 24 * time.Hour
	ruleExpiryMax     = 365 * 24 * time.Hour

	auditActionToolPolicyRuleCreated   = "tool_policy_rule_created"
	auditActionToolPolicyRuleConfirmed = "tool_policy_rule_confirmed"

	// auditActionToolPolicyRuleUsed is the row written when a stored allow relaxes a call on a persistent
	// run (WS13 S7, K3-10).
	auditActionToolPolicyRuleUsed = "tool_policy_rule_used"

	// pgUniqueViolation is the SQLSTATE of the active-key index refusing a second
	// concurrent first write.
	pgUniqueViolation = "23505"

	// The provider write (EP-10 S7), whose every call already asks the person.
	providerExecuteCapability = "cap.tool.provider.execute"
	errAlreadyAlwaysAsk       = "already_always_ask"
)

// ToolPoliciesHandler serves /api/v1/tool-policies and is the policy engine's
// source of stored policies.
type ToolPoliciesHandler struct {
	pool registryDatabase
	// enforced is TOOL_POLICY_ENFORCED as main read it. It only reports whether
	// the engine honours the stored policies; the handler never decides with it.
	enforced bool
}

// NewToolPoliciesHandler constructs the handler.
func NewToolPoliciesHandler(pool registryDatabase) *ToolPoliciesHandler {
	return &ToolPoliciesHandler{pool: pool}
}

// WithEnforced records whether the policy engine honours the stored policies, so
// the list can say so. Left unset, the list reports false: a level is never shown
// as honoured on a guess.
func (h *ToolPoliciesHandler) WithEnforced(enforced bool) *ToolPoliciesHandler {
	h.enforced = enforced
	return h
}

// toolPolicyListResponse is the list body: the policies and whether the engine
// honours them.
type toolPolicyListResponse struct {
	Policies []toolPolicyView `json:"policies"`
	Enforced bool             `json:"enforced"`
}

// Register mounts the routes on the mux behind the blanket JWT middleware.
func (h *ToolPoliciesHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc(toolPoliciesPath, func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet {
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		h.list(w, r)
	})
	mux.HandleFunc(toolPoliciesPath+"/", func(w http.ResponseWriter, r *http.Request) {
		capabilityID := strings.TrimPrefix(r.URL.Path, toolPoliciesPath+"/")
		if !mcpIdentifier.MatchString(capabilityID) {
			http.NotFound(w, r)
			return
		}
		switch r.Method {
		case http.MethodPut:
			h.set(w, r, capabilityID)
		case http.MethodDelete:
			h.revoke(w, r, capabilityID)
		default:
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
}

// ActiveToolPolicies implements policy.ToolPolicySource: the org's active
// policies on a capability and, when userID is set, that person's own.
func (h *ToolPoliciesHandler) ActiveToolPolicies(ctx context.Context, orgID, userID, capabilityID string) ([]policy.ToolPolicy, error) {
	rows, err := h.pool.Query(ctx, `
		SELECT id, scope_kind, decision, COALESCE(pinned_digest, ''), expires_at
		FROM tool_policies
		WHERE org_id = $1 AND capability_id = $2 AND revoked_at IS NULL
		  AND (scope_kind = 'org' OR (scope_kind = 'user' AND $3::text <> '' AND scope_value = $3::text))
	`, orgID, capabilityID, userID)
	if err != nil {
		return nil, fmt.Errorf("query tool policies: %w", err)
	}
	defer rows.Close()
	var policies []policy.ToolPolicy
	for rows.Next() {
		var p policy.ToolPolicy
		var expiresAt *time.Time
		if err := rows.Scan(&p.ID, &p.ScopeKind, &p.Decision, &p.PinnedDigest, &expiresAt); err != nil {
			return nil, fmt.Errorf("scan tool policy: %w", err)
		}
		if expiresAt != nil {
			p.ExpiresAt = *expiresAt
		}
		policies = append(policies, p)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read tool policies: %w", err)
	}
	return policies, nil
}

// RuleUse is one use of a stored allow that relaxed a call. It holds ids, closed words and a definition
// digest only: never tool input, a reason or any person's text.
type RuleUse struct {
	// OrgID is the org the rule and the run belong to.
	OrgID string
	// PolicyID is the id of the stored allow that relaxed the call.
	PolicyID string
	// CapabilityID is the capability the call was made on.
	CapabilityID string
	// ScopeKind is the allow's scope: org or user.
	ScopeKind string
	// RunID is the persistent run the call belongs to.
	RunID string
	// ToolDigest is the pinned definition digest of the tool; empty without a binding.
	ToolDigest string
	// Actor is the service principal that asked for the evaluation.
	Actor string
}

// RecordRuleUse appends one tool_policy_rule_used row to registry_audit_log. The caller fails the decision
// closed when it returns an error, so a rule is never used without its row.
func (h *ToolPoliciesHandler) RecordRuleUse(ctx context.Context, use RuleUse) error {
	diff, err := json.Marshal(map[string]any{
		"capability_id": use.CapabilityID,
		"scope_kind":    use.ScopeKind,
		"run_id":        use.RunID,
		"tool_digest":   use.ToolDigest,
	})
	if err != nil {
		return fmt.Errorf("encode rule use: %w", err)
	}
	if _, err := h.pool.Exec(ctx, `
		INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb)
	`, "ral_"+uuid.NewString(), auditEntityToolPolicy, use.PolicyID, auditActionToolPolicyRuleUsed, use.Actor, use.OrgID, string(diff)); err != nil {
		return fmt.Errorf("record rule use: %w", err)
	}
	return nil
}

type toolPolicyView struct {
	ID           string     `json:"id"`
	ScopeKind    string     `json:"scope_kind"`
	ScopeValue   string     `json:"scope_value"`
	CapabilityID string     `json:"capability_id"`
	Decision     string     `json:"decision"`
	PinnedDigest string     `json:"pinned_digest"`
	Reason       string     `json:"reason"`
	SetBy        string     `json:"set_by"`
	CreatedAt    time.Time  `json:"created_at"`
	UpdatedAt    time.Time  `json:"updated_at"`
	ExpiresAt    *time.Time `json:"expires_at,omitempty"`
}

// list is GET /api/v1/tool-policies?capability_id=: the org's active policies and
// the caller's own, with whether the engine honours them. Another person's policy
// is never returned.
func (h *ToolPoliciesHandler) list(w http.ResponseWriter, r *http.Request) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	capabilityID := r.URL.Query().Get("capability_id")
	if capabilityID != "" && !mcpIdentifier.MatchString(capabilityID) {
		jsonErr(w, "capability_id is invalid", http.StatusBadRequest)
		return
	}
	rows, err := h.pool.Query(r.Context(), `
		SELECT id, scope_kind, scope_value, capability_id, decision, COALESCE(pinned_digest, ''), reason, set_by, created_at, updated_at, expires_at
		FROM tool_policies
		WHERE org_id = $1 AND revoked_at IS NULL
		  AND (scope_kind = 'org' OR (scope_kind = 'user' AND scope_value = $2))
		  AND ($3::text = '' OR capability_id = $3)
		ORDER BY capability_id, scope_kind
		LIMIT $4
	`, orgID, verifiedActorID(r), capabilityID, maxToolPoliciesListed)
	if err != nil {
		slog.Error("list tool policies failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	// Initialised, not `var`: an empty list must marshal as [] and not null.
	views := []toolPolicyView{}
	for rows.Next() {
		var v toolPolicyView
		if err := rows.Scan(&v.ID, &v.ScopeKind, &v.ScopeValue, &v.CapabilityID, &v.Decision, &v.PinnedDigest, &v.Reason, &v.SetBy, &v.CreatedAt, &v.UpdatedAt, &v.ExpiresAt); err != nil {
			slog.Error("scan tool policy failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		views = append(views, v)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list tool policies failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, toolPolicyListResponse{Policies: views, Enforced: h.enforced})
}

type setToolPolicyRequest struct {
	// Scope is "user" (the default) or "org".
	Scope    string `json:"scope"`
	Decision string `json:"decision"`
	// PinnedDigest is the pinned_digest of the tool definition the person
	// reviewed. Required for an allow, refused for ask and never.
	PinnedDigest string `json:"pinned_digest"`
	Reason       string `json:"reason"`
	// ExpiresAt is when an allow lapses, as RFC 3339. Only an allow may carry one; left
	// out, an allow lapses ninety days from the write.
	ExpiresAt string `json:"expires_at"`
}

// ruleExpiry is when an allow written at now lapses (build plan WS13 S7, K3-10): the time the
// request asks for, or ninety days when it asks for none. A lapse must lie after now and no more
// than a year after it.
func ruleExpiry(now time.Time, requested string) (time.Time, error) {
	if requested == "" {
		return now.Add(ruleExpiryDefault), nil
	}
	at, err := time.Parse(time.RFC3339, requested)
	if err != nil {
		return time.Time{}, errors.New("expires_at must be an RFC 3339 time")
	}
	if !at.After(now) {
		return time.Time{}, errors.New("expires_at must be in the future")
	}
	if at.After(now.Add(ruleExpiryMax)) {
		return time.Time{}, errors.New("expires_at is limited to 365 days")
	}
	return at, nil
}

// policyWriter checks what every write needs, before the body is read, and
// returns the verified principal. It answers the refusal itself.
func policyWriter(w http.ResponseWriter, r *http.Request) (authctx.Principal, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return principal, false
	}
	// A policy is durable state: a ZDR principal, and one whose posture was never
	// stated, may not write one. Evaluation under ZDR is unaffected.
	if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		jsonErr(w, "tool_policy_write_under_zdr_refused", http.StatusForbidden)
		return principal, false
	}
	if !principal.HasScope(authz.WriteScope) {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return principal, false
	}
	// A policy is a person's decision, never a workload's.
	if principal.PrincipalType != "user" {
		jsonErr(w, "only a person may set a tool policy", http.StatusForbidden)
		return principal, false
	}
	return principal, true
}

// orgScopeAllowed answers the refusal itself when scope is the org's and the
// signed principal is not an admin.
func orgScopeAllowed(w http.ResponseWriter, r *http.Request, scope string) bool {
	if scope == policy.ToolPolicyScopeOrg && !verifiedIsAdmin(r) {
		jsonErr(w, "only an admin may set an org tool policy", http.StatusForbidden)
		return false
	}
	return true
}

func scopeValueOf(scope, orgID, actorID string) string {
	if scope == policy.ToolPolicyScopeOrg {
		return orgID
	}
	return actorID
}

// set is PUT /api/v1/tool-policies/{capability_id}. Setting a policy again
// updates the active row in place, so its id survives.
func (h *ToolPoliciesHandler) set(w http.ResponseWriter, r *http.Request, capabilityID string) {
	principal, ok := policyWriter(w, r)
	if !ok {
		return
	}
	orgID := principal.OrganizationID
	r.Body = http.MaxBytesReader(w, r.Body, maxToolPolicyBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var request setToolPolicyRequest
	if err := decoder.Decode(&request); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	if request.Scope == "" {
		request.Scope = policy.ToolPolicyScopeUser
	}
	if request.Scope != policy.ToolPolicyScopeUser && request.Scope != policy.ToolPolicyScopeOrg {
		jsonErr(w, "scope must be user or org", http.StatusBadRequest)
		return
	}
	if !orgScopeAllowed(w, r, request.Scope) {
		return
	}
	switch request.Decision {
	case policy.ToolPolicyAllow:
		if !sha256HexPattern.MatchString(request.PinnedDigest) {
			jsonErr(w, "an allow needs pinned_digest, the digest of the reviewed tool definition (sha256 hex)", http.StatusBadRequest)
			return
		}
	case policy.ToolPolicyAsk, policy.ToolPolicyNever:
		if request.PinnedDigest != "" {
			jsonErr(w, "pinned_digest is only for an allow", http.StatusBadRequest)
			return
		}
	default:
		jsonErr(w, "decision must be allow, ask or never", http.StatusBadRequest)
		return
	}
	// Every provider write already asks the person, so an ask on it changes
	// nothing and is refused rather than stored as a level that looks chosen.
	if capabilityID == providerExecuteCapability && request.Decision == policy.ToolPolicyAsk {
		writeTypedError(w, http.StatusUnprocessableEntity, errAlreadyAlwaysAsk, "Every action with this app already asks you first.")
		return
	}
	var expiresAt *time.Time
	if request.Decision == policy.ToolPolicyAllow {
		at, err := ruleExpiry(time.Now().UTC(), request.ExpiresAt)
		if err != nil {
			jsonErr(w, err.Error(), http.StatusBadRequest)
			return
		}
		expiresAt = &at
	} else if request.ExpiresAt != "" {
		jsonErr(w, "expires_at is only for an allow", http.StatusBadRequest)
		return
	}
	if utf8.RuneCountInString(request.Reason) > maxToolPolicyReasonLen {
		jsonErr(w, fmt.Sprintf("reason is limited to %d characters", maxToolPolicyReasonLen), http.StatusBadRequest)
		return
	}

	var kind string
	err := h.pool.QueryRow(r.Context(), `
		SELECT kind FROM capabilities
		WHERE id = $1 AND (org_id = $2 OR org_id = 'global') AND deleted_at IS NULL
	`, capabilityID, orgID).Scan(&kind)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("read capability for tool policy failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	if request.Decision == policy.ToolPolicyAllow {
		if status, msg := h.checkRelaxable(r.Context(), orgID, capabilityID, kind, request); status != 0 {
			jsonErr(w, msg, status)
			return
		}
	}

	scopeValue := scopeValueOf(request.Scope, orgID, principal.ActorID)
	diff := map[string]any{
		"capability_id": capabilityID,
		"scope_kind":    request.Scope,
		"scope_value":   scopeValue,
		"decision":      request.Decision,
		"pinned_digest": request.PinnedDigest,
	}
	// A rule is audited as created on its first write and as confirmed on every later one, so a
	// renewal reads as a confirmation. Its row carries the lapse time and nothing the person wrote.
	updateAction, insertAction := auditActionToolPolicySet, auditActionToolPolicySet
	if expiresAt != nil {
		diff["expires_at"] = expiresAt.Format(time.RFC3339)
		updateAction, insertAction = auditActionToolPolicyRuleConfirmed, auditActionToolPolicyRuleCreated
	}
	auditDiff, err := json.Marshal(diff)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	var id string
	err = h.pool.QueryRow(r.Context(), setToolPolicySQL,
		orgID, request.Scope, scopeValue, capabilityID, request.Decision, nullableDigest(request.PinnedDigest),
		request.Reason, principal.ActorID, "tpol_"+uuid.NewString(), "ral_"+uuid.NewString(),
		updateAction, string(auditDiff), expiresAt, insertAction,
	).Scan(&id)
	if err != nil {
		var pgErr *pgconn.PgError
		if errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation {
			// Two first writes raced; the other one won. Nothing was written here.
			jsonErr(w, "the policy was changed at the same time; retry", http.StatusConflict)
			return
		}
		slog.Error("set tool policy failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	response := map[string]any{
		"id":            id,
		"capability_id": capabilityID,
		"scope_kind":    request.Scope,
		"decision":      request.Decision,
		"pinned_digest": request.PinnedDigest,
	}
	if expiresAt != nil {
		response["expires_at"] = expiresAt.Format(time.RFC3339)
	}
	writeJSON(w, response)
}

func nullableDigest(digest string) any {
	if digest == "" {
		return nil
	}
	return digest
}

// checkRelaxable returns the refusal for an allow that may not be stored, or 0.
// It reads the tool's own registry row, never anything from the request but the
// digest the person says they reviewed.
func (h *ToolPoliciesHandler) checkRelaxable(ctx context.Context, orgID, capabilityID, kind string, request setToolPolicyRequest) (int, string) {
	if kind != models.KindMCPTool {
		return http.StatusUnprocessableEntity, "only an MCP tool may be allowed by a policy; a built-in tool's approval is never relaxed"
	}
	var class, floor, pinned, state string
	err := h.pool.QueryRow(ctx, `
		SELECT side_effect_class, approval_floor, COALESCE(pinned_digest, ''), COALESCE(review_state, '')
		FROM capability_tools
		WHERE org_id = $1 AND capability_id = $2 AND qualified_name IS NOT NULL
	`, orgID, capabilityID).Scan(&class, &floor, &pinned, &state)
	if errors.Is(err, pgx.ErrNoRows) {
		return http.StatusNotFound, "not found"
	}
	if err != nil {
		slog.Error("read tool row for tool policy failed", "error", err)
		return http.StatusInternalServerError, "database unavailable"
	}
	if state != reviewApproved {
		return http.StatusConflict, "the tool's definition is not approved; approve it before allowing it"
	}
	if !policy.CanRelax(request.Scope, class, floor) {
		return http.StatusUnprocessableEntity, "this scope may not relax a tool of this class"
	}
	if pinned == "" || pinned != request.PinnedDigest {
		return http.StatusConflict, "the tool's pinned definition is not the one that was reviewed; review it again"
	}
	return 0, ""
}

// setToolPolicySQL updates the active policy of the key or inserts the first
// one, and appends the audit row, as one statement. Parameters: $1 org, $2 scope
// kind, $3 scope value, $4 capability, $5 decision, $6 pinned digest, $7 reason,
// $8 actor, $9 new policy id, $10 audit id, $11 audit action when the policy
// existed, $12 audit diff, $13 lapse time (NULL for a policy that does not lapse),
// $14 audit action when it was created.
const setToolPolicySQL = `
WITH upd AS (
    UPDATE tool_policies
    SET decision = $5, pinned_digest = $6, reason = $7, set_by = $8, expires_at = $13::timestamptz, expiry_audited_at = NULL, updated_at = now()
    WHERE org_id = $1 AND scope_kind = $2 AND scope_value = $3 AND capability_id = $4 AND revoked_at IS NULL
    RETURNING id
), ins AS (
    INSERT INTO tool_policies (id, org_id, scope_kind, scope_value, capability_id, decision, pinned_digest, reason, set_by, expires_at)
    SELECT $9, $1, $2, $3, $4, $5, $6, $7, $8, $13::timestamptz
    WHERE NOT EXISTS (SELECT 1 FROM upd)
    RETURNING id
), pol AS (
    SELECT id FROM upd UNION ALL SELECT id FROM ins
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $10, '` + auditEntityToolPolicy + `', pol.id,
           CASE WHEN EXISTS (SELECT 1 FROM upd) THEN $11::text ELSE $14::text END,
           $8, $1, $12::jsonb FROM pol
    RETURNING 1
)
SELECT id FROM pol
`

// revoke is DELETE /api/v1/tool-policies/{capability_id}?scope=user|org. The row
// is kept with revoked_at set; the next set starts a new one.
func (h *ToolPoliciesHandler) revoke(w http.ResponseWriter, r *http.Request, capabilityID string) {
	scope := r.URL.Query().Get("scope")
	if scope == "" {
		scope = policy.ToolPolicyScopeUser
	}
	if scope != policy.ToolPolicyScopeUser && scope != policy.ToolPolicyScopeOrg {
		jsonErr(w, "scope must be user or org", http.StatusBadRequest)
		return
	}
	principal, ok := policyWriter(w, r)
	if !ok {
		return
	}
	if !orgScopeAllowed(w, r, scope) {
		return
	}
	orgID := principal.OrganizationID
	scopeValue := scopeValueOf(scope, orgID, principal.ActorID)
	auditDiff, err := json.Marshal(map[string]any{
		"capability_id": capabilityID,
		"scope_kind":    scope,
		"scope_value":   scopeValue,
	})
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	var id string
	err = h.pool.QueryRow(r.Context(), `
		WITH rev AS (
		    UPDATE tool_policies SET revoked_at = now(), updated_at = now()
		    WHERE org_id = $1 AND scope_kind = $2 AND scope_value = $3 AND capability_id = $4 AND revoked_at IS NULL
		    RETURNING id
		), aud AS (
		    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
		    SELECT $5, '`+auditEntityToolPolicy+`', rev.id, $6, $7, $1, $8::jsonb FROM rev
		    RETURNING 1
		)
		SELECT id FROM rev
	`, orgID, scope, scopeValue, capabilityID, "ral_"+uuid.NewString(), auditActionToolPolicyRevoked, principal.ActorID, string(auditDiff)).Scan(&id)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}
	if err != nil {
		slog.Error("revoke tool policy failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"id": id, "capability_id": capabilityID, "scope_kind": scope, "revoked": true})
}
