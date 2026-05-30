package envelope_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
)

func TestValidate_MissingEventID(t *testing.T) {
	env := &envelope.Envelope{
		EventType:     "SESSION_START",
		SchemaVersion: 1,
		Producer:      "test",
		OrgID:         "org1",
	}
	if err := env.Validate(); err == nil {
		t.Fatal("expected error for missing event_id")
	}
}

func TestValidate_MissingEventType(t *testing.T) {
	env := &envelope.Envelope{
		EventID:       "01HXYZ",
		SchemaVersion: 1,
		Producer:      "test",
		OrgID:         "org1",
	}
	if err := env.Validate(); err == nil {
		t.Fatal("expected error for missing event_type")
	}
}

func TestValidate_MissingSchemaVersion(t *testing.T) {
	env := &envelope.Envelope{
		EventID:   "01HXYZ",
		EventType: "SESSION_START",
		Producer:  "test",
		OrgID:     "org1",
	}
	if err := env.Validate(); err == nil {
		t.Fatal("expected error for missing schema_version")
	}
}

func validEnvelope() *envelope.Envelope {
	return &envelope.Envelope{
		EventID:        "01HXYZ",
		EventType:      "SESSION_START",
		SchemaVersion:  1,
		Ts:             time.Now(),
		Producer:       "test",
		CorrelationID:  "corr-1",
		IdempotencyKey: "key-1",
		OrgID:          "org1",
		UserID:         "user1",
		ResourceRef:    "run/01HXYZ",
	}
}

func TestValidate_Valid(t *testing.T) {
	if err := validEnvelope().Validate(); err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestValidate_MissingTs(t *testing.T) {
	env := validEnvelope()
	env.Ts = time.Time{}
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "ts") {
		t.Fatalf("expected ts error, got: %v", err)
	}
}

func TestValidate_MissingCorrelationID(t *testing.T) {
	env := validEnvelope()
	env.CorrelationID = ""
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "correlation_id") {
		t.Fatalf("expected correlation_id error, got: %v", err)
	}
}

func TestValidate_MissingIdempotencyKey(t *testing.T) {
	env := validEnvelope()
	env.IdempotencyKey = ""
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "idempotency_key") {
		t.Fatalf("expected idempotency_key error, got: %v", err)
	}
}

func TestValidate_MissingUserID(t *testing.T) {
	env := validEnvelope()
	env.UserID = ""
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "user_id") {
		t.Fatalf("expected user_id error, got: %v", err)
	}
}

func TestValidate_MissingResourceRef(t *testing.T) {
	env := validEnvelope()
	env.ResourceRef = ""
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "resource_ref") {
		t.Fatalf("expected resource_ref error, got: %v", err)
	}
}

func TestValidate_MissingProducer(t *testing.T) {
	env := validEnvelope()
	env.Producer = ""
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "producer") {
		t.Fatalf("expected producer error, got: %v", err)
	}
}

func TestValidate_MissingOrgID(t *testing.T) {
	env := validEnvelope()
	env.OrgID = ""
	if err := env.Validate(); err == nil || !containsSubstr(err.Error(), "org_id") {
		t.Fatalf("expected org_id error, got: %v", err)
	}
}

func containsSubstr(haystack, needle string) bool {
	return len(needle) == 0 || (len(haystack) >= len(needle) && indexOf(haystack, needle) >= 0)
}

func indexOf(haystack, needle string) int {
	for i := 0; i+len(needle) <= len(haystack); i++ {
		if haystack[i:i+len(needle)] == needle {
			return i
		}
	}
	return -1
}

func TestEncodeDecode_Roundtrip(t *testing.T) {
	original := &envelope.Envelope{
		EventID:        "01HXYZ",
		EventType:      "RUN_STARTED",
		SchemaVersion:  1,
		Ts:             time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC),
		Producer:       "model-gateway",
		CorrelationID:  "corr-1",
		CausationID:    "cause-1",
		IdempotencyKey: "key-1",
		OrgID:          "org1",
		UserID:         "user1",
		ResourceRef:    "run/01HXYZ",
		Payload:        json.RawMessage(`{"goal":"test"}`),
	}

	data, err := original.Encode()
	if err != nil {
		t.Fatalf("encode error: %v", err)
	}

	decoded, err := envelope.Decode(data)
	if err != nil {
		t.Fatalf("decode error: %v", err)
	}

	if decoded.EventID != original.EventID {
		t.Errorf("event_id mismatch: got %q, want %q", decoded.EventID, original.EventID)
	}
	if decoded.EventType != original.EventType {
		t.Errorf("event_type mismatch: got %q, want %q", decoded.EventType, original.EventType)
	}
}

func TestGoldenFixture_Roundtrip(t *testing.T) {
	fixturePath := filepath.Join("..", "..", "..", "rust", "crates", "mp-events", "tests", "fixtures", "envelope_valid.json")
	data, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Skipf("golden fixture not found at %s: %v", fixturePath, err)
	}

	env, err := envelope.Decode(data)
	if err != nil {
		t.Fatalf("decode golden fixture: %v", err)
	}

	if err := env.Validate(); err != nil {
		t.Fatalf("golden fixture failed validation: %v", err)
	}

	reEncoded, err := env.Encode()
	if err != nil {
		t.Fatalf("re-encode error: %v", err)
	}

	reDecoded, err := envelope.Decode(reEncoded)
	if err != nil {
		t.Fatalf("re-decode error: %v", err)
	}

	if reDecoded.EventID != env.EventID {
		t.Error("round-trip event_id mismatch")
	}
}

func TestDeriveIdempotencyHash_MatchesRust(t *testing.T) {
	const goldenHex = "fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637"
	got := envelope.DeriveIdempotencyHash("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1")
	if got != goldenHex {
		t.Fatalf("DeriveIdempotencyHash mismatch\n got:  %s\n want: %s", got, goldenHex)
	}
}
