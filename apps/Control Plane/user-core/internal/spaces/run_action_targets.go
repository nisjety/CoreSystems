package spaces

import "strings"

const (
	runActionTargetsEnv             = "RUN_ACTION_TARGETS"
	ticketsCreateActionID           = "tickets.create"
	runActionClassWriteApproval     = "write_approval"
	runActionHashBasisOwnerContract = "owner_contract"
)

// RunActionTargetsMode is the operator switch for which admitted owner actions a
// Model may be offered. The zero value is tickets_only, so an unset or unknown
// value can never widen what the Model is shown.
type RunActionTargetsMode string

const (
	RunActionTargetsTicketsOnly RunActionTargetsMode = "tickets_only"
	RunActionTargetsTable       RunActionTargetsMode = "table"
)

// RunActionTargetsModeFromLookup reads RUN_ACTION_TARGETS strictly: only the
// literal "table" offers every admitted row, anything else keeps tickets.create.
// Superseded: nothing in production reads this switch. The model action view admits
// rows only through CONTROL_SPACE_SELF_API_ACTIONS. It is kept so existing callers
// compile, and it no longer widens any view.
func RunActionTargetsModeFromLookup(lookup func(string) string) RunActionTargetsMode {
	if lookup != nil && lookup(runActionTargetsEnv) == string(RunActionTargetsTable) {
		return RunActionTargetsTable
	}
	return RunActionTargetsTicketsOnly
}

// runActionTargetRow is one owner-plane admission row. A row alone admits nothing
// on the Model side: the action also needs its pinned executor and an owner
// verifier that checks its own authority before any effect.
type runActionTargetRow struct {
	ActionID        string
	ServiceAudience string
	Permission      string
	Class           string
	HashBasis       string
	SchemaHash      string
}

// runActionTargets is the Control projection of the golden registry: one row per
// golden action, built from the same parsed document as runActionSpec. It replaces
// the one-row seed, so the owner reservation and this lookup cannot disagree with
// the decision path. It is a variable so the tests can inject a row.
var runActionTargets = runActionTargetRowsFromRegistry()

// runActionTargetRowsFromRegistry projects every golden row in sorted id order. A
// malformed golden copy projects no rows, so every lookup refuses.
func runActionTargetRowsFromRegistry() []runActionTargetRow {
	ids := runActionRegistryIDs()
	rows := make([]runActionTargetRow, 0, len(ids))
	for _, id := range ids {
		spec, found := runActionSpec(id)
		if !found {
			continue
		}
		class := spec.AutonomyRung
		if spec.Approval == "owner_approval" {
			class = runActionClassWriteApproval
		}
		rows = append(rows, runActionTargetRow{
			ActionID:        spec.ActionID,
			ServiceAudience: spec.ServiceAudience,
			Permission:      spec.Permission,
			Class:           class,
			HashBasis:       spec.HashBasis,
			SchemaHash:      spec.SchemaSHA256,
		})
	}
	return rows
}

func runActionTargetFor(actionID string) (runActionTargetRow, bool) {
	actionID = strings.TrimSpace(actionID)
	for _, target := range runActionTargets {
		if target.ActionID == actionID {
			return target, true
		}
	}
	return runActionTargetRow{}, false
}

// runActionTargetForMode narrows the table by the operator mode. tickets_only
// admits no row except tickets.create, whatever the table holds.
func runActionTargetForMode(actionID string, mode RunActionTargetsMode) (runActionTargetRow, bool) {
	target, found := runActionTargetFor(actionID)
	if !found || (mode != RunActionTargetsTable && target.ActionID != ticketsCreateActionID) {
		return runActionTargetRow{}, false
	}
	return target, true
}

// runActionTarget returns the Conversation Core audience an owner action must
// be decided for.
func runActionTarget(actionID string) (string, bool) {
	target, found := runActionTargetFor(actionID)
	return target.ServiceAudience, found
}
