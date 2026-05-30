package mpv1

import (
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
