// Package skilllint is the one lint every skill write passes (EP-11 S4).
//
// Two services write agent_skills: session-core's UpsertAgentSkill and this service's
// registry routes. The rules are written down once, in
// api/contracts/authored-behaviour-lint.json; this is the Go reading of them and
// mp-contracts skill_lint is the Rust one. Both test against the same rule file, its
// .sha256 pin and authored-behaviour-lint-golden.json, so a rule changed in one language
// and not the other fails a test.
//
// A finding names a rule and a field, never the text, so it can be logged, counted and
// returned to a caller without echoing the skill back.
package skilllint

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

// ModeEnv is the switch both writers read. Only "report" and "enforce" turn the lint on.
const ModeEnv = "SKILL_LINT_MODE"

// ErrorCode is the typed error code a refused write answers with (HTTP 422).
const ErrorCode = "skill_lint_failed"

// The limits, in characters (Unicode scalar values).
const (
	MaxNameChars           = 64
	MaxDescriptionChars    = 300
	MaxContentChars        = 32000
	ContentBudgetChars     = 8000 // SKILL_CONTEXT_BUDGET_CHARS: longer content is cut when injected
	TranscriptMinLines     = 3
	MaxTriggerKeywords     = 20
	MaxTriggerKeywordChars = 40
)

// TranscriptPrefixes are the line starts that read as a chat turn rather than a lesson.
// Case matters: the speaker labels chat exports write are capitalised.
var TranscriptPrefixes = []string{"User:", "Assistant:", "Bruker:"}

// The closed rule codes, in the order findings are reported.
const (
	CodeNameEmpty               = "name_empty"
	CodeNameTooLong             = "name_too_long"
	CodeNameCharset             = "name_charset"
	CodeDescriptionMissing      = "description_missing"
	CodeDescriptionTooLong      = "description_too_long"
	CodeContentEmpty            = "content_empty"
	CodeContentTooLong          = "content_too_long"
	CodeContentExceedsBudget    = "content_exceeds_budget"
	CodeTranscriptShape         = "transcript_shape"
	CodeTriggerKeywordCount     = "trigger_keyword_count"
	CodeTriggerKeywordTooLong   = "trigger_keyword_too_long"
	CodeToolRestrictionInvalid  = "tool_restriction_invalid"
	CodeFrontmatterNameMismatch = "frontmatter_name_mismatch"
)

// Severity says whether a finding refuses a write under enforce ("error") or only
// informs ("warn").
type Severity string

const (
	SeverityError Severity = "error"
	SeverityWarn  Severity = "warn"
)

// Rule is one rule of the contract. Limit is 0 for a rule without one.
type Rule struct {
	Code     string
	Field    string
	Severity Severity
	Limit    int
}

// Rules is every rule, in the contract's order.
var Rules = []Rule{
	{CodeNameEmpty, "name", SeverityError, 0},
	{CodeNameTooLong, "name", SeverityError, MaxNameChars},
	{CodeNameCharset, "name", SeverityError, 0},
	{CodeDescriptionMissing, "description", SeverityWarn, 0},
	{CodeDescriptionTooLong, "description", SeverityError, MaxDescriptionChars},
	{CodeContentEmpty, "content", SeverityError, 0},
	{CodeContentTooLong, "content", SeverityError, MaxContentChars},
	{CodeContentExceedsBudget, "content", SeverityWarn, ContentBudgetChars},
	{CodeTranscriptShape, "content", SeverityWarn, TranscriptMinLines},
	{CodeTriggerKeywordCount, "trigger_keywords", SeverityError, MaxTriggerKeywords},
	{CodeTriggerKeywordTooLong, "trigger_keywords", SeverityError, MaxTriggerKeywordChars},
	{CodeToolRestrictionInvalid, "tool_restrictions", SeverityError, 0},
	{CodeFrontmatterNameMismatch, "frontmatter", SeverityError, 0},
}

// Finding is one broken rule as a caller sees it: the code, the field and the severity.
type Finding struct {
	Code     string   `json:"code"`
	Field    string   `json:"field"`
	Severity Severity `json:"severity"`
}

// Input is the skill text a write would store. FrontmatterName is the name in a bundle's
// SKILL.md front matter; nil unless a bundle is checked.
type Input struct {
	Name             string
	Description      string
	Content          string
	TriggerKeywords  []string
	ToolRestrictions []string
	FrontmatterName  *string
}

// Mode is the lint mode a writer runs in.
type Mode string

const (
	// ModeOff runs nothing: a write is exactly what it was before this lint.
	ModeOff Mode = "off"
	// ModeReport counts and logs findings; every write goes ahead.
	ModeReport Mode = "report"
	// ModeEnforce refuses a write with an error finding; warnings never refuse.
	ModeEnforce Mode = "enforce"
)

// ModeFromLookup reads SKILL_LINT_MODE strictly: only the exact words "report" and
// "enforce" turn the lint on, anything else leaves it off.
func ModeFromLookup(lookup func(string) string) Mode {
	switch value := lookup(ModeEnv); value {
	case string(ModeReport), string(ModeEnforce):
		return Mode(value)
	default:
		return ModeOff
	}
}

// Lint runs every rule over input and returns the findings, in rule order.
func Lint(input Input) []Finding {
	var broken []string
	if strings.TrimSpace(input.Name) == "" {
		broken = append(broken, CodeNameEmpty)
	} else {
		if utf8.RuneCountInString(input.Name) > MaxNameChars {
			broken = append(broken, CodeNameTooLong)
		}
		if strings.IndexFunc(input.Name, unicode.IsControl) >= 0 || strings.TrimSpace(input.Name) != input.Name {
			broken = append(broken, CodeNameCharset)
		}
	}

	if strings.TrimSpace(input.Description) == "" {
		broken = append(broken, CodeDescriptionMissing)
	}
	if utf8.RuneCountInString(input.Description) > MaxDescriptionChars {
		broken = append(broken, CodeDescriptionTooLong)
	}

	contentChars := utf8.RuneCountInString(input.Content)
	if strings.TrimSpace(input.Content) == "" {
		broken = append(broken, CodeContentEmpty)
	}
	if contentChars > MaxContentChars {
		broken = append(broken, CodeContentTooLong)
	} else if contentChars > ContentBudgetChars {
		broken = append(broken, CodeContentExceedsBudget)
	}
	if transcriptLines(input.Content) >= TranscriptMinLines {
		broken = append(broken, CodeTranscriptShape)
	}

	if len(input.TriggerKeywords) > MaxTriggerKeywords {
		broken = append(broken, CodeTriggerKeywordCount)
	}
	for _, keyword := range input.TriggerKeywords {
		if utf8.RuneCountInString(keyword) > MaxTriggerKeywordChars {
			broken = append(broken, CodeTriggerKeywordTooLong)
			break
		}
	}
	for _, tool := range input.ToolRestrictions {
		if !plainToolName(tool) {
			broken = append(broken, CodeToolRestrictionInvalid)
			break
		}
	}

	if input.FrontmatterName != nil && strings.TrimSpace(*input.FrontmatterName) != strings.TrimSpace(input.Name) {
		broken = append(broken, CodeFrontmatterNameMismatch)
	}

	findings := make([]Finding, 0, len(broken))
	for _, code := range broken {
		if rule, ok := RuleFor(code); ok {
			findings = append(findings, Finding{Code: rule.Code, Field: rule.Field, Severity: rule.Severity})
		}
	}
	return findings
}

// RuleFor returns the rule a code names.
func RuleFor(code string) (Rule, bool) {
	for _, rule := range Rules {
		if rule.Code == code {
			return rule, true
		}
	}
	return Rule{}, false
}

// HasError reports whether any finding refuses a write under enforce.
func HasError(findings []Finding) bool {
	for _, finding := range findings {
		if finding.Severity == SeverityError {
			return true
		}
	}
	return false
}

// transcriptLines counts the lines that start, after leading whitespace, with a speaker label.
func transcriptLines(content string) int {
	count := 0
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimLeftFunc(line, unicode.IsSpace)
		for _, prefix := range TranscriptPrefixes {
			if strings.HasPrefix(line, prefix) {
				count++
				break
			}
		}
	}
	return count
}

// plainToolName is one tool name matching [A-Za-z0-9_.-]+: no wildcard, no space, not empty.
func plainToolName(tool string) bool {
	if tool == "" {
		return false
	}
	for i := 0; i < len(tool); i++ {
		c := tool[i]
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '.' || c == '-') {
			return false
		}
	}
	return true
}
