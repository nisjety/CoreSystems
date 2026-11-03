// Package api — the document_pii safety-policy kind (E3.6-8, capability-core half).
//
// capability-core owns the policy that decides whether the text of a parsed document is
// redacted of personal data and what happens when redaction fails. The kind `document_pii` is
// the catalogue key: an org writes one policy row of that kind (its own org_id), the platform
// default is the seeded global row (migration 0032, mode `off`), and Quarry's document pipeline
// reads the effective value. Nothing here redacts anything: a stored value changes no decision
// until Quarry reads it.
//
// The config is a closed schema, so a typo can never become a weaker policy:
//
//	{"mode": "off" | "redact" | "fail_closed", "model_tier": bool}
//
//	off          no redaction stage for parsed documents (today's behaviour)
//	redact       redact, and carry on when a page's redaction errors
//	fail_closed  redact, and a page whose redaction errors is a failed page with a typed
//	             reason, never empty markdown
//
// model_tier asks for the optional `ai_language_pii` model pass after the deterministic stage; it
// needs a mode other than `off`. It is a request, not a grant: the model lane has its own gate
// (MODEL_GATEWAY_AI_PII_SERVICE) and never runs under Zero Data Retention.
//
// The effective value is the org's own enabled row, else the enabled global row, else `off`. A
// stored config that does not parse is read as `fail_closed`: a policy somebody meant to write
// but got wrong must never read as "no policy".
//
// The whole key is behind DOCUMENT_PII_POLICY (off by default): while it is off a document_pii
// policy cannot be created and the effective route answers 404.
package api

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"
	"strings"

	"github.com/jackc/pgx/v5/pgconn"
)

const (
	// SafetyKindDocumentPII is the safety_policies kind of the E3.6-8 catalogue key.
	SafetyKindDocumentPII = "document_pii"

	documentPIIPolicyEnv = "DOCUMENT_PII_POLICY"
	// documentPIIPolicyName is the one name a document_pii row carries, so an org holds one
	// (safety_policies_org_name_uq makes a second a conflict).
	documentPIIPolicyName = "document_pii_default"
	// documentPIIAppliesTo is the only applies_to value of the kind.
	documentPIIAppliesTo = "document"
	// documentPIIEffectiveSuffix is the path below /api/v1/safety/ that answers the effective
	// default for the caller's org.
	documentPIIEffectiveSuffix = "document-pii/effective"

	maxDocumentPIIConfigBytes = 1 << 10
)

// Modes of the document_pii policy.
const (
	DocumentPIIModeOff        = "off"
	DocumentPIIModeRedact     = "redact"
	DocumentPIIModeFailClosed = "fail_closed"
)

// Sources the effective answer names.
const (
	documentPIISourceOrg           = "org"
	documentPIISourceGlobal        = "global"
	documentPIISourceDefault       = "default"
	documentPIISourceOrgInvalid    = "org_invalid"
	documentPIISourceGlobalInvalid = "global_invalid"
)

// DocumentPIIPolicyFromLookup reads DOCUMENT_PII_POLICY strictly: only the literal "true"
// turns the key on, so a typo or a stray "1" leaves it dark.
func DocumentPIIPolicyFromLookup(lookup func(string) string) bool {
	return lookup(documentPIIPolicyEnv) == "true"
}

// DocumentPIIConfig is the closed config of a document_pii policy.
type DocumentPIIConfig struct {
	Mode      string `json:"mode"`
	ModelTier bool   `json:"model_tier"`
}

// ParseDocumentPIIConfig reads a stored or submitted config strictly: unknown keys, a missing
// or unknown mode, trailing data, and a model tier under mode `off` are all errors.
func ParseDocumentPIIConfig(raw []byte) (DocumentPIIConfig, error) {
	if len(raw) == 0 || len(raw) > maxDocumentPIIConfigBytes {
		return DocumentPIIConfig{}, errors.New("config_json must be a small JSON object")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var cfg DocumentPIIConfig
	if err := decoder.Decode(&cfg); err != nil {
		return DocumentPIIConfig{}, errors.New("config_json must be an object with mode and optional model_tier only")
	}
	if decoder.More() {
		return DocumentPIIConfig{}, errors.New("config_json has trailing data")
	}
	switch cfg.Mode {
	case DocumentPIIModeOff, DocumentPIIModeRedact, DocumentPIIModeFailClosed:
	default:
		return DocumentPIIConfig{}, errors.New("mode must be off, redact or fail_closed")
	}
	if cfg.ModelTier && cfg.Mode == DocumentPIIModeOff {
		return DocumentPIIConfig{}, errors.New("model_tier needs a mode other than off")
	}
	return cfg, nil
}

func isDocumentPIIKind(kind string) bool {
	return strings.EqualFold(strings.TrimSpace(kind), SafetyKindDocumentPII)
}

func isUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == "23505"
}

// prepareDocumentPII validates a submitted document_pii policy and normalizes it in place. It
// returns the canonical config JSON, or an HTTP status and message when the request is refused.
// An org writes only its own row: the global default is a migration's.
func (h *SafetyHandler) prepareDocumentPII(p *safetyPolicyRow) ([]byte, int, string) {
	if !h.documentPII {
		return nil, http.StatusNotFound, "document_pii policy is not available"
	}
	if p.OrgID == "" || p.OrgID == "global" {
		return nil, http.StatusForbidden, "a document_pii policy belongs to an organization"
	}
	if name := strings.TrimSpace(p.Name); name != "" && name != documentPIIPolicyName {
		return nil, http.StatusBadRequest, "a document_pii policy is named " + documentPIIPolicyName
	}
	for _, applies := range p.AppliesTo {
		if applies != documentPIIAppliesTo {
			return nil, http.StatusBadRequest, "a document_pii policy applies_to document only"
		}
	}
	raw, err := json.Marshal(p.ConfigJSON)
	if err != nil {
		return nil, http.StatusBadRequest, "config_json is not valid JSON"
	}
	cfg, err := ParseDocumentPIIConfig(raw)
	if err != nil {
		return nil, http.StatusBadRequest, err.Error()
	}
	normalized, err := json.Marshal(cfg)
	if err != nil {
		return nil, http.StatusInternalServerError, "could not encode the policy"
	}
	p.Kind = SafetyKindDocumentPII
	p.Name = documentPIIPolicyName
	p.AppliesTo = []string{documentPIIAppliesTo}
	return normalized, 0, ""
}

// documentPIIEffective is the resolved default for the caller's org.
type documentPIIEffective struct {
	Mode      string `json:"mode"`
	ModelTier bool   `json:"model_tier"`
	Source    string `json:"source"`
}

// effectiveDocumentPII resolves the policy from the document_pii rows visible to orgID (its
// own and the global ones): the org's enabled row, else the enabled global row, else off. A row
// whose config does not parse is read as fail_closed and named in the source.
func effectiveDocumentPII(rows []safetyPolicyRow, orgID string) documentPIIEffective {
	pick := func(owner string) (safetyPolicyRow, bool) {
		for _, row := range rows {
			if row.OrgID == owner && row.Enabled && isDocumentPIIKind(row.Kind) {
				return row, true
			}
		}
		return safetyPolicyRow{}, false
	}
	read := func(row safetyPolicyRow, source, invalidSource string) documentPIIEffective {
		raw, err := json.Marshal(row.ConfigJSON)
		if err == nil {
			if cfg, parseErr := ParseDocumentPIIConfig(raw); parseErr == nil {
				return documentPIIEffective{Mode: cfg.Mode, ModelTier: cfg.ModelTier, Source: source}
			}
		}
		return documentPIIEffective{Mode: DocumentPIIModeFailClosed, Source: invalidSource}
	}
	if orgID != "" && orgID != "global" {
		if row, ok := pick(orgID); ok {
			return read(row, documentPIISourceOrg, documentPIISourceOrgInvalid)
		}
	}
	if row, ok := pick("global"); ok {
		return read(row, documentPIISourceGlobal, documentPIISourceGlobalInvalid)
	}
	return documentPIIEffective{Mode: DocumentPIIModeOff, Source: documentPIISourceDefault}
}

// documentPIIEffective answers GET /api/v1/safety/document-pii/effective.
func (h *SafetyHandler) documentPIIEffective(w http.ResponseWriter, r *http.Request) {
	if !h.documentPII {
		http.NotFound(w, r)
		return
	}
	orgID := verifiedOrganizationID(r)
	if orgID == "" {
		jsonErr(w, "organization is required", http.StatusForbidden)
		return
	}
	rows, err := h.pool.Query(r.Context(), `
		SELECT id, org_id, name, description, kind, config_json, applies_to,
		       priority, enabled, created_at, updated_at
		FROM safety_policies
		WHERE kind=$2 AND (org_id=$1 OR org_id='global') AND deleted_at IS NULL
		ORDER BY priority DESC, name
	`, orgID, SafetyKindDocumentPII)
	if err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	defer rows.Close()
	policies := []safetyPolicyRow{}
	for rows.Next() {
		var p safetyPolicyRow
		if err := rows.Scan(&p.ID, &p.OrgID, &p.Name, &p.Description, &p.Kind,
			&p.ConfigJSON, &p.AppliesTo, &p.Priority, &p.Enabled, &p.CreatedAt, &p.UpdatedAt); err != nil {
			jsonErr(w, "database unavailable", http.StatusInternalServerError)
			return
		}
		policies = append(policies, p)
	}
	if err := rows.Err(); err != nil {
		jsonErr(w, "database unavailable", http.StatusInternalServerError)
		return
	}
	writeJSON(w, effectiveDocumentPII(policies, orgID))
}
