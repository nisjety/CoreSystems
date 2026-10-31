package models

import "fmt"

// Declared side-effect classes for a tool (WS05). The vocabulary is closed and
// ordered from the weakest to the strictest, and `unclassified` is the
// strictest: a tool nobody has classified is treated as the most dangerous it
// could be, never as a read. This is the Go twin of mp-contracts::tool_class;
// both are pinned by the same known answers, so a class means the same thing to
// the registry that stores it and the gate that reads it.
const (
	SideEffectRead           = "read"
	SideEffectSandboxCompute = "sandbox_compute"
	SideEffectContextWrite   = "context_write"
	SideEffectExternalWrite  = "external_write"
	SideEffectDestructive    = "destructive"
	SideEffectOwnerEffect    = "owner_effect"
	SideEffectUnclassified   = "unclassified"
)

// Approval floors: the least approval a class may ever be run under. A floor
// only ever tightens what a posture would otherwise decide.
const (
	ApprovalFloorNone         = "none"
	ApprovalFloorAsk          = "ask"
	ApprovalFloorDurableOwner = "durable_owner"
)

// CallRefinementProviderOperation marks the one tool whose class depends on its
// arguments: execute_provider_action is a read or a write by its `operation`,
// so its row carries the write class and this marker lets a call whose
// operation is catalogued as a read lower to read. No other refinement exists.
const CallRefinementProviderOperation = "provider_operation"

// Autonomy rung labels a class requires, matching mp-contracts::autonomy.
const (
	RungReadOnly         = "read_only"
	RungWorkspaceWrite   = "workspace_write"
	RungDangerFullAccess = "danger_full_access"
)

// Declared MCP side effect, what a server's annotations amount to once the MCP
// interpretation defaults are applied. `undeclared` is its own value so silence
// is never mistaken for a statement.
const (
	DeclaredReadOnly            = "read_only"
	DeclaredNonDestructiveWrite = "non_destructive_write"
	DeclaredDestructive         = "destructive"
	DeclaredUndeclared          = "undeclared"
)

// sideEffectOrder lists the classes weakest to strictest; a class's index is its rank.
var sideEffectOrder = [...]string{
	SideEffectRead,
	SideEffectSandboxCompute,
	SideEffectContextWrite,
	SideEffectExternalWrite,
	SideEffectDestructive,
	SideEffectOwnerEffect,
	SideEffectUnclassified,
}

var approvalFloorOrder = [...]string{
	ApprovalFloorNone,
	ApprovalFloorAsk,
	ApprovalFloorDurableOwner,
}

// IsSupportedSideEffectClass reports whether raw is one of the closed set of
// classes. Strict for the reason IsSupportedRiskLevel is: a malformed stored
// value must never silently pass for a known class.
func IsSupportedSideEffectClass(raw string) bool {
	for _, class := range sideEffectOrder {
		if raw == class {
			return true
		}
	}
	return false
}

// NormalizeSideEffectClass maps a stored or wire value onto the vocabulary. An
// empty or unrecognised value becomes `unclassified`, the strictest class.
func NormalizeSideEffectClass(raw string) string {
	if IsSupportedSideEffectClass(raw) {
		return raw
	}
	return SideEffectUnclassified
}

// IsSupportedApprovalFloor reports whether raw is one of the closed set of floors.
func IsSupportedApprovalFloor(raw string) bool {
	for _, floor := range approvalFloorOrder {
		if raw == floor {
			return true
		}
	}
	return false
}

// NormalizeApprovalFloor maps a stored or wire value onto the floors. An empty
// or unrecognised value becomes `durable_owner`, the strictest floor, so a
// corrupt floor can only ever add a pause.
func NormalizeApprovalFloor(raw string) string {
	if IsSupportedApprovalFloor(raw) {
		return raw
	}
	return ApprovalFloorDurableOwner
}

// SideEffectClassRank is the class's position in the strictness order; higher
// is stricter. An unrecognised class ranks as `unclassified`.
func SideEffectClassRank(class string) int {
	for rank, known := range sideEffectOrder {
		if class == known {
			return rank
		}
	}
	return len(sideEffectOrder) - 1
}

// ApprovalFloorRank is the floor's position in the strictness order; higher is
// stricter. An unrecognised floor ranks as `durable_owner`.
func ApprovalFloorRank(floor string) int {
	for rank, known := range approvalFloorOrder {
		if floor == known {
			return rank
		}
	}
	return len(approvalFloorOrder) - 1
}

// StricterSideEffectClass returns the stricter of two classes. It is how a
// capability's ceiling is formed from its tools and how a write is checked
// against the class it would replace.
func StricterSideEffectClass(a, b string) string {
	if SideEffectClassRank(b) > SideEffectClassRank(a) {
		return NormalizeSideEffectClass(b)
	}
	return NormalizeSideEffectClass(a)
}

// StricterApprovalFloor returns the stricter of two floors.
func StricterApprovalFloor(a, b string) string {
	if ApprovalFloorRank(b) > ApprovalFloorRank(a) {
		return NormalizeApprovalFloor(b)
	}
	return NormalizeApprovalFloor(a)
}

// SideEffectClassIsRisky reports whether a call of this class needs the human
// gate under the `ask` posture. `unclassified` is risky by construction.
func SideEffectClassIsRisky(class string) bool {
	switch NormalizeSideEffectClass(class) {
	case SideEffectExternalWrite, SideEffectDestructive, SideEffectOwnerEffect, SideEffectUnclassified:
		return true
	default:
		return false
	}
}

// SideEffectClassIsRestrictedContextWrite reports whether a restricted context
// (plan mode, a delegated leaf) must refuse this class although it is not risky.
func SideEffectClassIsRestrictedContextWrite(class string) bool {
	return NormalizeSideEffectClass(class) == SideEffectContextWrite
}

// SideEffectClassRequiredRung is the autonomy rung a call of this class needs.
func SideEffectClassRequiredRung(class string) string {
	switch NormalizeSideEffectClass(class) {
	case SideEffectRead, SideEffectSandboxCompute:
		return RungReadOnly
	case SideEffectContextWrite:
		return RungWorkspaceWrite
	default:
		return RungDangerFullAccess
	}
}

// MinimumApprovalFloor is the floor a class implies on its own, whatever a row
// says: an owner effect is never run without a durable owner approval, and a
// tool nobody has classified is never run without a person being asked, so an
// `unclassified` row (the default every new row is born with) cannot run under
// an `auto` posture with no approval.
func MinimumApprovalFloor(class string) string {
	switch NormalizeSideEffectClass(class) {
	case SideEffectOwnerEffect:
		return ApprovalFloorDurableOwner
	case SideEffectUnclassified:
		return ApprovalFloorAsk
	default:
		return ApprovalFloorNone
	}
}

// ValidateClassAndFloor rejects a pair the registry must not store: either value
// outside its vocabulary, or a floor below what the class implies.
func ValidateClassAndFloor(class, floor string) error {
	if !IsSupportedSideEffectClass(class) {
		return fmt.Errorf("unsupported side_effect_class %q", class)
	}
	if !IsSupportedApprovalFloor(floor) {
		return fmt.Errorf("unsupported approval_floor %q", floor)
	}
	if ApprovalFloorRank(floor) < ApprovalFloorRank(MinimumApprovalFloor(class)) {
		return fmt.Errorf("side_effect_class %q requires approval_floor %q", class, MinimumApprovalFloor(class))
	}
	return nil
}

// EffectiveApprovalFloor is the floor to report for a stored (class, floor)
// pair: the stored floor, normalised like every other floor (an empty or
// unrecognised value is the strictest, `durable_owner`, exactly as the Rust
// twin's ApprovalFloor::from_wire reads it), raised to what the class implies on
// its own. A caller that reads only this can never see an owner effect without
// its durable owner approval, nor an unclassified tool without an ask.
func EffectiveApprovalFloor(class, floor string) string {
	return StricterApprovalFloor(NormalizeApprovalFloor(floor), MinimumApprovalFloor(class))
}

// DeriveMCPClass derives the class of an MCP tool from what its server declared.
// Derivation only tightens. A server's annotations are third-party claims, so
// unless the org has chosen to trust them a `read_only` claim is not believed
// and derives as an external write. A server that said nothing derives as
// `unclassified`; a destructive claim is believed in both modes because
// believing it can only add a pause.
func DeriveMCPClass(declared string, annotationsTrusted bool) string {
	switch declared {
	case DeclaredReadOnly:
		if annotationsTrusted {
			return SideEffectRead
		}
		return SideEffectExternalWrite
	case DeclaredNonDestructiveWrite:
		return SideEffectExternalWrite
	case DeclaredDestructive:
		return SideEffectDestructive
	default:
		return SideEffectUnclassified
	}
}
