// Package api — org permission rules (build plan W5-074-b, F-t3-074, DEC-15).
//
// capability-core is the system of record for an org's permission rules: the deny and ask rules the signed
// EvaluatePolicy gate applies to one named tool call. A rule matches the tool and a target execution-core derived
// from the call's input (a web host, a mail recipient's domain, a command's first token). A rule narrows a call and
// never widens one, so nothing here stores or matches an allow. This file holds the table's reader, the per-org cache
// the gate reads through, the pure matcher, and the org admin's routes: GET and POST on /api/v1/org-permission-rules
// and DELETE on /api/v1/org-permission-rules/{id} (W5-074-c). Every write bumps the org's policy revision and appends
// a content-free audit row, as the org hook rules do.
//
// The gate reads these rules only while CAPABILITY_PERMISSION_RULES_ENABLED is the literal "true" on capability-core.
// The switch is parsed here, as the other switches of this package are, and main hands the result to the server. The
// admin routes answer 404 while the same switch is off.
package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"sync"
	"time"
	"unicode"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/policy"
)

// PermissionRulesEnabledEnv is the switch that lets the EvaluatePolicy gate read the org permission rules.
const PermissionRulesEnabledEnv = "CAPABILITY_PERMISSION_RULES_ENABLED"

// PermissionRulesEnabledFromLookup reads PermissionRulesEnabledEnv strictly: only the literal "true" enables it, so a
// typo or a stray "1" never switches the read on.
func PermissionRulesEnabledFromLookup(lookup func(string) string) bool {
	return lookup(PermissionRulesEnabledEnv) == "true"
}

// The target kinds a rule may name. execution-core derives a call's kind (EvaluatePolicyRequest.target_kind).
// PermissionTargetAny is the kind of a rule that covers every call of its tool, whatever the call's target.
const (
	PermissionTargetDomain        = "domain"
	PermissionTargetRecipient     = "recipient"
	PermissionTargetCommandPrefix = "command_prefix"
	PermissionTargetAny           = "any"
)

const (
	// permissionRuleCacheTTL is how long an org's rules are served from memory before the table is read again.
	permissionRuleCacheTTL = 30 * time.Second
	// maxPermissionRulesPerOrg bounds one org's rules. The admin write holds an org to this many, so a table that holds
	// more fails the read: the gate never decides on a part of an org's rules.
	maxPermissionRulesPerOrg = 200
	// maxCachedPermissionOrgs is the cache size at which expired entries are dropped.
	maxCachedPermissionOrgs = 1024
)

// permissionRuleListSQL reads an org's rules in rule-id order. It asks for one row more than the bound, so a table over
// the bound is seen as over the bound and is not silently cut down to it.
const permissionRuleListSQL = `
SELECT rule_id::text, tool_pattern, target_kind, target_pattern, decision
FROM org_permission_rules
WHERE org_id = $1
ORDER BY rule_id
LIMIT $2
`

// PermissionRule is one stored rule as the gate reads it. A rule never allows.
type PermissionRule struct {
	// ID is the rule's uuid. A decision the rule makes names it as org_rule:<ID>.
	ID string

	// ToolPattern is "*" (any tool), a "prefix*" glob or an exact tool name.
	ToolPattern string

	// TargetKind is one of the PermissionTarget constants.
	TargetKind string

	// TargetPattern is "*", an exact value, or "*.suffix" for a domain.
	TargetPattern string

	// Decision is policy.DecisionDeny or policy.DecisionAsk.
	Decision string
}

// PermissionRuleStore reads an org's rules from the durable table and serves them from a per-org cache for
// permissionRuleCacheTTL. A failed read is not cached, so the next evaluation reads the table again. Build it with
// NewPermissionRuleStore.
type PermissionRuleStore struct {
	pool  registryDatabase
	ttl   time.Duration
	now   func() time.Time
	mu    sync.Mutex
	cache map[string]permissionRuleEntry
}

// permissionRuleEntry is one org's rules and the time until which they are fresh.
type permissionRuleEntry struct {
	rules   []PermissionRule
	expires time.Time
}

// NewPermissionRuleStore constructs the store over the capability-core database.
func NewPermissionRuleStore(pool registryDatabase) *PermissionRuleStore {
	return &PermissionRuleStore{
		pool:  pool,
		ttl:   permissionRuleCacheTTL,
		now:   time.Now,
		cache: make(map[string]permissionRuleEntry),
	}
}

// Load returns an org's rules: from the cache while they are fresh, otherwise from the table.
func (s *PermissionRuleStore) Load(ctx context.Context, orgID string) ([]PermissionRule, error) {
	s.mu.Lock()
	entry, cached := s.cache[orgID]
	s.mu.Unlock()
	if cached && s.now().Before(entry.expires) {
		return entry.rules, nil
	}
	rules, err := s.read(ctx, orgID)
	if err != nil {
		return nil, err
	}
	s.remember(orgID, rules)
	return rules, nil
}

// Match decides one call under the org's rules: the rules are loaded, then MatchPermissionRules picks the decision. The
// error is a failed read, which the gate answers according to the call's class.
func (s *PermissionRuleStore) Match(ctx context.Context, orgID, tool, kind, value string) (string, string, error) {
	rules, err := s.Load(ctx, orgID)
	if err != nil {
		return "", "", err
	}
	decision, ruleID := MatchPermissionRules(rules, tool, kind, value)
	return decision, ruleID, nil
}

// read queries the table for an org's rules.
func (s *PermissionRuleStore) read(ctx context.Context, orgID string) ([]PermissionRule, error) {
	rows, err := s.pool.Query(ctx, permissionRuleListSQL, orgID, maxPermissionRulesPerOrg+1)
	if err != nil {
		return nil, fmt.Errorf("query org permission rules: %w", err)
	}
	defer rows.Close()
	rules := make([]PermissionRule, 0)
	for rows.Next() {
		var rule PermissionRule
		if err := rows.Scan(&rule.ID, &rule.ToolPattern, &rule.TargetKind, &rule.TargetPattern, &rule.Decision); err != nil {
			return nil, fmt.Errorf("scan org permission rule: %w", err)
		}
		rules = append(rules, rule)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read org permission rules: %w", err)
	}
	if len(rules) > maxPermissionRulesPerOrg {
		return nil, fmt.Errorf("org holds more than %d permission rules", maxPermissionRulesPerOrg)
	}
	return rules, nil
}

// remember caches an org's rules for permissionRuleCacheTTL. At the size bound it first drops the entries that have
// expired.
func (s *PermissionRuleStore) remember(orgID string, rules []PermissionRule) {
	now := s.now()
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.cache) >= maxCachedPermissionOrgs {
		for id, entry := range s.cache {
			if !now.Before(entry.expires) {
				delete(s.cache, id)
			}
		}
	}
	s.cache[orgID] = permissionRuleEntry{rules: rules, expires: now.Add(s.ttl)}
}

// MatchPermissionRules decides one call under a set of rules. It returns the decision and the id of the rule that made
// it, or two empty strings when no rule applies. A deny outranks an ask whatever the order of the rules, so the first
// deny that applies is the answer; failing one, the first ask that applies is. A rule whose decision is outside the
// vocabulary asks, as a stored policy of an unknown decision does on the same gate.
func MatchPermissionRules(rules []PermissionRule, tool, kind, value string) (decision, ruleID string) {
	for _, rule := range rules {
		if !permissionRuleApplies(rule, tool, kind, value) {
			continue
		}
		if rule.Decision == policy.DecisionDeny {
			return policy.DecisionDeny, rule.ID
		}
		if decision == "" {
			decision, ruleID = policy.DecisionAsk, rule.ID
		}
	}
	return decision, ruleID
}

// permissionRuleApplies reports whether a rule covers a call: its tool pattern names the call's tool and its target
// names the call's target.
func permissionRuleApplies(rule PermissionRule, tool, kind, value string) bool {
	return toolPatternMatches(rule.ToolPattern, tool) && targetPatternMatches(rule.TargetKind, rule.TargetPattern, kind, value)
}

// toolPatternMatches reads a tool pattern: "*" is every tool, including a call that names none; "prefix*" is every tool
// with that prefix; anything else is that exact name.
func toolPatternMatches(pattern, tool string) bool {
	switch {
	case pattern == "*":
		return true
	case strings.HasSuffix(pattern, "*"):
		return strings.HasPrefix(tool, strings.TrimSuffix(pattern, "*"))
	default:
		return pattern != "" && pattern == tool
	}
}

// targetPatternMatches reads a rule's target against the call's. A rule of kind "any" covers every call whatever its
// target, so it still applies on a ZDR or waived run, which sends no value. Any other rule needs the call's kind to be
// its kind, and a "*" pattern then covers every target of that kind. A ZDR or waived run sends the kind and withholds
// the value (execution-core a_retention_bound_call_names_no_target_value). A rule of the kind cannot be ruled out of
// covering the withheld target, so it covers the call as a "*" pattern does: a deny it makes stands and an ask it makes
// asks. Such a run is never decided more loosely than the same call on a persistent run. Domains and recipients compare
// case-insensitively: "*.suffix" covers the suffix itself and every subdomain of it, and any other pattern is that exact
// domain. A command prefix is that exact token, or every token with the prefix when the pattern ends in "*".
func targetPatternMatches(ruleKind, pattern, kind, value string) bool {
	if ruleKind == PermissionTargetAny {
		return true
	}
	if ruleKind != kind {
		return false
	}
	if pattern == "*" {
		return true
	}
	switch ruleKind {
	case PermissionTargetCommandPrefix:
		if value == "" {
			return true
		}
		if strings.HasSuffix(pattern, "*") {
			return strings.HasPrefix(value, strings.TrimSuffix(pattern, "*"))
		}
		return pattern != "" && pattern == value
	case PermissionTargetDomain, PermissionTargetRecipient:
		if value == "" {
			return true
		}
		host, want := strings.ToLower(value), strings.ToLower(pattern)
		if want == "" {
			return false
		}
		if suffix, wildcard := strings.CutPrefix(want, "*."); wildcard {
			return suffix != "" && (host == suffix || strings.HasSuffix(host, "."+suffix))
		}
		return host == want
	default:
		return false
	}
}

// ---------------------------------------------------------------------------
// The org admin's routes (W5-074-c). The write follows the org hook rules (org_hooks.go): an org admin acting as a
// person, with the write scope and a non-ZDR posture, every claim read from the verified principal. Each change bumps
// the org's policy revision in the same statement, so the gate's cache and the policy document move together, and it
// appends a content-free audit row (the rule id, the decision and the target kind; never the patterns).
// ---------------------------------------------------------------------------

const (
	// orgPermissionRulesPath is the collection: GET lists the org's rules and POST adds one. DELETE names a rule under
	// orgPermissionRulesPath and the rule's id.
	orgPermissionRulesPath = "/api/v1/org-permission-rules"

	// maxPermissionRuleBodyBytes bounds a create body. A rule is four short strings.
	maxPermissionRuleBodyBytes = 4 << 10
	// maxPermissionPatternLength bounds a tool pattern and a target pattern.
	maxPermissionPatternLength = 200
	// maxPermissionCommandToken bounds a command_prefix pattern. execution-core cuts a call's command token at 64
	// characters, so a longer pattern could never match one.
	maxPermissionCommandToken = 64
)

// permissionRuleRefusal is one closed refusal code and the status it answers with.
type permissionRuleRefusal struct {
	code   string
	status int
}

var (
	permissionRuleRefuseInvalidTool     = permissionRuleRefusal{"invalid_tool_pattern", http.StatusBadRequest}
	permissionRuleRefuseInvalidKind     = permissionRuleRefusal{"invalid_target_kind", http.StatusBadRequest}
	permissionRuleRefuseInvalidTarget   = permissionRuleRefusal{"invalid_target_pattern", http.StatusBadRequest}
	permissionRuleRefuseInvalidDecision = permissionRuleRefusal{"invalid_decision", http.StatusBadRequest}
	permissionRuleRefuseDuplicate       = permissionRuleRefusal{"duplicate_rule", http.StatusConflict}
	permissionRuleRefuseLimit           = permissionRuleRefusal{"rule_limit_reached", http.StatusUnprocessableEntity}
	permissionRuleRefuseNotFound        = permissionRuleRefusal{"rule_not_found", http.StatusNotFound}
)

// permissionRuleBody is a create request. Unknown fields are refused, so a misspelt field can never be dropped from a
// rule without a word.
type permissionRuleBody struct {
	ToolPattern   string `json:"tool_pattern"`
	TargetKind    string `json:"target_kind"`
	TargetPattern string `json:"target_pattern"`
	Decision      string `json:"decision"`
}

// permissionRule is one rule as the handler stores it, after validation.
type permissionRule struct {
	toolPattern   string
	targetKind    string
	targetPattern string
	decision      string
}

// permissionRuleView is a rule as the API returns it.
type permissionRuleView struct {
	ID            string    `json:"id"`
	ToolPattern   string    `json:"tool_pattern"`
	TargetKind    string    `json:"target_kind"`
	TargetPattern string    `json:"target_pattern"`
	Decision      string    `json:"decision"`
	CreatedBy     string    `json:"created_by"`
	CreatedAt     time.Time `json:"created_at"`
}

// permissionRuleWriteView is the answer to a create: the rule and the policy revision the write produced.
type permissionRuleWriteView struct {
	permissionRuleView
	Revision int64 `json:"revision"`
}

// validatePermissionRule checks a written rule and returns it in the form the table keeps: the patterns canonical, a
// domain in lower case, the decision case-folded. The checks run in a fixed order, so one input always gets one refusal.
func validatePermissionRule(body permissionRuleBody) (permissionRule, *permissionRuleRefusal) {
	tool, ok := normalisePermissionToolPattern(body.ToolPattern)
	if !ok {
		return permissionRule{}, &permissionRuleRefuseInvalidTool
	}
	switch body.TargetKind {
	case PermissionTargetDomain, PermissionTargetRecipient, PermissionTargetCommandPrefix, PermissionTargetAny:
	default:
		return permissionRule{}, &permissionRuleRefuseInvalidKind
	}
	target, ok := normalisePermissionTarget(body.TargetKind, body.TargetPattern)
	if !ok {
		return permissionRule{}, &permissionRuleRefuseInvalidTarget
	}
	decision := strings.ToLower(strings.TrimSpace(body.Decision))
	if decision != policy.DecisionDeny && decision != policy.DecisionAsk {
		return permissionRule{}, &permissionRuleRefuseInvalidDecision
	}
	return permissionRule{toolPattern: tool, targetKind: body.TargetKind, targetPattern: target, decision: decision}, nil
}

// normalisePermissionToolPattern returns a tool pattern in the form the gate matches: "*" for any tool, a name ending in
// one "*", or an exact name. An empty pattern is refused, so a rule never covers every tool by omission, and so is any
// pattern with whitespace or with a "*" anywhere but at the end.
func normalisePermissionToolPattern(raw string) (string, bool) {
	if raw == "*" {
		return raw, true
	}
	if raw == "" || len(raw) > maxPermissionPatternLength || strings.IndexFunc(raw, unicode.IsSpace) >= 0 {
		return "", false
	}
	if !mcpIdentifier.MatchString(strings.TrimSuffix(raw, "*")) {
		return "", false
	}
	return raw, true
}

// normalisePermissionTarget returns a target pattern in the form targetPatternMatches reads. A kind of "any" takes "*"
// only. A domain or a recipient is lower-cased and must be a DNS name, or "*." and one, so a mail recipient's address
// can never be stored: only its domain. A command prefix is an exact token, or a token ending in one "*".
func normalisePermissionTarget(kind, raw string) (string, bool) {
	if len(raw) > maxPermissionPatternLength {
		return "", false
	}
	switch kind {
	case PermissionTargetAny:
		if raw == "" || raw == "*" {
			return "*", true
		}
	case PermissionTargetDomain, PermissionTargetRecipient:
		domain := strings.ToLower(raw)
		if domain == "*" || isPermissionDomainName(strings.TrimPrefix(domain, "*.")) {
			return domain, true
		}
	case PermissionTargetCommandPrefix:
		token := strings.TrimSuffix(raw, "*")
		if raw == "*" || (token != "" && len(token) <= maxPermissionCommandToken && isPermissionCommandToken(token)) {
			return raw, true
		}
	}
	return "", false
}

// isPermissionDomainName reports whether name is a DNS name: one or more labels, each a valid DNS label.
func isPermissionDomainName(name string) bool {
	if name == "" || len(name) > maxPermissionPatternLength {
		return false
	}
	for _, label := range strings.Split(name, ".") {
		if !isDNSLabel(label) {
			return false
		}
	}
	return true
}

// isPermissionCommandToken reports whether token uses only the characters execution-core keeps in a command token:
// ASCII letters, digits, and . _ - / +.
func isPermissionCommandToken(token string) bool {
	for i := 0; i < len(token); i++ {
		c := token[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9':
		case strings.IndexByte("._-/+", c) >= 0:
		default:
			return false
		}
	}
	return true
}

// PermissionRulesHandler serves /api/v1/org-permission-rules and /api/v1/org-permission-rules/{id}. It answers 404 while
// CAPABILITY_PERMISSION_RULES_ENABLED is off, the switch the EvaluatePolicy gate reads.
type PermissionRulesHandler struct {
	pool    registryDatabase
	enabled bool
}

// NewPermissionRulesHandler constructs the handler. enabled comes from PermissionRulesEnabledFromLookup; when false
// every route answers 404 and the table is not written.
func NewPermissionRulesHandler(pool registryDatabase, enabled bool) *PermissionRulesHandler {
	return &PermissionRulesHandler{pool: pool, enabled: enabled}
}

// Register mounts the routes on the mux behind the blanket JWT middleware.
func (h *PermissionRulesHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc(orgPermissionRulesPath, func(w http.ResponseWriter, r *http.Request) {
		if !h.enabled {
			http.NotFound(w, r)
			return
		}
		switch r.Method {
		case http.MethodGet:
			h.list(w, r)
		case http.MethodPost:
			h.create(w, r)
		default:
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
	mux.HandleFunc(orgPermissionRulesPath+"/", func(w http.ResponseWriter, r *http.Request) {
		if !h.enabled {
			http.NotFound(w, r)
			return
		}
		id := strings.TrimPrefix(r.URL.Path, orgPermissionRulesPath+"/")
		if id == "" || strings.Contains(id, "/") {
			http.NotFound(w, r)
			return
		}
		if r.Method != http.MethodDelete {
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		h.remove(w, r, id)
	})
}

// permissionRuleAdminListSQL reads an org's rules, oldest first. The admin list shows at most the bound a write holds an
// org to.
const permissionRuleAdminListSQL = `
SELECT rule_id::text, tool_pattern, target_kind, target_pattern, decision, created_by, created_at
FROM org_permission_rules
WHERE org_id = $1
ORDER BY created_at ASC, rule_id ASC
LIMIT $2
`

// permissionRuleRevisionSQL reads the org's policy revision. An org with no policy row is at no revision.
const permissionRuleRevisionSQL = `SELECT revision FROM org_agent_policy WHERE org_id = $1`

// permissionRuleDuplicateSQL reports whether an org already holds a rule with this tool, target and decision. It runs
// only after a create the statement refused, to tell a duplicate from a full org.
const permissionRuleDuplicateSQL = `
SELECT EXISTS (
    SELECT 1 FROM org_permission_rules
    WHERE org_id = $1::text AND tool_pattern = $2::text AND target_kind = $3::text
      AND target_pattern = $4::text AND decision = $5::text
)
`

// permissionRuleCreateSQL inserts a rule while the org holds fewer than the bound and no rule is identical, then bumps
// the policy revision, appends the history row and the audit row, and records the new revision on the rule. An org at
// its bound or holding the rule returns no row. Parameters: $1 org, $2 actor, $3 rule id, $4 history id, $5 audit id,
// $6 default document, $7 default digest, $8 the rule bound, $9 tool pattern, $10 target kind, $11 target pattern,
// $12 decision.
const permissionRuleCreateSQL = `
WITH room AS (
    SELECT 1
    WHERE (SELECT count(*) FROM org_permission_rules WHERE org_id = $1::text) < $8::bigint
      AND NOT EXISTS (
          SELECT 1 FROM org_permission_rules
          WHERE org_id = $1::text AND tool_pattern = $9::text AND target_kind = $10::text
            AND target_pattern = $11::text AND decision = $12::text
      )
), bump AS (
    INSERT INTO org_agent_policy (org_id, revision, doc, digest, updated_by)
    SELECT $1::text, 1, $6::jsonb, $7::text, $2::text FROM room
    ON CONFLICT (org_id) DO UPDATE
        SET revision = org_agent_policy.revision + 1, updated_by = EXCLUDED.updated_by, updated_at = now()
    RETURNING revision, doc, digest
), changed AS (
    INSERT INTO org_permission_rules (rule_id, org_id, tool_pattern, target_kind, target_pattern, decision, created_by, revision)
    SELECT $3::uuid, $1::text, $9::text, $10::text, $11::text, $12::text, $2::text, bump.revision FROM bump
    RETURNING rule_id, created_at
), hist AS (
    INSERT INTO org_agent_policy_history (id, org_id, revision, doc, digest, actor)
    SELECT $4::text, $1::text, bump.revision, bump.doc, bump.digest, $2::text FROM bump
    RETURNING revision
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $5::text, 'org_permission_rules', $3::uuid::text, 'org_permission_rule_created', $2::text, $1::text,
           jsonb_build_object('op', 'create', 'rule_id', $3::uuid::text, 'decision', $12::text,
                              'target_kind', $10::text, 'revision', bump.revision)
    FROM bump
    RETURNING 1
)
SELECT changed.rule_id::text, changed.created_at, bump.revision FROM changed, bump
`

// permissionRuleDeleteSQL removes an org's rule, then bumps the policy revision and appends the history and audit rows,
// as one statement. A rule the org does not hold returns no row. Parameters: $1 org, $2 actor, $3 rule id, $4 history
// id, $5 audit id, $6 default document, $7 default digest.
const permissionRuleDeleteSQL = `
WITH changed AS (
    DELETE FROM org_permission_rules
    WHERE rule_id = $3::uuid AND org_id = $1::text
    RETURNING rule_id, target_kind, decision
), bump AS (
    INSERT INTO org_agent_policy (org_id, revision, doc, digest, updated_by)
    SELECT $1::text, 1, $6::jsonb, $7::text, $2::text FROM changed
    ON CONFLICT (org_id) DO UPDATE
        SET revision = org_agent_policy.revision + 1, updated_by = EXCLUDED.updated_by, updated_at = now()
    RETURNING revision, doc, digest
), hist AS (
    INSERT INTO org_agent_policy_history (id, org_id, revision, doc, digest, actor)
    SELECT $4::text, $1::text, bump.revision, bump.doc, bump.digest, $2::text FROM bump
    RETURNING revision
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $5::text, 'org_permission_rules', $3::uuid::text, 'org_permission_rule_deleted', $2::text, $1::text,
           jsonb_build_object('op', 'delete', 'rule_id', $3::uuid::text, 'decision', changed.decision,
                              'target_kind', changed.target_kind, 'revision', bump.revision)
    FROM changed, bump
    RETURNING 1
)
SELECT changed.rule_id::text, bump.revision FROM changed, bump
`

// permissionRuleWriter answers the refusals every rule write needs, before the body is read, and returns the verified
// admin. The claims are the org hook rules' (orgHookWriter): a person, the write scope, the admin scope and a non-ZDR
// posture, all from the signed principal.
func permissionRuleWriter(w http.ResponseWriter, r *http.Request) (authctx.Principal, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return principal, false
	}
	if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		jsonErr(w, "org_permission_rule_write_under_zdr_refused", http.StatusForbidden)
		return principal, false
	}
	if !principal.HasScope(authz.WriteScope) {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return principal, false
	}
	if principal.PrincipalType != "user" {
		jsonErr(w, "only a person may write org permission rules", http.StatusForbidden)
		return principal, false
	}
	if !verifiedIsAdmin(r) {
		jsonErr(w, "only an admin may write org permission rules", http.StatusForbidden)
		return principal, false
	}
	return principal, true
}

// permissionRuleWriteArgs are the parameters every rule write shares, numbered as the statements use them: the org, the
// actor, the rule id, the history and audit ids, and the default policy document and digest that an org with no policy
// row takes at revision 1.
func permissionRuleWriteArgs(orgID, actor, ruleID string) ([]any, error) {
	digest, doc, err := orgAgentPolicyDigest(orgAgentPolicyDefaults())
	if err != nil {
		return nil, err
	}
	return []any{orgID, actor, ruleID, "oaph_" + uuid.NewString(), "ral_" + uuid.NewString(), string(doc), digest}, nil
}

// decodePermissionRule reads and validates a create body.
func decodePermissionRule(w http.ResponseWriter, r *http.Request) (permissionRule, bool) {
	r.Body = http.MaxBytesReader(w, r.Body, maxPermissionRuleBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var body permissionRuleBody
	if err := decoder.Decode(&body); err != nil || ensureJSONEOF(decoder) != nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return permissionRule{}, false
	}
	rule, refusal := validatePermissionRule(body)
	if refusal != nil {
		jsonErr(w, refusal.code, refusal.status)
		return permissionRule{}, false
	}
	return rule, true
}

// list is GET /api/v1/org-permission-rules: any member of the org reads the rules and the policy revision they are at.
func (h *PermissionRulesHandler) list(w http.ResponseWriter, r *http.Request) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	var revision int64
	if err := h.pool.QueryRow(r.Context(), permissionRuleRevisionSQL, orgID).Scan(&revision); err != nil && !errors.Is(err, pgx.ErrNoRows) {
		slog.Error("read org permission rule revision failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	rows, err := h.pool.Query(r.Context(), permissionRuleAdminListSQL, orgID, maxPermissionRulesPerOrg)
	if err != nil {
		slog.Error("list org permission rules failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	// Initialised, not `var`: an org with no rules must marshal as [] and not null.
	rules := []permissionRuleView{}
	for rows.Next() {
		var rule permissionRuleView
		if err := rows.Scan(&rule.ID, &rule.ToolPattern, &rule.TargetKind, &rule.TargetPattern, &rule.Decision, &rule.CreatedBy, &rule.CreatedAt); err != nil {
			slog.Error("scan org permission rule failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		rules = append(rules, rule)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list org permission rules failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"org_id": orgID, "revision": revision, "rules": rules})
}

// create is POST /api/v1/org-permission-rules: an org admin adds a rule. An org holds at most maxPermissionRulesPerOrg
// rules, and an identical rule is not added twice.
func (h *PermissionRulesHandler) create(w http.ResponseWriter, r *http.Request) {
	principal, ok := permissionRuleWriter(w, r)
	if !ok {
		return
	}
	rule, ok := decodePermissionRule(w, r)
	if !ok {
		return
	}
	ruleID := uuid.NewString()
	args, err := permissionRuleWriteArgs(principal.OrganizationID, principal.ActorID, ruleID)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	args = append(args, maxPermissionRulesPerOrg, rule.toolPattern, rule.targetKind, rule.targetPattern, rule.decision)
	var (
		id        string
		createdAt time.Time
		revision  int64
	)
	err = h.pool.QueryRow(r.Context(), permissionRuleCreateSQL, args...).Scan(&id, &createdAt, &revision)
	if errors.Is(err, pgx.ErrNoRows) {
		h.refuseCreate(w, r, principal.OrganizationID, rule)
		return
	}
	if err != nil {
		slog.Error("write org permission rule failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, permissionRuleWriteView{
		permissionRuleView: permissionRuleView{
			ID:            id,
			ToolPattern:   rule.toolPattern,
			TargetKind:    rule.targetKind,
			TargetPattern: rule.targetPattern,
			Decision:      rule.decision,
			CreatedBy:     principal.ActorID,
			CreatedAt:     createdAt,
		},
		Revision: revision,
	})
}

// refuseCreate answers a create the statement refused, which returned no row: the rule is already in the org, or the org
// is at its bound.
func (h *PermissionRulesHandler) refuseCreate(w http.ResponseWriter, r *http.Request, orgID string, rule permissionRule) {
	var duplicate bool
	if err := h.pool.QueryRow(r.Context(), permissionRuleDuplicateSQL, orgID, rule.toolPattern, rule.targetKind, rule.targetPattern, rule.decision).Scan(&duplicate); err != nil {
		slog.Error("check org permission rule duplicate failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	refusal := permissionRuleRefuseLimit
	if duplicate {
		refusal = permissionRuleRefuseDuplicate
	}
	jsonErr(w, refusal.code, refusal.status)
}

// remove is DELETE /api/v1/org-permission-rules/{id}: an org admin removes a rule. The removal is audited and bumps the
// policy revision.
func (h *PermissionRulesHandler) remove(w http.ResponseWriter, r *http.Request, ruleID string) {
	principal, ok := permissionRuleWriter(w, r)
	if !ok {
		return
	}
	if _, err := uuid.Parse(ruleID); err != nil {
		jsonErr(w, permissionRuleRefuseNotFound.code, permissionRuleRefuseNotFound.status)
		return
	}
	args, err := permissionRuleWriteArgs(principal.OrganizationID, principal.ActorID, ruleID)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	var (
		id       string
		revision int64
	)
	err = h.pool.QueryRow(r.Context(), permissionRuleDeleteSQL, args...).Scan(&id, &revision)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, permissionRuleRefuseNotFound.code, permissionRuleRefuseNotFound.status)
		return
	}
	if err != nil {
		slog.Error("delete org permission rule failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, map[string]any{"id": id, "deleted": true, "revision": revision})
}
