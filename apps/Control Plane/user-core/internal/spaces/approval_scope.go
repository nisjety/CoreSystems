package spaces

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strconv"
	"strings"
)

// WS10-S2: the scope vocabulary, the Space ceiling and the eligibility rules of
// Control-held approval authority. Control owns these; Model Plane only consumes
// them and may only tighten (allow < ask < block).
//
// What Control stores is a STANDING RULE (scope "always"). A once token lives in
// Session Core, and thread/run grants are Session Core's too (Model Plane plan
// mp-WS13-S6); Control still carries thread and once in the ceiling so one
// enum covers every scope the room can offer.

// ApprovalGrantScope is how far one approval reaches.
type ApprovalGrantScope string

const (
	// ApprovalScopeOnce waives this exact action once (Session Core owns it).
	ApprovalScopeOnce ApprovalGrantScope = "once"
	// ApprovalScopeThread waives prompts for one requester in one thread (Session
	// Core owns it once Model Plane delivers it).
	ApprovalScopeThread ApprovalGrantScope = "thread"
	// ApprovalScopeAlways is a Control standing rule for one agent's tool in one
	// Space. It is the only scope Control stores.
	ApprovalScopeAlways ApprovalGrantScope = "always"
)

// Rank orders the scopes by reach. Unknown values rank 0 and are never allowed.
func (s ApprovalGrantScope) Rank() int {
	switch s {
	case ApprovalScopeOnce:
		return 1
	case ApprovalScopeThread:
		return 2
	case ApprovalScopeAlways:
		return 3
	}
	return 0
}

// Valid reports whether s is one of the three closed scopes.
func (s ApprovalGrantScope) Valid() bool { return s.Rank() > 0 }

// Within reports whether s reaches no further than ceiling.
func (s ApprovalGrantScope) Within(ceiling ApprovalGrantScope) bool {
	return s.Valid() && ceiling.Valid() && s.Rank() <= ceiling.Rank()
}

// Addressee kinds a Space can default to (the named and requester kinds are
// per-approval and live in Session Core).
const (
	ApprovalAddresseeRequester  = "requester"
	ApprovalAddresseeAnyEditor  = "any_editor"
	ApprovalAddresseeAnyManager = "any_manager"
)

// Quarantine modes of a Space (WS10-S8 reads it; nothing enforces it here).
const (
	QuarantineOff     = "off"
	QuarantineShadow  = "shadow"
	QuarantineEnforce = "enforce"
)

// ApprovalPolicy is the per-Space approval ceiling. A Space with no row has the
// most restrictive policy (DefaultApprovalPolicy), never a permissive one.
type ApprovalPolicy struct {
	SpaceRef             string `json:"space_ref"`
	OrgID                string `json:"org_id"`
	MaxGrantScope        string `json:"max_grant_scope"`
	DefaultAddresseeKind string `json:"default_addressee_kind"`
	QuarantineMode       string `json:"quarantine_mode"`
	Revision             int64  `json:"revision"`
	UpdatedBy            string `json:"updated_by,omitempty"`
}

// DefaultApprovalPolicy is what a Space without a stored row gets. Revision 0
// means "not stored": a first write must present expected_revision 0.
func DefaultApprovalPolicy(spaceRef, orgID string) ApprovalPolicy {
	return ApprovalPolicy{
		SpaceRef: spaceRef, OrgID: orgID,
		MaxGrantScope:        string(ApprovalScopeOnce),
		DefaultAddresseeKind: ApprovalAddresseeRequester,
		QuarantineMode:       QuarantineOff,
	}
}

// ApprovalPolicyUpdate is the writable part of the policy. ExpectedRevision is
// the compare-and-set token (0 for the first write).
type ApprovalPolicyUpdate struct {
	MaxGrantScope        string `json:"max_grant_scope"`
	DefaultAddresseeKind string `json:"default_addressee_kind"`
	QuarantineMode       string `json:"quarantine_mode"`
	ExpectedRevision     int64  `json:"expected_revision"`
}

// Validate checks shape only; authority is resolved separately.
func (u ApprovalPolicyUpdate) Validate() error {
	if !ApprovalGrantScope(u.MaxGrantScope).Valid() {
		return fmt.Errorf("max_grant_scope must be once, thread or always")
	}
	switch u.DefaultAddresseeKind {
	case ApprovalAddresseeRequester, ApprovalAddresseeAnyEditor, ApprovalAddresseeAnyManager:
	default:
		return fmt.Errorf("default_addressee_kind must be requester, any_editor or any_manager")
	}
	switch u.QuarantineMode {
	case QuarantineOff, QuarantineShadow, QuarantineEnforce:
	default:
		return fmt.Errorf("quarantine_mode must be off, shadow or enforce")
	}
	if u.ExpectedRevision < 0 {
		return fmt.Errorf("expected_revision must not be negative")
	}
	return nil
}

// Loosens reports whether applying u to current raises the reach of what a
// member can waive. Only a loosening needs the entitlement and the non-ZDR
// check; tightening a ceiling is always allowed to a manager.
func (u ApprovalPolicyUpdate) Loosens(current ApprovalPolicy) bool {
	return ApprovalGrantScope(u.MaxGrantScope).Rank() > ApprovalGrantScope(current.MaxGrantScope).Rank()
}

// mcpToolActionPrefix is the name prefix of an MCP tool as Execution Core
// offers it (execution-core capability_policy MCP_TOOL_PREFIX).
const mcpToolActionPrefix = "mcp__"

// StandingRuleEligibleAction reports whether a standing rule may ever cover this
// tool/action. Default deny: only an MCP tool qualifies, and its action key is
// its own name. Provider writes (`execute_provider_action:*`), owner effects
// (`tickets.create`), shipments, shell and browser actions are ONCE only and
// can never be covered by a rule, whatever the caller asks for.
func StandingRuleEligibleAction(toolKey, actionKey string) bool {
	toolKey = strings.TrimSpace(toolKey)
	actionKey = strings.TrimSpace(actionKey)
	return toolKey == actionKey &&
		len(actionKey) > len(mcpToolActionPrefix) &&
		strings.HasPrefix(actionKey, mcpToolActionPrefix) &&
		len(actionKey) <= maxApprovalKeyLen
}

const (
	maxApprovalKeyLen     = 200
	maxApprovalIDLen      = 200
	approvalIdempotencyLo = 8
	approvalIdempotencyHi = 200
)

// ApprovalGateViewDigest commits to the part of the view the gate caches: the
// policy revision and ceiling. The tool levels join it in WS10-S5a.
func ApprovalGateViewDigest(policy ApprovalPolicy) string {
	sum := sha256.Sum256([]byte("approval-gate-view-v1\x00" +
		strconv.FormatInt(policy.Revision, 10) + "\x00" + policy.MaxGrantScope + "\x00" + policy.QuarantineMode))
	return "sha256:" + hex.EncodeToString(sum[:])
}
