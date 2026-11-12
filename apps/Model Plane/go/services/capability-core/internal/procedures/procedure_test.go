package procedures

import (
	"strings"
	"testing"
)

// roleLocator is the SemanticLocator a compiled semantic step carries: the serde shape of kind role.
const roleLocator = `{"kind":"role","role":"button","name":"Save","exact":true}`

// stepTarget is the target of a compiled semantic step: its locator, the fingerprint id Quarry matched
// and the tag. The fingerprint's normalised text is never copied, so it is not a key here.
const stepTarget = `"target":{"locator":` + roleLocator + `,"fingerprint_id":"fp_1","tag":"button"}`

// stepExpect is the postcondition Quarry verified on a compiled step.
const stepExpect = `"expect":{"all":[{"kind":"url_matches","pattern":"*/done"}]}`

// procedureBody wraps the step objects and the slot objects of one version 2 BrowserProcedure.
func procedureBody(steps, slots string) []byte {
	return []byte(`{"procedure_id":"proc_11111111-1111-1111-1111-111111111111","version":2,` +
		`"source_receipt_ids":["rcpt_1"],"steps":[` + steps + `],"slots":[` + slots + `]}`)
}

// assertRefused fails the test unless Validate refuses body with the given code.
func assertRefused(t *testing.T, body []byte, want Code) {
	t.Helper()
	_, err := Validate(body)
	if v, ok := AsValidation(err); !ok || v.Code != want {
		t.Fatalf("Validate = %v, want a refusal with %s", err, want)
	}
}

// The field the audit names: a typed value under a key other than text. The body is refused, and the
// refusal does not repeat the value.
func TestValidateRefusesATypedValueUnderAKeyOtherThanText(t *testing.T) {
	body := procedureBody(
		`{"step":1,"expected_outcome":"verified","action":{"type":"type","selector":"#pw","text":"","value":"hunter2"},"slot":"slot_1"}`,
		`{"name":"slot_1","step":1,"kind":"secret"}`)
	_, err := Validate(body)
	if v, ok := AsValidation(err); !ok || v.Code != CodeBadShape {
		t.Fatalf("Validate = %v, want a %s refusal", err, CodeBadShape)
	}
	if strings.Contains(err.Error(), "hunter2") {
		t.Fatalf("the refusal repeats the typed value: %v", err)
	}
}

// An action keeps only the fields of its own type. Text belongs to a typed action alone, and a press takes
// a key, not a value, so each body below carries one field its type does not have.
func TestValidateRefusesAnActionFieldItsTypeDoesNotHave(t *testing.T) {
	for _, step := range []string{
		`{"step":1,"action":{"type":"click","selector":"#go","text":"hunter2"}}`,
		`{"step":1,"action":{"type":"press","key":"Enter","value":"hunter2"}}`,
		`{"step":1,"action":{"type":"navigate","url":"https://example.test/","selector":"#pw"}}`,
		`{"step":1,"action":{"type":"back","text":"hunter2"}}`,
	} {
		assertRefused(t, procedureBody(step, ""), CodeBadShape)
	}
}

// A target and an expect take only the fields Quarry writes for them.
func TestValidateRefusesAnUnknownFieldInTargetOrExpect(t *testing.T) {
	action := `"action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `}`
	withTextInTarget := `{"step":1,` + action + `,"target":{"locator":` + roleLocator +
		`,"fingerprint_id":"fp_1","tag":"button","text":"hunter2"}}`
	withValueInExpect := `{"step":1,` + action + `,"expect":{"all":[{"kind":"url_matches","pattern":"*/done"}],"value":"hunter2"}}`
	for _, step := range []string{withTextInTarget, withValueInExpect} {
		assertRefused(t, procedureBody(step, ""), CodeBadShape)
	}
}

// A locator is stored whole, and Quarry's SemanticLocator has no deny_unknown_fields, so each kind's field set is
// closed, in an action and in a target alike. A typed value under a locator key is refused, and the refusal does
// not repeat it.
func TestValidateRefusesAnUnknownFieldInALocator(t *testing.T) {
	labelWithValue := `{"kind":"label","label":"Password","value":"hunter2"}`
	inAction := `{"step":1,"action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":` + labelWithValue + `}}`
	inTarget := `{"step":1,"action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `},` +
		`"target":{"locator":` + labelWithValue + `,"fingerprint_id":"fp_1","tag":"button"}}`
	for _, step := range []string{inAction, inTarget} {
		_, err := Validate(procedureBody(step, ""))
		if v, ok := AsValidation(err); !ok || v.Code != CodeBadShape {
			t.Fatalf("Validate = %v, want a %s refusal", err, CodeBadShape)
		}
		if strings.Contains(err.Error(), "hunter2") {
			t.Fatalf("the refusal repeats the typed value: %v", err)
		}
	}
}

// Each SemanticLocator kind Quarry writes is accepted with exactly the fields of that kind, and a kind outside the
// closed set is refused.
func TestValidateAcceptsEveryLocatorKindWithItsOwnFields(t *testing.T) {
	for _, loc := range []string{
		`{"kind":"role","role":"button","name":"Save","exact":true}`,
		`{"kind":"text","text":"Save","exact":false}`,
		`{"kind":"label","label":"Email","exact":true}`,
		`{"kind":"placeholder","placeholder":"Search","exact":false}`,
		`{"kind":"test_id","test_id":"save-button","exact":true}`,
		`{"kind":"nth","selector":"button","index":2}`,
	} {
		step := `{"step":1,"action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":` + loc + `}}`
		if _, err := Validate(procedureBody(step, "")); err != nil {
			t.Fatalf("Validate refused a %s locator: %v", loc, err)
		}
	}
	unknownKind := `{"step":1,"action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":{"kind":"css","selector":"#pw"}}}`
	assertRefused(t, procedureBody(unknownKind, ""), CodeBadShape)
}

// Every action shape the v2 compile writes is accepted, with the target and expect it carries.
func TestValidateAcceptsEveryActionShapeTheCompileWrites(t *testing.T) {
	cases := []struct {
		name, step, slots string
	}{
		{"navigate", `{"step":1,"action":{"type":"navigate","url":"https://example.test/"}}`, ""},
		{"click", `{"step":1,"action":{"type":"click","selector":"#go"}}`, ""},
		{"click_semantic", `{"step":1,"action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `},` + stepTarget + `,` + stepExpect + `}`, ""},
		{"type", `{"step":1,"action":{"type":"type","selector":"#q","text":""},"slot":"slot_1"}`, `{"name":"slot_1","step":1,"kind":"text"}`},
		{"type_semantic", `{"step":1,"action":{"type":"type_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `,"text":""},` + stepTarget + `,"slot":"slot_1"}`, `{"name":"slot_1","step":1,"kind":"secret"}`},
		{"select_semantic", `{"step":1,"action":{"type":"select_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `,"value":"NO"},` + stepTarget + `}`, ""},
		{"wait_for_semantic", `{"step":1,"action":{"type":"wait_for_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `,"timeout_ms":5000},` + stepTarget + `}`, ""},
		{"press", `{"step":1,"action":{"type":"press","key":"Enter"}}`, ""},
		{"scroll", `{"step":1,"action":{"type":"scroll","target":"down"}}`, ""},
		{"wait", `{"step":1,"action":{"type":"wait","ms":500}}`, ""},
		{"back", `{"step":1,"action":{"type":"back"}}`, ""},
		{"get_content", `{"step":1,"action":{"type":"get_content"}}`, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := Validate(procedureBody(tc.step, tc.slots)); err != nil {
				t.Fatalf("Validate refused a compiled %s step: %v", tc.name, err)
			}
		})
	}
}

// The field sets and the action vocabulary name the same types, so an allowed type cannot lack its closed
// field set, and a field set cannot outlive its type.
func TestActionKeysNameExactlyTheAllowedActionTypes(t *testing.T) {
	for typ := range allowedActionTypes {
		keys, ok := actionKeys[typ]
		if !ok {
			t.Errorf("action type %q has no closed field set", typ)
		} else if !keys["type"] {
			t.Errorf("action type %q does not allow its own type tag", typ)
		}
	}
	for typ := range actionKeys {
		if !allowedActionTypes[typ] {
			t.Errorf("field set for %q names a type outside the vocabulary", typ)
		}
	}
}
