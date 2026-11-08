package quarrycontracts

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
)

// The wire shape must equal what `AgentAction::FillOtpRef` emits in
// crates/quarry-core/src/contracts.rs (fill_otp_contract_tests).
func TestFillOtpRefAction_WireShape(t *testing.T) {
	raw, err := json.Marshal(NewFillOtpRefAction("snap", 3, "@e4", "g1"))
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	var got map[string]any
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	want := map[string]any{
		"type":              "fill_otp_ref",
		"snapshot_id":       "snap",
		"generation":        float64(3),
		"ref_id":            "@e4",
		"approval_grant_id": "g1",
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("wire = %v, want %v", got, want)
	}
}

func TestFillOtpRefAction_ZeroTypeIsPinned(t *testing.T) {
	raw, err := json.Marshal(FillOtpRefAction{SnapshotID: "s", Generation: 1, RefID: "@e1", ApprovalGrantID: "g"})
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if !strings.Contains(string(raw), `"type":"fill_otp_ref"`) {
		t.Fatalf("tag not pinned: %s", raw)
	}
}

func TestFillOtpRefAction_CarriesNoValue(t *testing.T) {
	in := []byte(`{"type":"fill_otp_ref","snapshot_id":"s","generation":1,"ref_id":"@e1","approval_grant_id":"g","value":"123456","code":"123456"}`)
	var parsed FillOtpRefAction
	if err := json.Unmarshal(in, &parsed); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	out, err := json.Marshal(parsed)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, leak := range []string{"123456", `"value"`, `"code"`} {
		if strings.Contains(string(out), leak) {
			t.Fatalf("%s on the wire: %s", leak, out)
		}
	}
	if SensitiveActionFillOtp != "fill_otp" {
		t.Fatalf("sensitive action name drifted: %q", SensitiveActionFillOtp)
	}
}
