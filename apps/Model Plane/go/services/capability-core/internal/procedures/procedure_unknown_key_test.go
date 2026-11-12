package procedures

import (
	"strings"
	"testing"
)

// An unknown key can hold a typed value, so a refusal never repeats it. Each closed object (the top level, a step,
// an action, a target and a slot) refuses its own unknown key, and the refusal names no key.
func TestValidateRefusesAnUnknownKeyWithoutRepeatingIt(t *testing.T) {
	const key = "hunter2"
	for _, tc := range []struct {
		name string
		body []byte
		want Code
	}{
		{
			"top level",
			[]byte(`{"procedure_id":"proc_11111111-1111-1111-1111-111111111111","version":2,"source_receipt_ids":["rcpt_1"],` +
				`"` + key + `":true,"steps":[{"step":1,"expected_outcome":"verified","action":{"type":"back"}}]}`),
			CodeBadShape,
		},
		{
			"step",
			procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"back"},"`+key+`":true}`, ""),
			CodeBadShape,
		},
		{
			"action",
			procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"back","`+key+`":true}}`, ""),
			CodeBadShape,
		},
		{
			"target",
			procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":`+
				roleLocator+`},"target":{"locator":`+roleLocator+`,"fingerprint_id":"fp_1","tag":"button","`+key+`":true}}`, ""),
			CodeBadShape,
		},
		{
			"slot",
			procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"type","selector":"#q","text":""},"slot":"slot_1"}`,
				`{"name":"slot_1","step":1,"kind":"text","`+key+`":true}`),
			CodeBadSlot,
		},
	} {
		t.Run(tc.name, func(t *testing.T) {
			_, err := Validate(tc.body)
			if v, ok := AsValidation(err); !ok || v.Code != tc.want {
				t.Fatalf("Validate = %v, want a %s refusal", err, tc.want)
			}
			if strings.Contains(err.Error(), key) {
				t.Fatalf("the refusal repeats the unknown key: %v", err)
			}
		})
	}
}
