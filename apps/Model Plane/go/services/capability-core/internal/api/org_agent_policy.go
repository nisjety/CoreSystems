// Package api — the org agent policy registry (build plan WS25-S7, DEC-14).
//
// capability-core is the system of record for an org's agent policy: one versioned
// document per org, keyed by the closed schema that mp-contracts (org_agent_policy.rs)
// and api/contracts/org-agent-policy-schema.json pin. An org admin writes the document
// with If-Match on its revision, and every member of the org reads it. Nothing enforces
// it yet: the gateway and the signed gate read it in WS25-S9, so a stored value changes
// no decision today.
//
// Who may write: an org admin (the signed scopes, verifiedIsAdmin), acting as a person,
// with the write scope and an explicit non-ZDR posture. A ZDR principal reads but never
// writes, because a policy is durable state. Every write appends a history row, which
// holds the document (keys and values only, never content), and a registry_audit_log row
// that carries the revision, the digest and the key names written, never a value.
//
// The whole surface is behind ORG_AGENT_POLICY_API: while it is off every route answers
// 404, and the table is not read.
package api

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"math"
	"net/http"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
)

const (
	orgAgentPolicyPath   = "/api/v1/org-agent-policy"
	orgAgentPolicyAPIEnv = "ORG_AGENT_POLICY_API"

	maxOrgAgentPolicyBodyBytes = 16 << 10
	maxOrgAgentPolicyHistory   = 100
	maxOrgAgentAllowListIDs    = 64
)

// OrgAgentPolicyAPIFromLookup reads ORG_AGENT_POLICY_API strictly: only the literal
// "true" turns the API on, so a typo or a stray "1" leaves it dark.
func OrgAgentPolicyAPIFromLookup(lookup func(string) string) bool {
	return lookup(orgAgentPolicyAPIEnv) == "true"
}

// orgAgentSetting is one row of the closed key schema. It mirrors the SETTINGS table of
// mp-contracts/src/org_agent_policy.rs; the golden test below holds the two together.
type orgAgentSetting struct {
	key            string
	kind           string
	order          []string
	def            string
	failClose      string
	reserved       bool
	reservedValues []string
}

var orgAgentPolicySettings = []orgAgentSetting{
	{key: "mode_cap", kind: "enum", order: []string{"lockdown", "read_only", "ask_first", "auto"}, def: "auto", failClose: "lockdown"},
	{key: "rung_cap", kind: "enum", order: []string{"read_only", "workspace_write", "danger_full_access"}, def: "danger_full_access", failClose: "read_only"},
	{key: "lockdown", kind: "bool", order: []string{"true", "false"}, def: "false", failClose: "true"},
	{key: "ext.mcp_servers", kind: "allow_list", order: []string{"none", "approved_only", "any"}, def: "any", failClose: "none"},
	{key: "ext.skills", kind: "allow_list", order: []string{"none", "approved_only", "any"}, def: "any", failClose: "none", reserved: true},
	{key: "ext.plugins", kind: "allow_list", order: []string{"none", "approved_only", "any"}, def: "any", failClose: "none", reserved: true},
	{key: "mcp_tool_approval", kind: "enum", order: []string{"approved_only", "trust_annotations"}, def: "trust_annotations", failClose: "approved_only"},
	{key: "user_hooks", kind: "enum", order: []string{"denied", "allowed"}, def: "allowed", failClose: "denied", reserved: true},
	{key: "reference_uploads", kind: "enum", order: []string{"off", "on"}, def: "off", failClose: "off", reserved: true},
	// Wave 3 EP-2 S8 (D5): the wide-research item cap, a whole number from 0 (wide research off)
	// to 16. The gateway enforces it when a wide-research run starts.
	{key: "research.max_fan_out_items", kind: "integer", order: orgAgentCountOrder(16), def: "8", failClose: "0"},
	// Wave 3 EP-9 W2: the org's web switch. `off` withholds public web access from both agent loops;
	// `cached_only` is reserved (refused not_enforced) until Quarry can answer from its cache alone.
	{key: "web.mode", kind: "enum", order: []string{"off", "cached_only", "live"}, def: "live", failClose: "off", reservedValues: []string{"cached_only"}},
	// Wave 3 EP-9 W2: whether a paid search provider may serve the org's agents.
	{key: "web.paid_providers", kind: "enum", order: []string{"off", "on"}, def: "on", failClose: "off"},
	// Wave 4 W4-D M1a: the origins an MCP App may reach. `approved_only` names origins; it is reserved
	// until MCP_APPS_NETWORK_ENABLED serves them, so a write of it is refused not_enforced.
	{key: "mcp_apps.network_origins", kind: "origin_allow_list", order: []string{"none", "approved_only"}, def: "none", failClose: "none", reserved: true},
}

// orgAgentCountOrder is the strictness order of an integer key: "0" to max, smallest first,
// so a count is tighten-only by the same rank as every other kind.
func orgAgentCountOrder(ceiling int) []string {
	order := make([]string, 0, ceiling+1)
	for count := 0; count <= ceiling; count++ {
		order = append(order, strconv.Itoa(count))
	}
	return order
}

// orgAgentRefusal is one closed refusal code and the status it answers with.
type orgAgentRefusal struct {
	code   string
	status int
}

var (
	orgAgentRefuseUnknownKey     = orgAgentRefusal{"unknown_key", http.StatusBadRequest}
	orgAgentRefuseInvalidValue   = orgAgentRefusal{"invalid_value", http.StatusBadRequest}
	orgAgentRefuseNotEnforced    = orgAgentRefusal{"not_enforced", http.StatusUnprocessableEntity}
	orgAgentRefuseExceedsCeiling = orgAgentRefusal{"exceeds_platform_ceiling", http.StatusUnprocessableEntity}
	orgAgentRefuseStaleRevision  = orgAgentRefusal{"stale_revision", http.StatusConflict}
)

// loosestOrgAgentCeiling is the platform ceiling of this build: the loosest value of
// every key, so the platform bounds nothing beyond the schema. A tighter ceiling is a
// platform decision that has no source yet (see the WS25-S7 depth findings).
func loosestOrgAgentCeiling() map[string]string {
	ceiling := make(map[string]string, len(orgAgentPolicySettings))
	for _, def := range orgAgentPolicySettings {
		ceiling[def.key] = def.order[len(def.order)-1]
	}
	return ceiling
}

// orgAgentRank is the position of token in the schema order: lower is stricter. A token
// outside the order ranks loosest of all and can never pass a ceiling.
func orgAgentRank(def orgAgentSetting, token string) int {
	if index := slices.Index(def.order, token); index >= 0 {
		return index
	}
	return len(def.order)
}

// orgAgentDefaultValue is the canonical value of a token already known to be in the schema.
func orgAgentDefaultValue(def orgAgentSetting, token string) any {
	switch def.kind {
	case "bool":
		return token == "true"
	case "allow_list", "origin_allow_list":
		return map[string]any{"mode": token, "ids": []string{}}
	case "integer":
		// A token in the order is always a decimal count, so the parse cannot fail.
		count, _ := strconv.Atoi(token)
		return count
	default:
		return token
	}
}

// orgAgentPolicyDefaults is the document an org with no row has: every key at its default.
func orgAgentPolicyDefaults() map[string]any {
	settings := make(map[string]any, len(orgAgentPolicySettings))
	for _, def := range orgAgentPolicySettings {
		settings[def.key] = orgAgentDefaultValue(def, def.def)
	}
	return settings
}

// validateOrgAgentSettings checks a written document and returns the complete document
// in force, a missing key taking its default. Unknown keys are refused before any other
// check, and keys are then checked in schema order, so the same input always gets the
// same refusal.
func validateOrgAgentSettings(written map[string]any, ceiling map[string]string) (map[string]any, *orgAgentRefusal) {
	for key := range written {
		if !slices.ContainsFunc(orgAgentPolicySettings, func(def orgAgentSetting) bool { return def.key == key }) {
			return nil, &orgAgentRefuseUnknownKey
		}
	}
	settings := make(map[string]any, len(orgAgentPolicySettings))
	for _, def := range orgAgentPolicySettings {
		token, value := def.def, orgAgentDefaultValue(def, def.def)
		if raw, ok := written[def.key]; ok {
			var refusal *orgAgentRefusal
			if token, value, refusal = parseOrgAgentValue(def, raw); refusal != nil {
				return nil, refusal
			}
		}
		if def.reserved && token != def.def {
			return nil, &orgAgentRefuseNotEnforced
		}
		if slices.Contains(def.reservedValues, token) {
			return nil, &orgAgentRefuseNotEnforced
		}
		bound, ok := ceiling[def.key]
		if !ok {
			bound = def.order[len(def.order)-1]
		}
		if orgAgentRank(def, token) > orgAgentRank(def, bound) {
			return nil, &orgAgentRefuseExceedsCeiling
		}
		settings[def.key] = value
	}
	return settings, nil
}

// parseOrgAgentValue reads one written value: its token in the schema order and its
// canonical form. An allow-list's ids are checked, de-duplicated and sorted.
func parseOrgAgentValue(def orgAgentSetting, raw any) (string, any, *orgAgentRefusal) {
	switch def.kind {
	case "enum":
		text, ok := raw.(string)
		if !ok || !slices.Contains(def.order, text) {
			return "", nil, &orgAgentRefuseInvalidValue
		}
		return text, text, nil
	case "bool":
		flag, ok := raw.(bool)
		if !ok {
			return "", nil, &orgAgentRefuseInvalidValue
		}
		if flag {
			return "true", true, nil
		}
		return "false", false, nil
	case "allow_list", "origin_allow_list":
		return parseOrgAgentAllowList(def, raw)
	case "integer":
		// encoding/json decodes every number as float64: a fraction, a negative or a quoted
		// number is refused rather than rounded or read as text.
		number, ok := raw.(float64)
		if !ok || number != math.Trunc(number) || number < 0 || number > math.MaxInt32 {
			return "", nil, &orgAgentRefuseInvalidValue
		}
		token := strconv.FormatInt(int64(number), 10)
		if !slices.Contains(def.order, token) {
			return "", nil, &orgAgentRefuseInvalidValue
		}
		return token, int(number), nil
	}
	return "", nil, &orgAgentRefuseInvalidValue
}

func parseOrgAgentAllowList(def orgAgentSetting, raw any) (string, any, *orgAgentRefusal) {
	object, ok := raw.(map[string]any)
	if !ok || len(object) != 2 {
		return "", nil, &orgAgentRefuseInvalidValue
	}
	mode, modeOK := object["mode"].(string)
	rawIDs, idsOK := object["ids"].([]any)
	if !modeOK || !idsOK || !slices.Contains(def.order, mode) {
		return "", nil, &orgAgentRefuseInvalidValue
	}
	if len(rawIDs) > 0 && mode != "approved_only" {
		return "", nil, &orgAgentRefuseInvalidValue
	}
	seen := make(map[string]bool, len(rawIDs))
	ids := []string{}
	for _, item := range rawIDs {
		id, ok := item.(string)
		if !ok || !orgAgentIDWellFormed(def.kind, id) {
			return "", nil, &orgAgentRefuseInvalidValue
		}
		if !seen[id] {
			seen[id] = true
			ids = append(ids, id)
		}
	}
	if len(ids) > maxOrgAgentAllowListIDs {
		return "", nil, &orgAgentRefuseInvalidValue
	}
	sort.Strings(ids)
	return mode, map[string]any{"mode": mode, "ids": ids}, nil
}

// orgAgentIDWellFormed is the shape an allow-list id must have for its kind: an MCP server id, or a
// canonical https origin.
func orgAgentIDWellFormed(kind, id string) bool {
	if kind == "origin_allow_list" {
		return isHTTPSOrigin(id)
	}
	return mcpIdentifier.MatchString(id)
}

// isHTTPSOrigin mirrors mp-contracts' is_https_origin: one canonical https origin, lowercase host
// labels and an optional canonical port. The two must accept and refuse the same strings.
func isHTTPSOrigin(value string) bool {
	rest, ok := strings.CutPrefix(value, "https://")
	if !ok || len(value) > 200 {
		return false
	}
	host, port, hasPort := rest, "", false
	if i := strings.LastIndex(rest, ":"); i >= 0 {
		host, port, hasPort = rest[:i], rest[i+1:], true
	}
	if host == "" || len(host) > 253 {
		return false
	}
	for _, label := range strings.Split(host, ".") {
		if !isDNSLabel(label) {
			return false
		}
	}
	return !hasPort || isCanonicalPort(port)
}

func isDNSLabel(label string) bool {
	if label == "" || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
		return false
	}
	for i := 0; i < len(label); i++ {
		c := label[i]
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
			return false
		}
	}
	return true
}

func isCanonicalPort(port string) bool {
	if port == "" || len(port) > 5 || port[0] == '0' {
		return false
	}
	for i := 0; i < len(port); i++ {
		if port[i] < '0' || port[i] > '9' {
			return false
		}
	}
	number, err := strconv.Atoi(port)
	return err == nil && number <= 65535
}

// orgAgentPolicyDigest is the sha256 of the canonical document: encoding/json sorts map
// keys, so the same document always has the same digest.
func orgAgentPolicyDigest(settings map[string]any) (string, []byte, error) {
	canonical, err := json.Marshal(settings)
	if err != nil {
		return "", nil, fmt.Errorf("encode org agent policy: %w", err)
	}
	sum := sha256.Sum256(canonical)
	return hex.EncodeToString(sum[:]), canonical, nil
}

// OrgAgentPolicyHandler serves /api/v1/org-agent-policy and its effective and history reads.
type OrgAgentPolicyHandler struct {
	pool    registryDatabase
	enabled bool
	ceiling map[string]string
}

// NewOrgAgentPolicyHandler constructs the handler. enabled comes from
// OrgAgentPolicyAPIFromLookup; when false every route answers 404.
func NewOrgAgentPolicyHandler(pool registryDatabase, enabled bool) *OrgAgentPolicyHandler {
	return &OrgAgentPolicyHandler{pool: pool, enabled: enabled, ceiling: loosestOrgAgentCeiling()}
}

// Register mounts the routes on the mux behind the blanket JWT middleware.
func (h *OrgAgentPolicyHandler) Register(mux *http.ServeMux) {
	mux.HandleFunc(orgAgentPolicyPath, func(w http.ResponseWriter, r *http.Request) {
		if !h.enabled {
			http.NotFound(w, r)
			return
		}
		switch r.Method {
		case http.MethodGet:
			h.get(w, r)
		case http.MethodPut:
			h.put(w, r)
		default:
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
		}
	})
	mux.HandleFunc(orgAgentPolicyPath+"/effective", func(w http.ResponseWriter, r *http.Request) {
		if !h.enabled {
			http.NotFound(w, r)
			return
		}
		if r.Method != http.MethodGet {
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		h.effective(w, r)
	})
	mux.HandleFunc(orgAgentPolicyPath+"/history", func(w http.ResponseWriter, r *http.Request) {
		if !h.enabled {
			http.NotFound(w, r)
			return
		}
		if r.Method != http.MethodGet {
			jsonErr(w, "method not allowed", http.StatusMethodNotAllowed)
			return
		}
		h.history(w, r)
	})
}

// orgAgentPolicyRow is the org's stored document. An org with no row is the defaults at
// revision 0, with no updated_at.
type orgAgentPolicyRow struct {
	OrgID     string
	Revision  int64
	Settings  map[string]any
	Digest    string
	UpdatedBy string
	UpdatedAt *time.Time
}

type orgAgentPolicyView struct {
	OrgID     string         `json:"org_id"`
	Revision  int64          `json:"revision"`
	Digest    string         `json:"digest"`
	Settings  map[string]any `json:"settings"`
	UpdatedBy string         `json:"updated_by"`
	UpdatedAt *time.Time     `json:"updated_at"`
}

const orgAgentPolicyReadSQL = `
SELECT revision, doc::text, digest, updated_by, updated_at
FROM org_agent_policy
WHERE org_id = $1
`

func (h *OrgAgentPolicyHandler) load(ctx context.Context, orgID string) (orgAgentPolicyRow, error) {
	var (
		row orgAgentPolicyRow
		doc string
	)
	err := h.pool.QueryRow(ctx, orgAgentPolicyReadSQL, orgID).Scan(&row.Revision, &doc, &row.Digest, &row.UpdatedBy, &row.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		settings := orgAgentPolicyDefaults()
		digest, _, digestErr := orgAgentPolicyDigest(settings)
		if digestErr != nil {
			return orgAgentPolicyRow{}, digestErr
		}
		return orgAgentPolicyRow{OrgID: orgID, Settings: settings, Digest: digest}, nil
	}
	if err != nil {
		return orgAgentPolicyRow{}, fmt.Errorf("read org agent policy: %w", err)
	}
	row.OrgID = orgID
	if err := json.Unmarshal([]byte(doc), &row.Settings); err != nil {
		return orgAgentPolicyRow{}, fmt.Errorf("stored org agent policy document: %w", err)
	}
	return row, nil
}

// get is GET /api/v1/org-agent-policy: any member of the org reads the document in force.
func (h *OrgAgentPolicyHandler) get(w http.ResponseWriter, r *http.Request) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	row, err := h.load(r.Context(), orgID)
	if err != nil {
		slog.Error("read org agent policy failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, orgAgentPolicyView{OrgID: row.OrgID, Revision: row.Revision, Digest: row.Digest, Settings: row.Settings, UpdatedBy: row.UpdatedBy, UpdatedAt: row.UpdatedAt})
}

// orgAgentPolicyWriter answers the refusals every write needs, before the body is read,
// and returns the verified admin.
func orgAgentPolicyWriter(w http.ResponseWriter, r *http.Request) (authctx.Principal, bool) {
	principal, ok := authctx.PrincipalFromContext(r.Context())
	if !ok || principal.OrganizationID == "" || principal.ActorID == "" {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return principal, false
	}
	if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		jsonErr(w, "org_agent_policy_write_under_zdr_refused", http.StatusForbidden)
		return principal, false
	}
	if !principal.HasScope(authz.WriteScope) {
		jsonErr(w, "capability write scope required", http.StatusForbidden)
		return principal, false
	}
	if principal.PrincipalType != "user" {
		jsonErr(w, "only a person may set the org agent policy", http.StatusForbidden)
		return principal, false
	}
	if !verifiedIsAdmin(r) {
		jsonErr(w, "only an admin may set the org agent policy", http.StatusForbidden)
		return principal, false
	}
	return principal, true
}

// orgAgentIfMatch reads the revision the write was made against. A missing If-Match is
// refused, so a write can never overwrite a revision its author did not see.
func orgAgentIfMatch(w http.ResponseWriter, r *http.Request) (int64, bool) {
	raw := strings.TrimSpace(r.Header.Get("If-Match"))
	if raw == "" {
		jsonErr(w, "if_match_required", http.StatusPreconditionRequired)
		return 0, false
	}
	revision, err := strconv.ParseInt(strings.Trim(strings.TrimPrefix(raw, "W/"), `"`), 10, 64)
	if err != nil || revision < 0 {
		jsonErr(w, "invalid_if_match", http.StatusBadRequest)
		return 0, false
	}
	return revision, true
}

// orgAgentPolicyWriteSQL replaces the org's document at the expected revision, or
// creates revision 1 when the org has no row and the expected revision is 0, and appends
// the history and audit rows, as one statement. Parameters: $1 org, $2 expected revision,
// $3 document, $4 digest, $5 actor, $6 history id, $7 audit id, $8 audit diff (without
// the revision, which this statement adds). No row back means the revision was stale.
const orgAgentPolicyWriteSQL = `
WITH cur AS (
    UPDATE org_agent_policy
    SET revision = revision + 1, doc = $3::jsonb, digest = $4::text, updated_by = $5::text, updated_at = now()
    WHERE org_id = $1::text AND revision = $2::bigint
    RETURNING revision
), ins AS (
    INSERT INTO org_agent_policy (org_id, revision, doc, digest, updated_by)
    SELECT $1::text, 1, $3::jsonb, $4::text, $5::text
    WHERE $2::bigint = 0 AND NOT EXISTS (SELECT 1 FROM org_agent_policy WHERE org_id = $1::text)
    RETURNING revision
), newrev AS (
    SELECT revision FROM cur UNION ALL SELECT revision FROM ins
), hist AS (
    INSERT INTO org_agent_policy_history (id, org_id, revision, doc, digest, actor)
    SELECT $6::text, $1::text, newrev.revision, $3::jsonb, $4::text, $5::text FROM newrev
    RETURNING revision
), aud AS (
    INSERT INTO registry_audit_log (id, entity_kind, entity_id, action, actor, org_id, diff_json)
    SELECT $7::text, 'org_agent_policy', $1::text, 'org_agent_policy_changed', $5::text, $1::text,
           $8::jsonb || jsonb_build_object('revision', newrev.revision)
    FROM newrev
    RETURNING 1
)
SELECT revision FROM newrev
`

// put is PUT /api/v1/org-agent-policy: an org admin replaces the document at the revision
// the If-Match header names. The body is {"settings": {...}}; a missing key takes its default.
func (h *OrgAgentPolicyHandler) put(w http.ResponseWriter, r *http.Request) {
	principal, ok := orgAgentPolicyWriter(w, r)
	if !ok {
		return
	}
	expected, ok := orgAgentIfMatch(w, r)
	if !ok {
		return
	}
	r.Body = http.MaxBytesReader(w, r.Body, maxOrgAgentPolicyBodyBytes)
	decoder := json.NewDecoder(r.Body)
	decoder.DisallowUnknownFields()
	var request struct {
		Settings map[string]any `json:"settings"`
	}
	if err := decoder.Decode(&request); err != nil || ensureJSONEOF(decoder) != nil || request.Settings == nil {
		jsonErr(w, "invalid request body", http.StatusBadRequest)
		return
	}
	settings, refusal := validateOrgAgentSettings(request.Settings, h.ceiling)
	if refusal != nil {
		jsonErr(w, refusal.code, refusal.status)
		return
	}
	digest, doc, err := orgAgentPolicyDigest(settings)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	keys := make([]string, 0, len(settings))
	for key := range settings {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	auditDiff, err := json.Marshal(map[string]any{"digest": digest, "keys": keys})
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	var revision int64
	err = h.pool.QueryRow(r.Context(), orgAgentPolicyWriteSQL,
		principal.OrganizationID, expected, string(doc), digest, principal.ActorID,
		"oaph_"+uuid.NewString(), "ral_"+uuid.NewString(), string(auditDiff),
	).Scan(&revision)
	if errors.Is(err, pgx.ErrNoRows) {
		jsonErr(w, orgAgentRefuseStaleRevision.code, orgAgentRefuseStaleRevision.status)
		return
	}
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation {
		// Two first writes raced; the other one took revision 1.
		jsonErr(w, orgAgentRefuseStaleRevision.code, orgAgentRefuseStaleRevision.status)
		return
	}
	if err != nil {
		slog.Error("write org agent policy failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, orgAgentPolicyView{OrgID: principal.OrganizationID, Revision: revision, Digest: digest, Settings: settings, UpdatedBy: principal.ActorID})
}

type orgAgentEffectiveSetting struct {
	Key             string `json:"key"`
	Kind            string `json:"kind"`
	Status          string `json:"status"`
	Value           any    `json:"value"`
	Source          string `json:"source"`
	FailClosedValue any    `json:"fail_closed_value"`
}

// effective is GET /api/v1/org-agent-policy/effective?user=me: each key with the value in
// force and the layer that set it. Only the org layer exists yet; a person's own layer
// joins in S9.
func (h *OrgAgentPolicyHandler) effective(w http.ResponseWriter, r *http.Request) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	if user := r.URL.Query().Get("user"); user != "" && user != "me" {
		jsonErr(w, "user must be me", http.StatusBadRequest)
		return
	}
	row, err := h.load(r.Context(), orgID)
	if err != nil {
		slog.Error("read effective org agent policy failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	settings := make([]orgAgentEffectiveSetting, 0, len(orgAgentPolicySettings))
	for _, def := range orgAgentPolicySettings {
		value, stored := row.Settings[def.key]
		if !stored {
			// A document written before the key existed holds the key's default.
			value = orgAgentDefaultValue(def, def.def)
		}
		source := "platform_default"
		if row.Revision > 0 && orgAgentTokenOf(value) != def.def {
			source = "org_policy"
		}
		status := "enforced"
		if def.reserved {
			status = "reserved"
		}
		settings = append(settings, orgAgentEffectiveSetting{
			Key: def.key, Kind: def.kind, Status: status, Value: value, Source: source,
			FailClosedValue: orgAgentDefaultValue(def, def.failClose),
		})
	}
	writeJSON(w, map[string]any{"org_id": row.OrgID, "revision": row.Revision, "digest": row.Digest, "settings": settings})
}

// orgAgentTokenOf is the schema token of a stored value, the inverse of orgAgentDefaultValue.
func orgAgentTokenOf(value any) string {
	switch typed := value.(type) {
	case bool:
		if typed {
			return "true"
		}
		return "false"
	case string:
		return typed
	case map[string]any:
		mode, _ := typed["mode"].(string)
		return mode
	case float64:
		// A stored integer key, read back from the JSONB document.
		return strconv.FormatInt(int64(typed), 10)
	case int:
		// An integer key's default, before it was ever stored.
		return strconv.Itoa(typed)
	}
	return ""
}

type orgAgentHistoryEntry struct {
	Revision  int64          `json:"revision"`
	Digest    string         `json:"digest"`
	Actor     string         `json:"actor"`
	CreatedAt time.Time      `json:"created_at"`
	Settings  map[string]any `json:"settings"`
}

const orgAgentPolicyHistorySQL = `
SELECT revision, digest, actor, created_at, doc::text
FROM org_agent_policy_history
WHERE org_id = $1
ORDER BY revision DESC
LIMIT $2
`

// history is GET /api/v1/org-agent-policy/history: the revision chain, newest first.
func (h *OrgAgentPolicyHandler) history(w http.ResponseWriter, r *http.Request) {
	orgID, ok := mcpVerifiedOrganization(r, false)
	if !ok {
		jsonErr(w, "authentication required", http.StatusUnauthorized)
		return
	}
	rows, err := h.pool.Query(r.Context(), orgAgentPolicyHistorySQL, orgID, maxOrgAgentPolicyHistory)
	if err != nil {
		slog.Error("list org agent policy history failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	// Initialised, not `var`: an org with no history must marshal as [] and not null.
	entries := []orgAgentHistoryEntry{}
	for rows.Next() {
		var (
			entry orgAgentHistoryEntry
			doc   string
		)
		if err := rows.Scan(&entry.Revision, &entry.Digest, &entry.Actor, &entry.CreatedAt, &doc); err != nil {
			slog.Error("scan org agent policy history failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		if err := json.Unmarshal([]byte(doc), &entry.Settings); err != nil {
			slog.Error("stored org agent policy history document failed", "error", err)
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		entries = append(entries, entry)
	}
	if err := rows.Err(); err != nil {
		slog.Error("list org agent policy history failed", "error", err)
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, entries)
}
