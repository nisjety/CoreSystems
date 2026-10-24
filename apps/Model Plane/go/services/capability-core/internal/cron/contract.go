package cron

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"io"
	"regexp"
	"strings"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
)

// The stored-consent contract (DEC-16, amended not reversed) is the read-only
// authority a scheduled Do run may use without a user present. It lives in the
// schedule's task template as `approval_contract`, a JSON text value. Storage
// keeps a string verbatim, while a nested object would be re-ordered by Postgres
// JSONB and refused by the template digest rule, so the text itself is what the
// digest covers. Control signs that digest; this file recomputes it, checks the
// contract's shape and window, and checks every named tool against the registry.
// It is the Go twin of mp-contracts::scheduled_contract.

const (
	ContractVersion           = 1
	ContractClassCeiling      = models.SideEffectRead
	ContractMaxTools          = 16
	ContractMaxRounds         = 8
	ContractMaxTokensPerRound = 4096
	// ContractMaxDays is the longest a contract may run before renewal. Control
	// enforces the same bound when it signs the decision.
	ContractMaxDays = 90
	// ContractDefaultDays is what the consent card offers; it is not enforced here.
	ContractDefaultDays = 30

	contractTimeLayout = "2006-01-02T15:04:05Z"
)

var (
	contractToolNamePattern = regexp.MustCompile(`^[a-z][a-z0-9_.]{0,63}$`)
	contractCapabilityID    = regexp.MustCompile(`^[a-z][a-z0-9_.-]{0,127}$`)
)

// nonDelegableExact are tool names that never enter a stored consent, whatever
// their registry row says. Each acts on the world, holds a browser or code
// runtime, or changes what the assistant remembers or can do.
var nonDelegableExact = map[string]bool{
	"browser_agent":    true,
	"code_interpreter": true,
	"shell":            true,
	"tickets.create":   true,
	"book_shipment":    true,
	"memory.write":     true,
	"skill.write":      true,
	"schedule.create":  true,
	"schedule.patch":   true,
	"schedule.delete":  true,
}

// nonDelegablePrefixes cover families: browser tools, process tools, and every
// MCP tool (MCP tools are never delegable in v1, whatever their annotations say).
var nonDelegablePrefixes = []string{"browser.", "process_", "mcp__"}

// ScheduleContract is the parsed `approval_contract`. Its text is the digest
// input, so this struct only validates shape and bounds.
type ScheduleContract struct {
	V                 int            `json:"v"`
	Tools             []ContractTool `json:"tools"`
	ExpiresAt         string         `json:"expires_at"`
	MaxRounds         int            `json:"max_rounds"`
	MaxTokensPerRound int            `json:"max_tokens_per_round"`
}

// ContractTool is one tool the owner approved for unattended use, with the
// class it was approved under. The approval is only as good as the registry
// row it names, which VerifyContractAgainstRegistry re-reads at create.
type ContractTool struct {
	Tool       string `json:"tool"`
	Capability string `json:"capability"`
	Class      string `json:"class"`
	ApprovedAt string `json:"approved_at"`
}

// DeclaredTool is one capability_tools row as the registry declares it.
type DeclaredTool struct {
	ToolName        string
	CapabilityID    string
	SideEffectClass string
	ApprovalFloor   string
	CallRefinement  string
}

// IsNonDelegableTool reports whether a tool may never be part of a stored consent.
func IsNonDelegableTool(name string) bool {
	if nonDelegableExact[name] {
		return true
	}
	for _, prefix := range nonDelegablePrefixes {
		if strings.HasPrefix(name, prefix) {
			return true
		}
	}
	return false
}

// DelegableClass reports whether a declared class may be approved in a stored
// consent. Version 1 admits `read` only, so a contract can never authorise an effect.
func DelegableClass(class string) bool {
	return class == ContractClassCeiling
}

// ParseScheduleContract decodes an `approval_contract` text strictly: unknown
// keys, trailing data, a wrong version, an empty or oversized tool list, a
// non-canonical time, or a limit out of range is refused. It checks shape and
// bounds only; registry and window checks are separate so each can be tested alone.
func ParseScheduleContract(text string) (ScheduleContract, error) {
	decoder := json.NewDecoder(strings.NewReader(text))
	decoder.DisallowUnknownFields()
	var contract ScheduleContract
	if err := decoder.Decode(&contract); err != nil {
		return ScheduleContract{}, fmt.Errorf("schedule contract is malformed: %w", err)
	}
	if _, err := decoder.Token(); err != io.EOF {
		return ScheduleContract{}, fmt.Errorf("schedule contract has trailing data")
	}
	if contract.V != ContractVersion {
		return ScheduleContract{}, fmt.Errorf("schedule contract version %d is not supported", contract.V)
	}
	if len(contract.Tools) == 0 || len(contract.Tools) > ContractMaxTools {
		return ScheduleContract{}, fmt.Errorf("schedule contract must name between 1 and %d tools", ContractMaxTools)
	}
	seen := map[string]bool{}
	for _, tool := range contract.Tools {
		if !contractToolNamePattern.MatchString(tool.Tool) || !contractCapabilityID.MatchString(tool.Capability) {
			return ScheduleContract{}, fmt.Errorf("schedule contract tool identifiers are invalid")
		}
		if seen[tool.Tool] {
			return ScheduleContract{}, fmt.Errorf("schedule contract names a tool twice")
		}
		seen[tool.Tool] = true
		if IsNonDelegableTool(tool.Tool) {
			return ScheduleContract{}, fmt.Errorf("schedule contract names a non-delegable tool")
		}
		if !DelegableClass(tool.Class) {
			return ScheduleContract{}, fmt.Errorf("schedule contract class %q is above the ceiling", tool.Class)
		}
		if _, err := parseContractTime(tool.ApprovedAt); err != nil {
			return ScheduleContract{}, fmt.Errorf("schedule contract approval time is invalid")
		}
	}
	if _, err := parseContractTime(contract.ExpiresAt); err != nil {
		return ScheduleContract{}, fmt.Errorf("schedule contract expiry is invalid")
	}
	if contract.MaxRounds < 1 || contract.MaxRounds > ContractMaxRounds {
		return ScheduleContract{}, fmt.Errorf("schedule contract max_rounds must be between 1 and %d", ContractMaxRounds)
	}
	if contract.MaxTokensPerRound < 1 || contract.MaxTokensPerRound > ContractMaxTokensPerRound {
		return ScheduleContract{}, fmt.Errorf("schedule contract max_tokens_per_round must be between 1 and %d", ContractMaxTokensPerRound)
	}
	return contract, nil
}

// parseContractTime accepts only the exact UTC form `YYYY-MM-DDTHH:MM:SSZ`, one
// spelling per instant, so the digest is the single source of what was approved.
func parseContractTime(raw string) (time.Time, error) {
	parsed, err := time.Parse(contractTimeLayout, raw)
	if err != nil {
		return time.Time{}, err
	}
	if parsed.UTC().Format(contractTimeLayout) != raw {
		return time.Time{}, fmt.Errorf("time %q is not canonical", raw)
	}
	return parsed, nil
}

// CheckContractWindow refuses a contract that has already expired or runs past
// the maximum. Control applies the same bound; capability-core repeats it so a
// template cannot be stored with a contract Control would not have signed.
func CheckContractWindow(contract ScheduleContract, now time.Time) error {
	expires, err := parseContractTime(contract.ExpiresAt)
	if err != nil {
		return err
	}
	if !expires.After(now) {
		return fmt.Errorf("schedule contract has already expired")
	}
	if expires.After(now.AddDate(0, 0, ContractMaxDays)) {
		return fmt.Errorf("schedule contract expires after the %d day maximum", ContractMaxDays)
	}
	return nil
}

// VerifyContractAgainstRegistry requires every approved tool to be a read-class,
// no-floor, non-refined row of the capability it names, and not on the
// non-delegable list. Every matching row must agree, so an org override cannot
// hide a stricter row behind a permissive global one.
func VerifyContractAgainstRegistry(contract ScheduleContract, declared []DeclaredTool) error {
	for _, approved := range contract.Tools {
		if IsNonDelegableTool(approved.Tool) {
			return fmt.Errorf("tool %q cannot be approved for unattended use", approved.Tool)
		}
		matches := 0
		for _, row := range declared {
			if row.ToolName != approved.Tool {
				continue
			}
			matches++
			if row.CapabilityID != approved.Capability || row.SideEffectClass != models.SideEffectRead ||
				row.ApprovalFloor != models.ApprovalFloorNone || row.CallRefinement != "" {
				return fmt.Errorf("tool %q is not a read tool with no approval floor in the registry", approved.Tool)
			}
		}
		if matches == 0 {
			return fmt.Errorf("tool %q is not declared in the registry", approved.Tool)
		}
	}
	return nil
}

// ContractDigest is `sha256:<hex>` over the exact UTF-8 bytes of the stored
// `approval_contract` text. Taking the bytes, not a re-serialisation, keeps Go,
// Rust and the gateway in agreement without a canonical form to maintain.
func ContractDigest(text string) string {
	return fmt.Sprintf("sha256:%x", sha256.Sum256([]byte(text)))
}
