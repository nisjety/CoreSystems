package spaces

import (
	"bytes"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"regexp"
	"sort"
	"strings"
)

// spaceSelfAPIRegistryJSON is the vendored, byte-identical copy of the golden
// apps/Model Plane/api/contracts/space-self-api-registry.json. The repository
// sync check pins the golden and every copy. This package decodes the copy
// strictly and refuses any row it cannot check alone; a malformed copy yields no
// rows, so every lookup fails closed.
//
//go:embed registry/space-self-api-registry.json
var spaceSelfAPIRegistryJSON []byte

const (
	spaceSelfAPIRegistryContract  = "space-self-api-registry"
	spaceSelfAPIRegistryVersion   = 1
	spaceSelfAPIActionsEnv        = "CONTROL_SPACE_SELF_API_ACTIONS"
	runActionDecisionKind         = "run_action"
	runActionScheduleDecisionKind = "schedule_create"
)

var (
	runActionActionIDPattern = regexp.MustCompile(`^space\.[a-z_]+\.[a-z_]+$`)
	// runActionForbiddenNameParts are substrings no action id tail or argument
	// name may carry: membership, roles, deletion, credentials, grants, policy,
	// and any model-supplied thread, Space, org, user or actor identity.
	runActionForbiddenNameParts = []string{
		"member", "role", "delete", "credential", "grant", "policy",
		"thread_id", "space", "org", "user", "actor",
	}
)

// runActionSpecRow is one row of the golden registry. The field tags mirror the
// JSON contract exactly; DisallowUnknownFields keeps the copy and the golden in
// step.
type runActionSpecRow struct {
	ActionID        string   `json:"action_id"`
	CapabilityID    string   `json:"capability_id"`
	OwnerPlane      string   `json:"owner_plane"`
	OwnerService    string   `json:"owner_service"`
	ServiceAudience string   `json:"service_audience"`
	DecisionType    string   `json:"decision_type"`
	Permission      string   `json:"permission"`
	MinRole         string   `json:"min_role"`
	Risk            string   `json:"risk"`
	DefaultLevel    string   `json:"default_level"`
	Approval        string   `json:"approval"`
	AutonomyRung    string   `json:"autonomy_rung"`
	Reversible      bool     `json:"reversible"`
	MaxPerRun       int      `json:"max_per_run"`
	Reservation     bool     `json:"reservation"`
	ArgumentOrder   []string `json:"argument_order"`
	ParametersJSON  string   `json:"parameters_json"`
	SchemaSHA256    string   `json:"schema_sha256"`
	HashBasis       string   `json:"hash_basis"`
	ZDR             string   `json:"zdr"`
	UIActionID      *string  `json:"ui_action_id"`
	Phase           int      `json:"phase"`
}

type runActionRegistryDocument struct {
	Contract      string             `json:"contract"`
	Version       int                `json:"version"`
	Rows          []runActionSpecRow `json:"rows"`
	DigestVectors json.RawMessage    `json:"digest_vectors"`
}

// runActionRegistry and runActionRegistryErr are set once at package init and
// never change afterwards.
var runActionRegistry, runActionRegistryErr = loadRunActionRegistry(spaceSelfAPIRegistryJSON)

// loadRunActionRegistry decodes the golden as one strict JSON document and
// validates every row it can check without the sibling copies.
func loadRunActionRegistry(raw []byte) (map[string]runActionSpecRow, error) {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var doc runActionRegistryDocument
	if err := decoder.Decode(&doc); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return nil, fmt.Errorf("space self-api registry is not one strict JSON document")
	}
	if doc.Contract != spaceSelfAPIRegistryContract || doc.Version != spaceSelfAPIRegistryVersion || len(doc.DigestVectors) == 0 {
		return nil, fmt.Errorf("space self-api registry header is invalid")
	}
	rows := make(map[string]runActionSpecRow, len(doc.Rows))
	for _, row := range doc.Rows {
		if err := validateRunActionSpecRow(row); err != nil {
			return nil, err
		}
		if _, repeated := rows[row.ActionID]; repeated {
			return nil, fmt.Errorf("space self-api registry repeats an action id")
		}
		rows[row.ActionID] = row
	}
	return rows, nil
}

func validateRunActionSpecRow(row runActionSpecRow) error {
	id := row.ActionID
	if id != ticketsCreateActionID && !runActionActionIDPattern.MatchString(id) {
		return fmt.Errorf("space self-api registry action id is invalid")
	}
	if runActionNameIsForbidden(strings.TrimPrefix(id, "space.")) {
		return fmt.Errorf("space self-api registry action id names a forbidden part")
	}
	if row.CapabilityID == "" || row.OwnerPlane == "" || row.OwnerService == "" || row.ServiceAudience == "" ||
		row.Permission == "" || row.Risk == "" || row.AutonomyRung == "" {
		return fmt.Errorf("space self-api registry row %q is incomplete", id)
	}
	if row.DecisionType != runActionDecisionKind && row.DecisionType != runActionScheduleDecisionKind {
		return fmt.Errorf("space self-api registry row %q has an unknown decision type", id)
	}
	if RoleRank(row.MinRole) == 0 || (row.DefaultLevel != "allow" && row.DefaultLevel != "ask") ||
		(row.Approval != "none" && row.Approval != "owner_approval") || row.ZDR != "refuse" {
		return fmt.Errorf("space self-api registry row %q has an invalid policy field", id)
	}
	if row.Phase < 0 || row.Phase > 3 || (row.Phase > 0 && row.MaxPerRun < 1) {
		return fmt.Errorf("space self-api registry row %q has an invalid phase or cap", id)
	}
	// The owner reservation table (migration 025) admits tickets.create only.
	if row.Reservation && id != ticketsCreateActionID {
		return fmt.Errorf("space self-api registry row %q may not reserve an owner effect", id)
	}
	if row.UIActionID != nil && strings.TrimSpace(*row.UIActionID) == "" {
		return fmt.Errorf("space self-api registry row %q has an empty ui action id", id)
	}
	if !validSHA256Commitment(row.SchemaSHA256) {
		return fmt.Errorf("space self-api registry row %q has an invalid schema hash", id)
	}
	switch row.HashBasis {
	case "owner_contract":
		// Only the pinned tickets.create hash predates the parameter-bytes rule.
		if id != ticketsCreateActionID {
			return fmt.Errorf("space self-api registry row %q may not pin an owner contract", id)
		}
	case "parameters_json":
		if row.SchemaSHA256 != runActionSchemaCommitment(row.ParametersJSON) {
			return fmt.Errorf("space self-api registry row %q schema hash does not match its parameters", id)
		}
	default:
		return fmt.Errorf("space self-api registry row %q has an unknown hash basis", id)
	}
	return validateRunActionParameters(row)
}

// validateRunActionParameters checks the closed argument schema a model sees: an
// object with additionalProperties false, exactly one property per argument,
// only scalar or array types, and no format or reference keyword (no URL or
// markup field).
func validateRunActionParameters(row runActionSpecRow) error {
	var schema map[string]any
	decoder := json.NewDecoder(strings.NewReader(row.ParametersJSON))
	decoder.UseNumber()
	if err := decoder.Decode(&schema); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return fmt.Errorf("space self-api registry row %q parameters are not one JSON object", row.ActionID)
	}
	if schema["type"] != "object" || schema["additionalProperties"] != false || runActionSchemaHasUnsafeKeyword(schema) {
		return fmt.Errorf("space self-api registry row %q parameters are not a closed object", row.ActionID)
	}
	properties, ok := schema["properties"].(map[string]any)
	if !ok || len(properties) != len(row.ArgumentOrder) {
		return fmt.Errorf("space self-api registry row %q argument order does not name each property", row.ActionID)
	}
	seen := make(map[string]bool, len(row.ArgumentOrder))
	for _, name := range row.ArgumentOrder {
		property, found := properties[name].(map[string]any)
		if !found || seen[name] || runActionNameIsForbidden(name) {
			return fmt.Errorf("space self-api registry row %q argument is invalid", row.ActionID)
		}
		seen[name] = true
		switch property["type"] {
		case "string", "boolean", "integer", "array":
		default:
			return fmt.Errorf("space self-api registry row %q argument has an unsupported type", row.ActionID)
		}
	}
	return nil
}

func runActionSchemaHasUnsafeKeyword(value any) bool {
	switch typed := value.(type) {
	case map[string]any:
		for key, child := range typed {
			if key == "format" || key == "$ref" || runActionSchemaHasUnsafeKeyword(child) {
				return true
			}
		}
	case []any:
		for _, child := range typed {
			if runActionSchemaHasUnsafeKeyword(child) {
				return true
			}
		}
	}
	return false
}

func runActionNameIsForbidden(name string) bool {
	for _, part := range runActionForbiddenNameParts {
		if strings.Contains(name, part) {
			return true
		}
	}
	return false
}

func runActionSchemaCommitment(text string) string {
	sum := sha256.Sum256([]byte(text))
	return "sha256:" + hex.EncodeToString(sum[:])
}

// runActionSpec returns the golden row for an action id. A malformed copy yields
// no row, so every caller refuses.
func runActionSpec(actionID string) (runActionSpecRow, bool) {
	if runActionRegistryErr != nil {
		return runActionSpecRow{}, false
	}
	row, found := runActionRegistry[strings.TrimSpace(actionID)]
	return row, found
}

// runActionAllowlisted reports whether CONTROL_SPACE_SELF_API_ACTIONS names the
// action. The list is an operator setting, empty by default, read at call time.
// It can only narrow what the golden admits and never grants authority.
func runActionAllowlisted(actionID string) bool {
	actionID = strings.TrimSpace(actionID)
	if actionID == "" {
		return false
	}
	for _, item := range strings.Split(os.Getenv(spaceSelfAPIActionsEnv), ",") {
		if strings.TrimSpace(item) == actionID {
			return true
		}
	}
	return false
}

// runActionSelfAPIIssuable reports whether Control may issue or accept a golden
// row for effect. tickets.create predates the list and is exempt from it for
// decisions only; the model action view requires runActionAllowlisted for every
// row, tickets.create included.
func runActionSelfAPIIssuable(actionID string) bool {
	actionID = strings.TrimSpace(actionID)
	if actionID == ticketsCreateActionID {
		return true
	}
	return runActionAllowlisted(actionID)
}

// runActionRegistryIDs lists the golden action ids in sorted order, so every
// issuer orders its rows the same way. A malformed copy lists none.
func runActionRegistryIDs() []string {
	if runActionRegistryErr != nil {
		return nil
	}
	ids := make([]string, 0, len(runActionRegistry))
	for id := range runActionRegistry {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	return ids
}

// RunActionReadAdmitted reports whether a read-only run action (A-REC-03,
// space.directory.resolve) may run for a member of the given Space role: the
// golden names the row as a run action, the operator allowlist names it, and
// the role meets the row's floor. A read issues no signed decision, so this is
// the whole registry check its Control route makes. A malformed copy, an absent
// row or an empty allowlist answers false.
func RunActionReadAdmitted(actionID, role string) bool {
	spec, found := runActionSpec(actionID)
	return found && spec.DecisionType == runActionDecisionKind && runActionAllowlisted(spec.ActionID) &&
		RoleRank(role) >= RoleRank(spec.MinRole) && RoleRank(role) > 0
}
