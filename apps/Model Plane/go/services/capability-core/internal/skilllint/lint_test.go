package skilllint

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"reflect"
	"strings"
	"testing"
)

// The contract files are read where they are written, beside the other Model Plane
// contracts; mp-contracts skill_lint reads the same three files.
const (
	contractPath  = "../../../../../api/contracts/authored-behaviour-lint.json"
	pinPath       = "../../../../../api/contracts/authored-behaviour-lint.json.sha256"
	goldenPath    = "../../../../../api/contracts/authored-behaviour-lint-golden.json"
	goldenPinPath = "../../../../../api/contracts/authored-behaviour-lint-golden.json.sha256"
)

func readContract(t *testing.T, path string) []byte {
	t.Helper()
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return raw
}

// The Rust test computes the same digest over the same file, so a rule changed in the file
// without both readers being retested fails in both languages.
func TestLintContractDigestIsThePinnedOne(t *testing.T) {
	normalised := bytes.ReplaceAll(readContract(t, contractPath), []byte("\r\n"), []byte("\n"))
	sum := sha256.Sum256(normalised)
	pinned := strings.Fields(string(readContract(t, pinPath)))
	if len(pinned) == 0 || hex.EncodeToString(sum[:]) != pinned[0] {
		t.Fatalf("authored-behaviour-lint.json changed without its .sha256 pin being refreshed")
	}
}

func TestLintGoldenDigestIsThePinnedOne(t *testing.T) {
	normalised := bytes.ReplaceAll(readContract(t, goldenPath), []byte("\r\n"), []byte("\n"))
	sum := sha256.Sum256(normalised)
	pinned := strings.Fields(string(readContract(t, goldenPinPath)))
	if len(pinned) == 0 || hex.EncodeToString(sum[:]) != pinned[0] {
		t.Fatalf("authored-behaviour-lint-golden.json changed without its .sha256 pin being refreshed")
	}
}

func TestLintRulesAreTheContractRulesInOrder(t *testing.T) {
	var contract struct {
		Mode struct {
			Env string `json:"env"`
		} `json:"mode"`
		Refusal struct {
			HTTPCode       string `json:"http_code"`
			HTTPDetailsKey string `json:"http_details_key"`
		} `json:"refusal"`
		TranscriptPrefixes []string `json:"transcript_prefixes"`
		TranscriptMinLines int      `json:"transcript_min_lines"`
		ToolNamePattern    string   `json:"tool_name_pattern"`
		Kinds              []string `json:"kinds"`
		Placeholder        string   `json:"template_placeholder"`
		PresetSettingsKeys []string `json:"preset_settings_keys"`
		Rules              []struct {
			Code     string `json:"code"`
			Field    string `json:"field"`
			Severity string `json:"severity"`
			Limit    *int   `json:"limit"`
		} `json:"rules"`
	}
	if err := json.Unmarshal(readContract(t, contractPath), &contract); err != nil {
		t.Fatalf("decode contract: %v", err)
	}
	if len(contract.Rules) != len(Rules) {
		t.Fatalf("contract has %d rules, Go has %d: a rule was added to one side only", len(contract.Rules), len(Rules))
	}
	for i, row := range contract.Rules {
		limit := 0
		if row.Limit != nil {
			limit = *row.Limit
		}
		got := Rule{Code: row.Code, Field: row.Field, Severity: Severity(row.Severity), Limit: limit}
		if got != Rules[i] {
			t.Errorf("rule %d: contract %+v, Go %+v", i, got, Rules[i])
		}
	}
	if contract.Mode.Env != ModeEnv || contract.Refusal.HTTPCode != ErrorCode || contract.Refusal.HTTPDetailsKey != "findings" {
		t.Errorf("switch or refusal drifted: %+v %+v", contract.Mode, contract.Refusal)
	}
	if !reflect.DeepEqual(contract.TranscriptPrefixes, TranscriptPrefixes) || contract.TranscriptMinLines != TranscriptMinLines {
		t.Errorf("transcript rule drifted: %v %d", contract.TranscriptPrefixes, contract.TranscriptMinLines)
	}
	if contract.ToolNamePattern != "^[A-Za-z0-9_.-]+$" {
		t.Errorf("tool name pattern drifted: %s", contract.ToolNamePattern)
	}
	if !reflect.DeepEqual(contract.Kinds, Kinds) || contract.Placeholder != TemplatePlaceholder ||
		!reflect.DeepEqual(contract.PresetSettingsKeys, PresetSettingsKeys) {
		t.Errorf("kind rules drifted: %v %q %v", contract.Kinds, contract.Placeholder, contract.PresetSettingsKeys)
	}
}

func TestLintGoldenParity(t *testing.T) {
	var golden struct {
		Cases []struct {
			ID    string `json:"id"`
			Input struct {
				Name             string   `json:"name"`
				Description      string   `json:"description"`
				Content          string   `json:"content"`
				TriggerKeywords  []string `json:"trigger_keywords"`
				ToolRestrictions []string `json:"tool_restrictions"`
				FrontmatterName  *string  `json:"frontmatter_name"`
				Kind             string   `json:"kind"`
				Settings         any      `json:"settings"`
			} `json:"input"`
			Repeat *struct {
				Field string `json:"field"`
				Text  string `json:"text"`
				Count int    `json:"count"`
			} `json:"repeat"`
			Expect []string `json:"expect"`
		} `json:"cases"`
	}
	if err := json.Unmarshal(readContract(t, goldenPath), &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if len(golden.Cases) == 0 {
		t.Fatal("the golden has no cases")
	}
	for _, c := range golden.Cases {
		input := Input{
			Name: c.Input.Name, Description: c.Input.Description, Content: c.Input.Content,
			TriggerKeywords: c.Input.TriggerKeywords, ToolRestrictions: c.Input.ToolRestrictions,
			FrontmatterName: c.Input.FrontmatterName,
			Kind:            c.Input.Kind, Settings: c.Input.Settings,
		}
		if c.Repeat != nil {
			text := strings.Repeat(c.Repeat.Text, c.Repeat.Count)
			switch c.Repeat.Field {
			case "name":
				input.Name = text
			case "description":
				input.Description = text
			case "content":
				input.Content = text
			default:
				t.Fatalf("case %s repeats unknown field %q", c.ID, c.Repeat.Field)
			}
		}
		got := []string{}
		for _, finding := range Lint(input) {
			got = append(got, finding.Code)
		}
		want := c.Expect
		if want == nil {
			want = []string{}
		}
		if !reflect.DeepEqual(got, want) {
			t.Errorf("case %s: got %v, want %v", c.ID, got, want)
		}
	}
}

func TestLintModeIsOnlyTheExactWords(t *testing.T) {
	for raw, want := range map[string]Mode{
		"report": ModeReport, "enforce": ModeEnforce,
		"": ModeOff, "off": ModeOff, "true": ModeOff, "Enforce": ModeOff, " report": ModeOff,
	} {
		if got := ModeFromLookup(func(string) string { return raw }); got != want {
			t.Errorf("SKILL_LINT_MODE=%q = %q, want %q", raw, got, want)
		}
	}
}

func TestLintFindingsCarrySeverityAndNoText(t *testing.T) {
	findings := Lint(Input{Name: "Report", Content: "secret steps", ToolRestrictions: []string{"*"}})
	want := []Finding{
		{Code: CodeDescriptionMissing, Field: "description", Severity: SeverityWarn},
		{Code: CodeToolRestrictionInvalid, Field: "tool_restrictions", Severity: SeverityError},
	}
	if !reflect.DeepEqual(findings, want) {
		t.Fatalf("findings = %+v, want %+v", findings, want)
	}
	if !HasError(findings) || HasError(findings[:1]) {
		t.Fatal("only an error-severity finding refuses")
	}
	encoded, _ := json.Marshal(findings)
	if strings.Contains(string(encoded), "secret") {
		t.Fatalf("a finding must not carry the text: %s", encoded)
	}
}

// EP-11 S6: the kind rules read nothing on a skill, so a skill whose text happens to hold
// {{name}} or a stray settings object is linted as before; KindRuleFindings keeps only them.
func TestLintKindRulesSkipSkillsAndFilter(t *testing.T) {
	settings := map[string]any{"planMode": false, "persona": "x"}
	if findings := Lint(Input{Name: "Report", Description: "d", Content: "Hello {{name}}", Settings: settings}); len(findings) != 0 {
		t.Fatalf("a skill got kind findings: %+v", findings)
	}
	findings := Lint(Input{Name: "Report", Content: "c", Kind: KindPreset, Settings: settings})
	got := []string{}
	for _, finding := range KindRuleFindings(findings) {
		got = append(got, finding.Code)
	}
	if want := []string{CodePresetSettingsInvalid, CodePresetUnknownKey}; !reflect.DeepEqual(got, want) {
		t.Fatalf("kind findings = %v, want %v (description_missing is not a kind rule)", got, want)
	}
	for kind, want := range map[string]bool{"skill": true, "template": true, "preset": true, "": false, "agent": false, "Skill": false} {
		if ValidKind(kind) != want {
			t.Errorf("ValidKind(%q) = %v", kind, !want)
		}
	}
}
