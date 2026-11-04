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
//
// EP-11 S6 adds the authored kinds template and preset to the same table and the same lint:
// a template may hold no placeholder but {{input}}, and a preset's settings are a closed
// object of visible composer controls.
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

// The authored kinds (EP-11 S6). An empty kind reads as KindSkill.
const (
	KindSkill    = "skill"
	KindTemplate = "template"
	KindPreset   = "preset"
)

// Kinds is every authored kind, in the contract's order.
var Kinds = []string{KindSkill, KindTemplate, KindPreset}

// TemplatePlaceholder is the one placeholder a template may hold: the composer fills it with
// the rest of the / line. Every "{{" in a template must start it.
const TemplatePlaceholder = "{{input}}"

// PresetSettingsKeys are the keys a preset's settings may hold, each one a control the person
// could set by hand: modeId (text), browseWeb (bool), planMode (must be true: a console preset
// never turns planning off), skillIds (at most MaxPresetSkills texts) and templateId (text).
var PresetSettingsKeys = []string{"modeId", "browseWeb", "planMode", "skillIds", "templateId"}

// MaxPresetSkills is the most skills one preset may pick (MAX_PICKED_SKILLS in the composer).
const MaxPresetSkills = 4

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
	CodeTemplatePlaceholder     = "template_placeholder_invalid"
	CodePresetSettingsInvalid   = "preset_settings_invalid"
	CodePresetUnknownKey        = "preset_unknown_key"
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
	{CodeTemplatePlaceholder, "content", SeverityError, 0},
	{CodePresetSettingsInvalid, "settings", SeverityError, MaxPresetSkills},
	{CodePresetUnknownKey, "settings", SeverityError, 0},
}

// Finding is one broken rule as a caller sees it: the code, the field and the severity.
type Finding struct {
	Code     string   `json:"code"`
	Field    string   `json:"field"`
	Severity Severity `json:"severity"`
}

// Input is the skill text a write would store. FrontmatterName is the name in a bundle's
// SKILL.md front matter; nil unless a bundle is checked. Kind is the authored kind (empty
// reads as skill); Settings is a preset's settings as decoded JSON, nil when absent.
type Input struct {
	Name             string
	Description      string
	Content          string
	TriggerKeywords  []string
	ToolRestrictions []string
	FrontmatterName  *string
	Kind             string
	Settings         any
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

	if input.Kind == KindTemplate && !templatePlaceholdersValid(input.Content) {
		broken = append(broken, CodeTemplatePlaceholder)
	}
	if input.Kind == KindPreset {
		invalid, unknownKey := presetSettingsFindings(input.Settings)
		if invalid {
			broken = append(broken, CodePresetSettingsInvalid)
		}
		if unknownKey {
			broken = append(broken, CodePresetUnknownKey)
		}
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

// KindRuleFindings are the findings of the template and preset rules alone. capability-core
// runs them on every template or preset write whatever SKILL_LINT_MODE says: no stored row
// predates them, so there is nothing to grandfather.
func KindRuleFindings(findings []Finding) []Finding {
	var kindRules []Finding
	for _, finding := range findings {
		switch finding.Code {
		case CodeTemplatePlaceholder, CodePresetSettingsInvalid, CodePresetUnknownKey:
			kindRules = append(kindRules, finding)
		}
	}
	return kindRules
}

// ValidKind reports whether kind is one of Kinds.
func ValidKind(kind string) bool {
	for _, known := range Kinds {
		if kind == known {
			return true
		}
	}
	return false
}

// templatePlaceholdersValid is true when every position where "{{" occurs, overlapping ones
// included, starts "{{input}}", so "{{ input }}", "{{name}}" and "{{{input}}}" are refused.
func templatePlaceholdersValid(content string) bool {
	for i := 0; i+1 < len(content); i++ {
		if content[i] == '{' && content[i+1] == '{' && !strings.HasPrefix(content[i:], TemplatePlaceholder) {
			return false
		}
	}
	return true
}

// presetSettingsFindings checks a preset's settings against the closed schema: whether a known
// key has the wrong type or value, and whether a key is outside PresetSettingsKeys. Absent
// (nil) settings are an empty object; settings that are not an object are invalid.
func presetSettingsFindings(settings any) (invalid, unknownKey bool) {
	if settings == nil {
		return false, false
	}
	object, ok := settings.(map[string]any)
	if !ok {
		return true, false
	}
	for key, value := range object {
		switch key {
		case "modeId", "templateId":
			_, isText := value.(string)
			invalid = invalid || !isText
		case "browseWeb":
			_, isBool := value.(bool)
			invalid = invalid || !isBool
		case "planMode":
			planMode, isBool := value.(bool)
			invalid = invalid || !isBool || !planMode
		case "skillIds":
			invalid = invalid || !textList(value, MaxPresetSkills)
		default:
			unknownKey = true
		}
	}
	return invalid, unknownKey
}

// textList is true for a JSON list of at most limit texts.
func textList(value any, limit int) bool {
	items, ok := value.([]any)
	if !ok || len(items) > limit {
		return false
	}
	for _, item := range items {
		if _, isText := item.(string); !isText {
			return false
		}
	}
	return true
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
