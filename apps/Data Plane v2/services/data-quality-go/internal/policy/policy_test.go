package policy

import (
	"errors"
	"math"
	"strings"
	"testing"
)

func str(s string) *string   { return &s }
func num(f float64) *float64 { return &f }
func inherit() Input {
	return Input{ContextualRetrieval: Inherit, GraphExtraction: Inherit, BudgetMode: BudgetInherit}
}
func with(in Input, f func(*Input)) Input {
	f(&in)
	return in
}

// enforceEverything makes every part of the policy enforced for one test, as it will be once
// the budget gate (CR-T9b) reads its columns too (graph-index, CR-T12, already reads graph_extraction).
// Not parallel-safe, and nothing in this package runs in parallel.
func enforceEverything(t *testing.T) {
	t.Helper()
	before := Enforced
	Enforced = Enforcement{GraphExtraction: true, Budget: true}
	t.Cleanup(func() { Enforced = before })
}

func TestAnAbsentPolicyIsTheDefaultAndTheDefaultIsOn(t *testing.T) {
	record := Default("org-a")
	if record.Exists {
		t.Fatal("a default is not a stored row")
	}
	if record.ContextualRetrieval != Inherit || record.GraphExtraction != Inherit || record.BudgetMode != BudgetInherit {
		t.Fatalf("default = %+v, want every switch inherit", record.Policy)
	}
	if got := record.Effective(); got.ContextualRetrieval != "on" || got.GraphExtraction != "deployment_default" {
		t.Fatalf("effective default = %+v, want contextual retrieval on", got)
	}
}

func TestEffectiveReadsTheSwitches(t *testing.T) {
	cases := []struct {
		contextual, graph string
		wantContextual    string
		wantGraph         string
	}{
		{Inherit, Inherit, "on", "deployment_default"},
		{On, On, "on", "on"},
		{Off, Off, "off", "off"},
		{On, Off, "on", "off"},
	}
	for _, c := range cases {
		got := Policy{ContextualRetrieval: c.contextual, GraphExtraction: c.graph}.Effective()
		if got.ContextualRetrieval != c.wantContextual || got.GraphExtraction != c.wantGraph {
			t.Errorf("%s/%s = %+v, want %s/%s", c.contextual, c.graph, got, c.wantContextual, c.wantGraph)
		}
	}
}

func TestValidateAcceptsTheDocumentedShapes(t *testing.T) {
	enforceEverything(t)
	for name, in := range map[string]Input{
		"the default":        inherit(),
		"explicit on":        with(inherit(), func(i *Input) { i.ContextualRetrieval = On }),
		"off with a reason":  with(inherit(), func(i *Input) { i.ContextualRetrieval = Off; i.Reason = str("eval baseline organization") }),
		"graph off":          with(inherit(), func(i *Input) { i.GraphExtraction = Off; i.Reason = str("incident INC-1") }),
		"a pin":              with(inherit(), func(i *Input) { i.ContextPromptVersion = str("ctx-v2") }),
		"alert only":         with(inherit(), func(i *Input) { i.BudgetMode = BudgetAlertOnly; i.MonthlyBudgetUSD = num(0) }),
		"stop with a reason": with(inherit(), func(i *Input) { i.BudgetMode = BudgetStop; i.Reason = str("cap reached") }),
		"padded values":      {ContextualRetrieval: " on ", GraphExtraction: "\tinherit", BudgetMode: " throttle "},
	} {
		if _, err := in.Validate(); err != nil {
			t.Errorf("%s: %v", name, err)
		}
	}
}

func TestValidateRefusesWhatTheDatabaseOrTheEngineWouldRefuse(t *testing.T) {
	enforceEverything(t)
	for name, tc := range map[string]struct {
		in    Input
		field string
	}{
		"empty contextual":      {with(inherit(), func(i *Input) { i.ContextualRetrieval = "" }), "contextual_retrieval"},
		"unknown contextual":    {with(inherit(), func(i *Input) { i.ContextualRetrieval = "disabled" }), "contextual_retrieval"},
		"upper case":            {with(inherit(), func(i *Input) { i.ContextualRetrieval = "OFF" }), "contextual_retrieval"},
		"unknown graph":         {with(inherit(), func(i *Input) { i.GraphExtraction = "yes" }), "graph_extraction"},
		"unknown budget mode":   {with(inherit(), func(i *Input) { i.BudgetMode = "unlimited" }), "budget_mode"},
		"unknown prompt":        {with(inherit(), func(i *Input) { i.ContextPromptVersion = str("ctx-v3") }), "context_prompt_version"},
		"blank prompt":          {with(inherit(), func(i *Input) { i.ContextPromptVersion = str(" ") }), "context_prompt_version"},
		"negative budget":       {with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(-0.01) }), "monthly_budget_usd"},
		"NaN budget":            {with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(math.NaN()) }), "monthly_budget_usd"},
		"infinite budget":       {with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(math.Inf(1)) }), "monthly_budget_usd"},
		"absurd budget":         {with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(MaxMonthlyBudgetUSD * 2) }), "monthly_budget_usd"},
		"off without a reason":  {with(inherit(), func(i *Input) { i.ContextualRetrieval = Off }), "reason"},
		"off with a blank one":  {with(inherit(), func(i *Input) { i.ContextualRetrieval = Off; i.Reason = str("   ") }), "reason"},
		"graph off, no reason":  {with(inherit(), func(i *Input) { i.GraphExtraction = Off }), "reason"},
		"stop without a reason": {with(inherit(), func(i *Input) { i.BudgetMode = BudgetStop }), "reason"},
		"a newline in a reason": {with(inherit(), func(i *Input) { i.ContextualRetrieval = Off; i.Reason = str("a\nb") }), "reason"},
		"a tab in a reason":     {with(inherit(), func(i *Input) { i.ContextualRetrieval = Off; i.Reason = str("a\tb") }), "reason"},
		"an overlong reason":    {with(inherit(), func(i *Input) { i.ContextualRetrieval = Off; i.Reason = str(strings.Repeat("é", MaxReasonLength+1)) }), "reason"},
	} {
		_, err := tc.in.Validate()
		fe, ok := IsFieldError(err)
		if !ok {
			t.Errorf("%s: err = %v, want a field error", name, err)
			continue
		}
		if fe.Field != tc.field {
			t.Errorf("%s: refused for %q, want %q", name, fe.Field, tc.field)
		}
	}
}

// The longest reason is measured in characters, not bytes: a Norwegian reason is not
// penalised for its letters.
func TestAReasonOfTheMaximumLengthInCharactersIsAccepted(t *testing.T) {
	in := with(inherit(), func(i *Input) {
		i.ContextualRetrieval = Off
		i.Reason = str(strings.Repeat("å", MaxReasonLength))
	})
	if _, err := in.Validate(); err != nil {
		t.Fatalf("a %d-character reason was refused: %v", MaxReasonLength, err)
	}
}

func TestValidateNormalizesWithoutChangingMeaning(t *testing.T) {
	enforceEverything(t)
	in := Input{
		ContextualRetrieval:  " off ",
		GraphExtraction:      " inherit",
		BudgetMode:           "alert_only ",
		ContextPromptVersion: str(" ctx-v1 "),
		MonthlyBudgetUSD:     num(10.5),
		Reason:               str("  planned baseline  "),
	}
	out, err := in.Validate()
	if err != nil {
		t.Fatal(err)
	}
	if out.ContextualRetrieval != Off || out.GraphExtraction != Inherit || out.BudgetMode != BudgetAlertOnly {
		t.Fatalf("switches not trimmed: %+v", out)
	}
	if *out.ContextPromptVersion != "ctx-v1" || *out.Reason != "planned baseline" || *out.MonthlyBudgetUSD != 10.5 {
		t.Fatalf("values not normalized: %+v", out)
	}
	// The caller's input is not modified.
	if in.ContextualRetrieval != " off " || *in.Reason != "  planned baseline  " {
		t.Fatalf("Validate changed its receiver: %+v", in)
	}
}

// An empty reason on a policy that needs none is no reason, not an empty string in the audit trail.
func TestABlankReasonOnAPolicyThatNeedsNoneIsDropped(t *testing.T) {
	out, err := with(inherit(), func(i *Input) { i.Reason = str("  ") }).Validate()
	if err != nil || out.Reason != nil {
		t.Fatalf("out = %+v err = %v, want no reason and no error", out, err)
	}
}

func TestOnlyTheFullDefaultIsAReset(t *testing.T) {
	if !inherit().IsDefault() {
		t.Fatal("every switch inherit, no pin, no budget is the default")
	}
	// A reason alone does not make a policy: the default keeps no row to hold it.
	if !with(inherit(), func(i *Input) { i.Reason = str("incident over") }).IsDefault() {
		t.Fatal("a reason on the default is still the default")
	}
	for name, in := range map[string]Input{
		"on is explicit":    with(inherit(), func(i *Input) { i.ContextualRetrieval = On }),
		"graph on":          with(inherit(), func(i *Input) { i.GraphExtraction = On }),
		"a pin":             with(inherit(), func(i *Input) { i.ContextPromptVersion = str("ctx-v1") }),
		"a budget mode":     with(inherit(), func(i *Input) { i.BudgetMode = BudgetAlertOnly }),
		"a budget figure":   with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(0) }),
		"contextual is off": with(inherit(), func(i *Input) { i.ContextualRetrieval = Off }),
	} {
		if in.IsDefault() {
			t.Errorf("%s was taken for the default", name)
		}
	}
}

// An operator control that stores a value nothing honours is a silent no-op: an operator who sets
// graph_extraction to off believes graph extraction stopped. What no consumer reads yet is refused,
// naming the task that will read it; what the embedding engine and graph-index read today is accepted.
func TestPartsNothingEnforcesYetAreRefusedNotStoredAsNoOps(t *testing.T) {
	if !Enforced.GraphExtraction || Enforced.Budget {
		t.Fatalf("Enforced = %+v: graph-index reads graph_extraction (CR-T12) and nothing reads the budget yet (CR-T9b); flip a flag only in the change that makes its consumer read the column", Enforced)
	}
	for name, tc := range map[string]struct {
		in    Input
		field string
		task  string
	}{
		"alert only":           {with(inherit(), func(i *Input) { i.BudgetMode = BudgetAlertOnly }), "budget_mode", "CR-T9b"},
		"stop":                 {with(inherit(), func(i *Input) { i.BudgetMode = BudgetStop; i.Reason = str("cap") }), "budget_mode", "CR-T9b"},
		"a budget figure":      {with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(10) }), "monthly_budget_usd", "CR-T9b"},
		"a zero budget figure": {with(inherit(), func(i *Input) { i.MonthlyBudgetUSD = num(0) }), "monthly_budget_usd", "CR-T9b"},
	} {
		_, err := tc.in.Validate()
		fe, ok := IsFieldError(err)
		if !ok || fe.Field != tc.field || !strings.Contains(fe.Message, tc.task) || !strings.Contains(fe.Message, "silent no-op") {
			t.Errorf("%s: err = %v, want a refusal of %s that names %s and says why", name, err, tc.field, tc.task)
		}
	}
	// What the engine honours today is accepted.
	for name, in := range map[string]Input{
		"contextual off with a reason": with(inherit(), func(i *Input) { i.ContextualRetrieval = Off; i.Reason = str("eval baseline organization") }),
		"contextual on":                with(inherit(), func(i *Input) { i.ContextualRetrieval = On }),
		"a prompt pin":                 with(inherit(), func(i *Input) { i.ContextPromptVersion = str("ctx-v2") }),
		"graph off with a reason":      with(inherit(), func(i *Input) { i.GraphExtraction = Off; i.Reason = str("incident") }),
		"graph on":                     with(inherit(), func(i *Input) { i.GraphExtraction = On }),
	} {
		if _, err := in.Validate(); err != nil {
			t.Errorf("%s was refused: %v", name, err)
		}
	}
}

func TestOrganizationIDsAreNarrow(t *testing.T) {
	for _, ok := range []string{"org-a", "org_a", "Org.1", "org:eu:1", "a", "org-corpus-baseline", "0f2f8c9e-6c5d-4b8a-9e0e-1d2c3b4a5f60", strings.Repeat("a", MaxOrgIDLength)} {
		if !ValidOrgID(ok) {
			t.Errorf("%q was refused", ok)
		}
	}
	for _, bad := range []string{"", " ", "-a", "_a", ".a", ":a", "a b", "a/b", "a\\b", "a%20b", "a\nb", "a\x00b", "a'b", "a;b", "åre", strings.Repeat("a", MaxOrgIDLength+1)} {
		if ValidOrgID(bad) {
			t.Errorf("%q was accepted", bad)
		}
	}
}

// The prompt versions the API accepts are the ones the embedding engine knows
// (embedding-engine-rs src/context/prompt.rs, PromptVersion::ALL, pinned there as well).
func TestThePromptVersionsMatchTheEngine(t *testing.T) {
	if strings.Join(PromptVersions, ",") != "ctx-v1,ctx-v2" {
		t.Fatalf("PromptVersions = %v: change it together with the engine's PromptVersion::ALL", PromptVersions)
	}
}

func TestFieldErrorsAreRecognizedThroughWrapping(t *testing.T) {
	_, err := with(inherit(), func(i *Input) { i.BudgetMode = "x" }).Validate()
	wrapped := errors.Join(errors.New("context"), err)
	if _, ok := IsFieldError(wrapped); !ok {
		t.Fatal("a wrapped field error was not recognized")
	}
	if _, ok := IsFieldError(errors.New("plain")); ok {
		t.Fatal("a plain error was taken for a field error")
	}
}
