package mpv1

import (
	"bytes"
	"encoding/hex"
	"testing"

	"google.golang.org/protobuf/proto"
)

const goldenEventWireHex = "0a1a3031485637413451324d37533545394a365238433254315a345810501801220b08f1f8dacf0610c0a9d33a2a0d6d6f64656c2d676174657761793208636f72722d3030313a0963617573652d30303142077265712d3030314a1a3031484f52473031323334353637383930414243444546474849521a30314855535230313233343536373839304142434445464748495a207468726561642f3031485448524541443031323334353637383930414243444562330a2d747970652e676f6f676c65617069732e636f6d2f6d6f64656c5f706c616e652e76312e52756e5374617274656412027b7d"

func TestEventDecodeFromGoldenHexRED(t *testing.T) {
	wire, err := hex.DecodeString(goldenEventWireHex)
	if err != nil {
		t.Fatalf("decode hex: %v", err)
	}

	var got Event
	if err := proto.Unmarshal(wire, &got); err != nil {
		t.Fatalf("unmarshal event from golden bytes: %v", err)
	}

	if got.GetEventId() != "01HV7A4Q2M7S5E9J6R8C2T1Z4X" {
		t.Fatalf("event_id mismatch: got %q", got.GetEventId())
	}
	if got.GetEventType() != EventType_EVENT_TYPE_INGRESS_ACCEPTED {
		t.Fatalf("event_type mismatch: got %v", got.GetEventType())
	}
	if got.GetProducer() != "model-gateway" {
		t.Fatalf("producer mismatch: got %q", got.GetProducer())
	}
	if got.GetIdempotencyKey() != "req-001" {
		t.Fatalf("idempotency_key mismatch: got %q", got.GetIdempotencyKey())
	}
	if got.GetResourceRef() != "thread/01HTHREAD01234567890ABCDE" {
		t.Fatalf("resource_ref mismatch: got %q", got.GetResourceRef())
	}
}

func TestEventDecodeRejectsTruncatedPayload(t *testing.T) {
	badWire := []byte{0x0A, 0x02, 0x41}
	var got Event
	if err := proto.Unmarshal(badWire, &got); err == nil {
		t.Fatal("expected unmarshal failure for truncated payload")
	}
}

// goldenTraceFieldsWireHex is the encoding of the run-trace fields (21-25) on
// their own. Appending it to goldenEventWireHex is exactly the canonical
// encoding of the same event with the trace fields set, because protobuf writes
// fields in ascending number order and 21-25 follow 12.
const goldenTraceFieldsWireHex = "a80107b201150a0a67656e6572616c2d763112077072696d617279b80102c2011a30314852554e3031323334353637383930414243444546474849ca010a7265715f616263313233"

func TestEventDecodeWithTraceFieldsFromGoldenHex(t *testing.T) {
	wire, err := hex.DecodeString(goldenEventWireHex + goldenTraceFieldsWireHex)
	if err != nil {
		t.Fatalf("decode hex: %v", err)
	}

	var got Event
	if err := proto.Unmarshal(wire, &got); err != nil {
		t.Fatalf("unmarshal event with trace fields: %v", err)
	}

	// Fields 1-13 are untouched by the trace extension.
	if got.GetEventId() != "01HV7A4Q2M7S5E9J6R8C2T1Z4X" || got.GetProducer() != "model-gateway" {
		t.Fatalf("pre-trace fields changed: %q / %q", got.GetEventId(), got.GetProducer())
	}
	if got.GetProducerSeq() != 7 {
		t.Fatalf("producer_seq: got %d", got.GetProducerSeq())
	}
	if got.GetAgent().GetId() != "general-v1" || got.GetAgent().GetRole() != "primary" || got.GetAgent().GetParentId() != "" {
		t.Fatalf("agent: got %v", got.GetAgent())
	}
	if got.GetOrigin() != RunOrigin_RUN_ORIGIN_AGENT {
		t.Fatalf("origin: got %v", got.GetOrigin())
	}
	if got.GetRunId() != "01HRUN01234567890ABCDEFGHI" {
		t.Fatalf("run_id: got %q", got.GetRunId())
	}
	if got.GetRequestId() != "req_abc123" {
		t.Fatalf("request_id: got %q", got.GetRequestId())
	}

	// The same message must encode back to the same bytes.
	again, err := proto.MarshalOptions{Deterministic: true}.Marshal(&got)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if !bytes.Equal(again, wire) {
		t.Fatalf("re-encoding changed the wire bytes: got %x want %x", again, wire)
	}
}

func TestEventDecodeStillAcceptsPreTraceGoldenHex(t *testing.T) {
	wire, err := hex.DecodeString(goldenEventWireHex)
	if err != nil {
		t.Fatalf("decode hex: %v", err)
	}

	var got Event
	if err := proto.Unmarshal(wire, &got); err != nil {
		t.Fatalf("a pre-trace event must still decode: %v", err)
	}
	if got.GetProducerSeq() != 0 || got.GetAgent() != nil || got.GetOrigin() != RunOrigin_RUN_ORIGIN_UNSPECIFIED ||
		got.GetRunId() != "" || got.GetRequestId() != "" {
		t.Fatalf("trace fields must be zero on a pre-trace event: %v", &got)
	}

	// A pre-trace event re-encodes byte for byte: the new fields add nothing.
	again, err := proto.MarshalOptions{Deterministic: true}.Marshal(&got)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if hex.EncodeToString(again) != goldenEventWireHex {
		t.Fatalf("a pre-trace event no longer re-encodes to the golden bytes")
	}
}

func TestTraceEventTypesAreAllocated(t *testing.T) {
	want := map[EventType]int32{
		EventType_EVENT_TYPE_BROWSER_SESSION_OPENED:        150,
		EventType_EVENT_TYPE_BROWSER_SESSION_CLOSED:        151,
		EventType_EVENT_TYPE_BROWSER_CONTROL_CHANGED:       152,
		EventType_EVENT_TYPE_BROWSER_TAB_CHANGED:           153,
		EventType_EVENT_TYPE_BROWSER_STEP_RECEIPT_RECORDED: 154,
		EventType_EVENT_TYPE_BROWSER_EGRESS_DECIDED:        155,
		EventType_EVENT_TYPE_BROWSER_ARTIFACT_RECORDED:     156,
		EventType_EVENT_TYPE_BROWSER_ATTACH_SHARED_JAR:     160,
	}
	for et, n := range want {
		if int32(et) != n {
			t.Errorf("%v: got %d, want %d", et, int32(et), n)
		}
	}
}
