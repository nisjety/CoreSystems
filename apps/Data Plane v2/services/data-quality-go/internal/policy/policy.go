// Package policy owns the per-organization processing policy: the operator's
// overrides for background content processing (contextual retrieval, graph
// extraction, a prompt-version pin and a budget mode), stored one row per
// organization in org_processing_policy (migration 20261003130000).
//
// # What the policy is, and what it is not
//
// An ABSENT row means the system default, which is ON. A row exists only because
// an operator set something. Turning a feature OFF is an exceptional, audited act:
// it needs a reason, it is operator-only (a service principal, never an
// organization's own admin), and the pipeline records an explicit
// `skipped_policy` status on the affected units, so "off" is never silent. The
// owner ruling is that contextual retrieval is NOT switched off for customers; the
// per-organization off exists for the evaluation baseline organizations and for
// incidents (decision OD-PER-ORG-OFF).
//
// The embedding engine reads the same table (embedding-engine-rs src/policy.rs)
// and is the only consumer that turns it into behaviour. This package is the
// write path and the read path of the admin API; it never touches knowledge_units.
package policy

import (
	"errors"
	"fmt"
	"math"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"
)

// The values of the two on/off switches.
const (
	Inherit = "inherit"
	On      = "on"
	Off     = "off"
)

// The values of budget_mode. alert_only is the system default (owner default D-E:
// exceeding a budget alerts, it does not stop work); stop is explicit and audited.
const (
	BudgetInherit   = "inherit"
	BudgetAlertOnly = "alert_only"
	BudgetThrottle  = "throttle"
	BudgetStop      = "stop"
)

// PromptVersions are the contextual-retrieval prompt versions the embedding engine
// knows (embedding-engine-rs src/context/prompt.rs, PromptVersion::ALL). A pin
// outside this list is refused here because the engine refuses it too: a typo must
// not quietly keep the default.
var PromptVersions = []string{"ctx-v1", "ctx-v2"}

// Enforcement says which parts of a policy something reads and acts on TODAY.
//
// An operator control that stores a value nothing honours is worse than no control: an
// operator who sets graph_extraction to off believes graph extraction stopped. Until a consumer
// reads a part, a value for it other than the default is REFUSED, not stored (400, naming the
// task that will read it), so the API never claims an effect it does not have. The embedding
// engine reads contextual_retrieval and context_prompt_version (CR-T11); graph-index reads
// graph_extraction (CR-T12: org_processing_policy is read before every extraction, an organization
// that is off ends in the state skipped_policy with no call) and the budget gate reads the budget
// (CR-T9b). graph-index already reads the budget columns for its own work, but the flag stays off
// until the gate that spans every consumer does, so no organization is told one worker honours a
// limit that another ignores. Flip a flag in the same change that makes its consumer read the
// column, with its test.
type Enforcement struct {
	GraphExtraction bool
	Budget          bool
}

// Enforced is what is honoured today.
var Enforced = Enforcement{GraphExtraction: true}

const (
	// MaxReasonLength is the longest reason, in characters.
	MaxReasonLength = 500
	// MaxMonthlyBudgetUSD bounds a budget figure so a typo cannot be a nonsense value.
	MaxMonthlyBudgetUSD = 1_000_000_000.0
	// MaxOrgIDLength is the longest organization id accepted in the admin path.
	MaxOrgIDLength = 128
)

// ValidOrgID reports whether an organization id is acceptable in the admin path.
// It is deliberately narrower than "any string": an id that carries a slash, a
// space or a control character is never an organization of this platform, and the
// path parameter must not be able to carry one into a log line or a SQL literal.
func ValidOrgID(id string) bool {
	if id == "" || len(id) > MaxOrgIDLength {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9':
		case i > 0 && (c == '-' || c == '_' || c == '.' || c == ':'):
		default:
			return false
		}
	}
	return true
}

// Policy is the policy of one organization as stored.
type Policy struct {
	OrgID                string
	ContextualRetrieval  string
	GraphExtraction      string
	ContextPromptVersion *string
	BudgetMode           string
	MonthlyBudgetUSD     *float64
	Reason               *string
	UpdatedBy            string
	CreatedAt            time.Time
	UpdatedAt            time.Time
}

// Record is a policy together with whether a row exists. An organization without
// a row has the system default: every switch inherit, no pin, no budget figure.
type Record struct {
	Policy
	Exists bool
}

// Default is the policy of an organization that has no row.
func Default(orgID string) Record {
	return Record{Policy: Policy{
		OrgID:               orgID,
		ContextualRetrieval: Inherit,
		GraphExtraction:     Inherit,
		BudgetMode:          BudgetInherit,
	}}
}

// Effective is what a policy comes to, in words an operator can read back.
type Effective struct {
	// ContextualRetrieval is "on" or "off". The global kill switch
	// (CONTEXTUAL_RETRIEVAL_ENABLED=false) overrides every organization value:
	// an organization set to "on" is not switched on against an incident hold.
	ContextualRetrieval string `json:"contextual_retrieval"`
	// GraphExtraction is "off", "on" or "deployment_default" (inherit): this
	// service does not know the deployment's own setting.
	GraphExtraction string `json:"graph_extraction"`
}

// Effective turns the switches into what they mean.
func (p Policy) Effective() Effective {
	contextual := "on"
	if p.ContextualRetrieval == Off {
		contextual = "off"
	}
	graph := "deployment_default"
	switch p.GraphExtraction {
	case Off:
		graph = "off"
	case On:
		graph = "on"
	}
	return Effective{ContextualRetrieval: contextual, GraphExtraction: graph}
}

// Input is what an operator asks the policy to become. It replaces the whole
// policy: a PUT states every switch, so that an omitted field can never lift a
// hold by accident.
type Input struct {
	ContextualRetrieval  string
	GraphExtraction      string
	ContextPromptVersion *string
	BudgetMode           string
	MonthlyBudgetUSD     *float64
	Reason               *string
}

// FieldError names the field an input was refused for.
type FieldError struct {
	Field   string
	Message string
}

func (e *FieldError) Error() string { return e.Field + ": " + e.Message }

func fieldError(field, format string, args ...any) error {
	return &FieldError{Field: field, Message: fmt.Sprintf(format, args...)}
}

// IsFieldError extracts the refusal from an error.
func IsFieldError(err error) (*FieldError, bool) {
	var fe *FieldError
	if errors.As(err, &fe) {
		return fe, true
	}
	return nil, false
}

func oneOf(value string, allowed ...string) bool {
	for _, candidate := range allowed {
		if value == candidate {
			return true
		}
	}
	return false
}

// Validate normalizes (trims) and checks an input. The rules that the database
// also enforces (a reason when a feature is off) are checked here first so the
// operator gets a field name rather than a constraint violation, and one rule is
// stricter than the database: `stop` also needs a reason, because it is the one
// budget mode that withholds work.
func (in Input) Validate() (Input, error) {
	out := in
	out.ContextualRetrieval = strings.TrimSpace(in.ContextualRetrieval)
	out.GraphExtraction = strings.TrimSpace(in.GraphExtraction)
	out.BudgetMode = strings.TrimSpace(in.BudgetMode)

	if !oneOf(out.ContextualRetrieval, Inherit, On, Off) {
		return Input{}, fieldError("contextual_retrieval", "must be one of inherit, on, off")
	}
	if !oneOf(out.GraphExtraction, Inherit, On, Off) {
		return Input{}, fieldError("graph_extraction", "must be one of inherit, on, off")
	}
	if !oneOf(out.BudgetMode, BudgetInherit, BudgetAlertOnly, BudgetThrottle, BudgetStop) {
		return Input{}, fieldError("budget_mode", "must be one of inherit, alert_only, throttle, stop")
	}
	if !Enforced.GraphExtraction && out.GraphExtraction != Inherit {
		return Input{}, fieldError("graph_extraction",
			"is not enforced yet (graph-index reads it from CR-T12): a value that nothing honours would be a silent no-op, so only inherit is accepted")
	}
	if !Enforced.Budget && out.BudgetMode != BudgetInherit {
		return Input{}, fieldError("budget_mode",
			"is not enforced yet (the budget gate is CR-T9b): a value that nothing honours would be a silent no-op, so only inherit is accepted")
	}
	if !Enforced.Budget && in.MonthlyBudgetUSD != nil {
		return Input{}, fieldError("monthly_budget_usd",
			"is not enforced yet (the budget gate is CR-T9b): a figure that nothing reads would be a silent no-op, so only null is accepted")
	}

	out.ContextPromptVersion = nil
	if in.ContextPromptVersion != nil {
		version := strings.TrimSpace(*in.ContextPromptVersion)
		if !oneOf(version, PromptVersions...) {
			return Input{}, fieldError("context_prompt_version", "must be null or one of %s", strings.Join(PromptVersions, ", "))
		}
		out.ContextPromptVersion = &version
	}

	out.MonthlyBudgetUSD = nil
	if in.MonthlyBudgetUSD != nil {
		figure := *in.MonthlyBudgetUSD
		if math.IsNaN(figure) || math.IsInf(figure, 0) || figure < 0 || figure > MaxMonthlyBudgetUSD {
			return Input{}, fieldError("monthly_budget_usd", "must be null or a number from 0 to %.0f", MaxMonthlyBudgetUSD)
		}
		out.MonthlyBudgetUSD = &figure
	}

	out.Reason = nil
	if in.Reason != nil {
		reason := strings.TrimSpace(*in.Reason)
		if utf8.RuneCountInString(reason) > MaxReasonLength {
			return Input{}, fieldError("reason", "must be at most %d characters", MaxReasonLength)
		}
		for _, r := range reason {
			if unicode.IsControl(r) || r == utf8.RuneError {
				return Input{}, fieldError("reason", "must be plain text on one line")
			}
		}
		if reason != "" {
			out.Reason = &reason
		}
	}

	if out.ContextualRetrieval == Off || out.GraphExtraction == Off {
		if out.Reason == nil {
			return Input{}, fieldError("reason", "is required when a feature is set to off: an off is an audited exception, never silent")
		}
	}
	if out.BudgetMode == BudgetStop && out.Reason == nil {
		return Input{}, fieldError("reason", "is required when budget_mode is stop: it withholds work")
	}
	return out, nil
}

// IsDefault reports whether the input asks for the system default, in which case
// the row is removed rather than stored: a row exists only because an operator set
// something.
func (in Input) IsDefault() bool {
	return in.ContextualRetrieval == Inherit &&
		in.GraphExtraction == Inherit &&
		in.BudgetMode == BudgetInherit &&
		in.ContextPromptVersion == nil &&
		in.MonthlyBudgetUSD == nil
}

// Actor is who changed a policy, for the audit trail.
type Actor struct {
	// ID is the verified principal id (a service id).
	ID string
	// Type is the verified principal type ("service").
	Type string
	// RequestID correlates the change with the request that made it.
	RequestID string
}

// Result is what a successful write did.
type Result struct {
	// Action is "set" when a row was written and "reset" when the policy went back
	// to the default and the row was removed.
	Action string
	Before Record
	After  Record
}
