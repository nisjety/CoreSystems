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

// runActionTargets seeds tickets.create only. It is a variable so the tests can
// exercise a second row without changing the production table.
var runActionTargets = []runActionTargetRow{
	{
		ActionID:        ticketsCreateActionID,
		ServiceAudience: ticketsCreateServiceAudience,
		Permission:      ticketsCreateActionPermission,
		Class:           runActionClassWriteApproval,
		HashBasis:       runActionHashBasisOwnerContract,
		SchemaHash:      ticketsCreateActionSchemaSHA256,
	},
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
