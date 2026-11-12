package procedures

import "testing"

// A field with no serde default is required by Quarry: a stored body that leaves one out cannot be replayed. Each body
// below is complete except for the one field its name says it lacks. A missing field of a step, action, target, expect,
// locator or condition is CodeBadShape; a missing field of a slot is CodeBadSlot.
func TestValidateRefusesABodyThatLeavesOutARequiredField(t *testing.T) {
	const back = `{"step":1,"expected_outcome":"verified","action":{"type":"back"}}`
	for _, tc := range []struct {
		name string
		body []byte
		want Code
	}{
		{"no procedure_id", []byte(`{"version":2,"source_receipt_ids":["rcpt_1"],"steps":[` + back + `]}`), CodeBadShape},
		{"no source_receipt_ids", []byte(`{"procedure_id":"proc_11111111-1111-1111-1111-111111111111","version":2,"steps":[` + back + `]}`), CodeBadShape},
		{"no step number", procedureBody(`{"expected_outcome":"verified","action":{"type":"back"}}`, ""), CodeBadShape},
		{"no expected_outcome", procedureBody(`{"step":1,"action":{"type":"back"}}`, ""), CodeBadShape},
		{"no url on a navigate", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"navigate"}}`, ""), CodeBadShape},
		{"no text on a type", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"type","selector":"#q"},"slot":"slot_1"}`, `{"name":"slot_1","step":1,"kind":"text"}`), CodeBadShape},
		{"no locator on a click_semantic", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0}}`, ""), CodeBadShape},
		{"no fingerprint_id on a target", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"back"},"target":{"locator":`+roleLocator+`,"tag":"button"}}`, ""), CodeBadShape},
		{"no pattern on a url condition", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"back"},"expect":{"all":[{"kind":"url_matches"}]}}`, ""), CodeBadShape},
		{"no text on a text condition", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"back"},"expect":{"all":[{"kind":"text_present","within":null}]}}`, ""), CodeBadShape},
		{"no role on a role locator", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":{"kind":"role","name":"Save","exact":true}}}`, ""), CodeBadShape},
		{"no index on an nth locator", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":{"kind":"nth","selector":"button"}}}`, ""), CodeBadShape},
		{"no step on a slot", procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"type","selector":"#q","text":""},"slot":"slot_1"}`, `{"name":"slot_1","kind":"text"}`), CodeBadSlot},
	} {
		t.Run(tc.name, func(t *testing.T) {
			assertRefused(t, tc.body, tc.want)
		})
	}
}

// The fields Quarry leaves out when they have a serde default or an Option are optional, so a body that omits them is
// accepted: the slots list, a step's target, slot, expect and redacted_input, a role locator's name and exact, and a
// text condition's within.
func TestValidateAcceptsABodyWithoutItsOptionalFields(t *testing.T) {
	for _, body := range [][]byte{
		[]byte(`{"procedure_id":"proc_11111111-1111-1111-1111-111111111111","version":2,"source_receipt_ids":["rcpt_1"],` +
			`"steps":[{"step":1,"expected_outcome":"verified","action":{"type":"back"}}]}`),
		procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"click_semantic","snapshot_id":"","generation":0,"locator":{"kind":"role","role":"button"}}}`, ""),
		procedureBody(`{"step":1,"expected_outcome":"verified","action":{"type":"back"},"expect":{"all":[{"kind":"text_present","text":"Saved"}]}}`, ""),
	} {
		if _, err := Validate(body); err != nil {
			t.Fatalf("Validate refused a body without its optional fields: %v", err)
		}
	}
}
