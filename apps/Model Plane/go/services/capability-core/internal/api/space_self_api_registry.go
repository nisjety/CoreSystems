package api

import (
	"bytes"
	_ "embed"
	"encoding/json"
	"fmt"
	"io"
)

// spaceSelfAPIRegistryJSON is the vendored, byte-identical copy of the golden
// apps/Model Plane/api/contracts/space-self-api-registry.json. Capability Core
// reads it only as a cross-check. A Control-signed run-action row is offered only
// when the golden lists the same id, decision type, capability, parameters,
// approval and schema hash. The golden carries no prose, so the description comes
// from the admission row. Any disagreement refuses the whole view.
//
//go:embed registry/space-self-api-registry.json
var spaceSelfAPIRegistryJSON []byte

const (
	spaceSelfAPIRegistryContract = "space-self-api-registry"
	spaceSelfAPIRegistryVersion  = 1
)

// spaceSelfAPIRow is one golden row. The tags mirror the contract exactly. The
// strict decode refuses a copy that carries a field this struct does not name.
type spaceSelfAPIRow struct {
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

type spaceSelfAPIRegistryDocument struct {
	Contract      string            `json:"contract"`
	Version       int               `json:"version"`
	Rows          []spaceSelfAPIRow `json:"rows"`
	DigestVectors json.RawMessage   `json:"digest_vectors"`
}

// spaceSelfAPIRows and spaceSelfAPIRowsErr are set once at package init and never
// change afterwards.
var spaceSelfAPIRows, spaceSelfAPIRowsErr = loadSpaceSelfAPIRows(spaceSelfAPIRegistryJSON)

// loadSpaceSelfAPIRows decodes the golden as one strict JSON document. A malformed
// copy yields no rows, so every lookup refuses.
func loadSpaceSelfAPIRows(raw []byte) (map[string]spaceSelfAPIRow, error) {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var doc spaceSelfAPIRegistryDocument
	if err := decoder.Decode(&doc); err != nil || decoder.Decode(&struct{}{}) != io.EOF {
		return nil, fmt.Errorf("space self-api registry is not one strict JSON document")
	}
	if doc.Contract != spaceSelfAPIRegistryContract || doc.Version != spaceSelfAPIRegistryVersion || len(doc.DigestVectors) == 0 {
		return nil, fmt.Errorf("space self-api registry header is invalid")
	}
	rows := make(map[string]spaceSelfAPIRow, len(doc.Rows))
	for _, row := range doc.Rows {
		if row.ActionID == "" || row.CapabilityID == "" || !schemaHashPattern.MatchString(row.SchemaSHA256) {
			return nil, fmt.Errorf("space self-api registry row is incomplete")
		}
		if _, repeated := rows[row.ActionID]; repeated {
			return nil, fmt.Errorf("space self-api registry repeats an action id")
		}
		rows[row.ActionID] = row
	}
	return rows, nil
}

// spaceSelfAPIRowFor returns the golden row for an action id, or false when the
// id is not a row or the copy is malformed.
func spaceSelfAPIRowFor(actionID string) (spaceSelfAPIRow, bool) {
	if spaceSelfAPIRowsErr != nil {
		return spaceSelfAPIRow{}, false
	}
	row, found := spaceSelfAPIRows[actionID]
	return row, found
}
