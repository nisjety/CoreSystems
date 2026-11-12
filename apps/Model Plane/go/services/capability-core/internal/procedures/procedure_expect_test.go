package procedures

import (
	"strings"
	"testing"
)

// conditionBody wraps one expect condition, already encoded as JSON, in a version 2 body with one back step.
func conditionBody(condition string) []byte {
	return []byte(`{"version":2,"steps":[{"step":0,"action":{"type":"back"},"expect":{"all":[` + condition + `]}}]}`)
}

// Quarry writes each expect condition with exactly the fields of its kind, and writes a text condition's within as
// null when it names no element. Each of these conditions is accepted.
func TestValidateAcceptsEveryExpectConditionShapeQuarryWrites(t *testing.T) {
	const role = `{"kind":"role","role":"button","name":"Save","exact":true}`
	for _, condition := range []string{
		`{"kind":"url_matches","pattern":"*/done"}`,
		`{"kind":"element_present","locator":` + role + `}`,
		`{"kind":"element_absent","locator":{"kind":"label","label":"Email","exact":true}}`,
		`{"kind":"text_present","text":"Saved","within":null}`,
		`{"kind":"text_absent","text":"Error"}`,
		`{"kind":"text_absent","text":"Error","within":` + role + `}`,
	} {
		if _, err := Validate(conditionBody(condition)); err != nil {
			t.Fatalf("Validate refused a compiled expect condition: %v", err)
		}
	}
}

// A condition keeps only the fields of its kind, and its locator and within are closed as locators, so a typed value
// under any of those keys is refused. The refusal names the step and never repeats the value. A non-array all, a
// missing all and a condition that is not an object are refused too.
func TestValidateClosesEveryExpectConditionToItsKind(t *testing.T) {
	const role = `{"kind":"role","role":"button","name":"Save","exact":true}`
	for _, condition := range []string{
		`{"kind":"url_matches","pattern":"*","typed":"hunter2"}`,
		`{"kind":"element_present","locator":` + role + `,"typed":"hunter2"}`,
		`{"kind":"element_absent","locator":{"kind":"label","label":"Password","value":"hunter2"}}`,
		`{"kind":"text_present","text":"Saved","within":{"kind":"role","role":"dialog","value":"hunter2"}}`,
		`{"kind":"text_present","text":"Saved","within":"hunter2"}`,
		`{"kind":"text_absent","text":"Error","locator":` + role + `}`,
		`{"kind":"element_present","locator":null}`,
		`{"kind":"eval","script":"hunter2"}`,
		`"hunter2"`,
	} {
		_, err := Validate(conditionBody(condition))
		if v, ok := AsValidation(err); !ok || v.Code != CodeBadShape {
			t.Fatalf("Validate = %v, want a %s refusal", err, CodeBadShape)
		}
		if strings.Contains(err.Error(), "hunter2") {
			t.Fatalf("the refusal repeats the typed value: %v", err)
		}
	}
	for _, body := range []string{
		`{"version":2,"steps":[{"step":0,"action":{"type":"back"},"expect":{"all":"hunter2"}}]}`,
		`{"version":2,"steps":[{"step":0,"action":{"type":"back"},"expect":{}}]}`,
	} {
		if _, err := Validate([]byte(body)); err == nil {
			t.Fatalf("Validate accepted a body whose expect is not a list of conditions")
		} else if v, ok := AsValidation(err); !ok || v.Code != CodeBadShape {
			t.Fatalf("Validate = %v, want a %s refusal", err, CodeBadShape)
		}
	}
}
