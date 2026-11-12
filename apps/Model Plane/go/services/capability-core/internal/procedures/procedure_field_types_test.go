package procedures

import (
	"strings"
	"testing"
)

// A closed field holds the JSON type Quarry's serde field expects. A typed value under a string, flag or number field
// is refused with CodeBadShape, and so is a number past u32 or an expected_outcome that is not one of its tokens. The
// refusal names the field and never the value.
func TestValidateRefusesAValueOfTheWrongTypeUnderAClosedField(t *testing.T) {
	const typed = `{"typed":"hunter2"}`
	for _, step := range []string{
		`{"step":1,"expected_outcome":"verified","action":{"type":"navigate","url":` + typed + `}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":` + typed + `,"locator":` + roleLocator + `}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"wait","ms":` + typed + `}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"wait","ms":4294967296}}`,
		`{"step":4294967296,"expected_outcome":"verified","action":{"type":"back"}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":{"kind":"text","text":"x","exact":` + typed + `}}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":{"kind":"nth","selector":"button","index":` + typed + `}}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":` + roleLocator + `},` +
			`"target":{"locator":` + roleLocator + `,"fingerprint_id":` + typed + `,"tag":"button"}}`,
		`{"step":1,"expected_outcome":"verified","action":{"type":"back"},"expect":{"all":[{"kind":"url_matches","pattern":` + typed + `}]}}`,
		`{"step":1,"expected_outcome":` + typed + `,"action":{"type":"back"}}`,
		`{"step":1,"expected_outcome":"maybe","action":{"type":"back"}}`,
	} {
		_, err := Validate(procedureBody(step, ""))
		if v, ok := AsValidation(err); !ok || v.Code != CodeBadShape {
			t.Fatalf("Validate = %v, want a %s refusal for a body with a field of the wrong type", err, CodeBadShape)
		}
		if strings.Contains(err.Error(), "hunter2") {
			t.Fatalf("the refusal repeats the typed value: %v", err)
		}
	}
}

// The u32 fields take their largest value, and expected_outcome takes each of its three tokens.
func TestValidateAcceptsTheU32LimitAndEveryOutcomeToken(t *testing.T) {
	for _, step := range []string{
		`{"step":4294967295,"expected_outcome":"verified","action":{"type":"wait","ms":4294967295}}`,
		`{"step":0,"expected_outcome":"failed","action":{"type":"back"}}`,
		`{"step":0,"expected_outcome":"unknown","action":{"type":"back"}}`,
	} {
		if _, err := Validate(procedureBody(step, "")); err != nil {
			t.Fatalf("Validate refused a valid step %s: %v", step, err)
		}
	}
}

// A typed step's text is refused with CodeTypedText whatever its JSON type, as it was before the field types were checked.
func TestValidateKeepsTypedTextOnItsOwnCode(t *testing.T) {
	for _, text := range []string{`"hunter2"`, `{"typed":"hunter2"}`} {
		step := `{"step":1,"expected_outcome":"verified","action":{"type":"type","selector":"#q","text":` + text + `},"slot":"slot_1"}`
		_, err := Validate(procedureBody(step, `{"name":"slot_1","step":1,"kind":"text"}`))
		if v, ok := AsValidation(err); !ok || v.Code != CodeTypedText {
			t.Fatalf("Validate = %v, want a %s refusal", err, CodeTypedText)
		}
		if strings.Contains(err.Error(), "hunter2") {
			t.Fatalf("the refusal repeats the typed value: %v", err)
		}
	}
}
