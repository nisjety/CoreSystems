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

// ── Zero Data Retention (`zdr`, proto field 13) ──────────────────────────────

// TestZDR_RoundTripsAllThreeStates is the core of the contract: the flag must
// survive marshal → unmarshal, and ABSENT must stay distinguishable from an
// explicit `false`. A plain bool field would collapse those two, which is what
// makes a consumer unable to tell "declared retainable" from "nobody said".
func TestZDR_RoundTripsAllThreeStates(t *testing.T) {
	for _, tc := range []struct {
		name    string
		set     *bool
		wantKey string // literal that must appear (or not) on the wire
		absent  bool
	}{
		{name: "absent", set: nil, absent: true},
		{name: "explicit false", set: envelope.ZDRFlag(false), wantKey: `"zdr":false`},
		{name: "explicit true", set: envelope.ZDRFlag(true), wantKey: `"zdr":true`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			env := validEnvelope()
			env.Zdr = tc.set

			data, err := env.Encode()
			if err != nil {
				t.Fatalf("encode error: %v", err)
			}
			if tc.absent {
				if containsSubstr(string(data), `"zdr"`) {
					t.Fatalf("an undeclared posture must not appear on the wire: %s", data)
				}
			} else if !containsSubstr(string(data), tc.wantKey) {
				t.Fatalf("missing %s on the wire: %s", tc.wantKey, data)
			}

			decoded, err := envelope.Decode(data)
			if err != nil {
				t.Fatalf("decode error: %v", err)
			}
			switch {
			case tc.set == nil && decoded.Zdr != nil:
				t.Fatalf("absent became %v after round-trip", *decoded.Zdr)
			case tc.set != nil && decoded.Zdr == nil:
				t.Fatal("declared posture was lost in the round-trip")
			case tc.set != nil && *decoded.Zdr != *tc.set:
				t.Fatalf("zdr = %v, want %v", *decoded.Zdr, *tc.set)
			}
		})
	}
}

// TestZDR_JSONKeyMatchesProtoAndRust pins the wire key. The proto declares
// `bool zdr = 13` and the Rust twin derives Serialize on a field literally named
// `zdr` with no rename, so both producers must emit exactly `zdr` — a mismatched
// key is the same dropped-flag bug in a new place.
func TestZDR_JSONKeyMatchesProtoAndRust(t *testing.T) {
	env := validEnvelope()
	env.Zdr = envelope.ZDRFlag(true)
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode error: %v", err)
	}
	var generic map[string]json.RawMessage
	if err := json.Unmarshal(data, &generic); err != nil {
		t.Fatalf("unmarshal error: %v", err)
	}
	raw, ok := generic["zdr"]
	if !ok {
		t.Fatalf("expected key %q, got keys: %v", "zdr", generic)
	}
	if string(raw) != "true" {
		t.Fatalf("zdr = %s, want true", raw)
	}
	// A Rust-shaped payload (plain bool, same key) must decode here too.
	fromRust, err := envelope.Decode([]byte(`{"event_id":"e","zdr":true}`))
	if err != nil {
		t.Fatalf("decode rust-shaped payload: %v", err)
	}
	if fromRust.Zdr == nil || !*fromRust.Zdr {
		t.Fatal("a Rust-produced zdr:true must decode as declared ZDR")
	}
}

// TestZDR_GoldenFixtureStaysByteCompatible guards the shared Go/Rust fixture,
// which carries no `zdr` key. Re-encoding it must NOT inject one: a struct shape
// that emitted `"zdr":false` for an unset field would silently upgrade every
// legacy envelope from "no posture declared" to "declared retainable".
func TestZDR_GoldenFixtureStaysByteCompatible(t *testing.T) {
	fixturePath := filepath.Join("..", "..", "..", "rust", "crates", "mp-events", "tests", "fixtures", "envelope_valid.json")
	data, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Skipf("golden fixture not found at %s: %v", fixturePath, err)
	}
	env, err := envelope.Decode(data)
	if err != nil {
		t.Fatalf("decode golden fixture: %v", err)
	}
	if env.Zdr != nil {
		t.Fatalf("fixture declares no posture, got %v", *env.Zdr)
	}
	reEncoded, err := env.Encode()
	if err != nil {
		t.Fatalf("re-encode error: %v", err)
	}
	if containsSubstr(string(reEncoded), `"zdr"`) {
		t.Fatalf("re-encoding invented a posture the fixture never declared: %s", reEncoded)
	}
}

func TestIsZDR(t *testing.T) {
	env := validEnvelope()
	if env.IsZDR() {
		t.Error("an undeclared posture is not ZDR (it is unknown)")
	}
	env.Zdr = envelope.ZDRFlag(false)
	if env.IsZDR() {
		t.Error("zdr:false must not read as ZDR")
	}
	env.Zdr = envelope.ZDRFlag(true)
	if !env.IsZDR() {
		t.Error("zdr:true must read as ZDR")
	}
}

// TestDeclaredZDR covers the byte-level probe used on the publish path, where
// the payload is already serialized.
func TestDeclaredZDR(t *testing.T) {
	for _, tc := range []struct {
		name         string
		data         string
		wantZDR      bool
		wantDeclared bool
	}{
		{name: "explicit true", data: `{"zdr":true}`, wantZDR: true, wantDeclared: true},
		{name: "explicit false", data: `{"zdr":false}`, wantZDR: false, wantDeclared: true},
		{name: "absent key", data: `{"event_id":"e"}`},
		{name: "explicit null", data: `{"zdr":null}`},
		{name: "not json", data: `payload`},
		{name: "empty", data: ``},
		{name: "json but not an object", data: `["zdr",true]`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			zdr, declared := envelope.DeclaredZDR([]byte(tc.data))
			if zdr != tc.wantZDR || declared != tc.wantDeclared {
				t.Fatalf("DeclaredZDR(%q) = (%v, %v), want (%v, %v)",
					tc.data, zdr, declared, tc.wantZDR, tc.wantDeclared)
			}
		})
	}
}

// TestValidate_IgnoresRetentionPosture documents that an undeclared posture is
// still a VALID envelope. Validation must not start rejecting them: the
// lifecycle fact is what downstream run counters consume, and the retention
// decision belongs to whoever wants to persist content, which fails closed on
// absence.
func TestValidate_IgnoresRetentionPosture(t *testing.T) {
	env := validEnvelope()
	env.Zdr = nil
	if err := env.Validate(); err != nil {
		t.Fatalf("an undeclared posture must remain valid, got: %v", err)
	}
}

func TestDeriveIdempotencyHash_MatchesRust(t *testing.T) {
	const goldenHex = "fbc1d94e94d756ede12c527b3b59e2204f58a623e6bd5a3d679eb03d93f22637"
	got := envelope.DeriveIdempotencyHash("model-gateway", "INGRESS_ACCEPTED", "thread/abc", "req-1")
	if got != goldenHex {
		t.Fatalf("DeriveIdempotencyHash mismatch\n got:  %s\n want: %s", got, goldenHex)
	}
}
