package envelope_test

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/envelope"
)

const preTraceJSON = `{"event_id":"01HXYZ01234567890ABCDEFGHI","event_type":"RUN_STARTED","schema_version":1,"ts":"2026-01-15T12:00:00Z","producer":"model-gateway","correlation_id":"corr-001","causation_id":"cause-001","idempotency_key":"req-001","org_id":"01HORG01234567890ABCDEFGHI","user_id":"01HUSR01234567890ABCDEFGHI","resource_ref":"run/01HRUN01234567890ABCDEFGHI","payload":{"goal":"test run"}}`

func traceEnvelope() *envelope.Envelope {
	return &envelope.Envelope{
		EventID:        "01HXYZ",
		EventType:      "BROWSER_STEP_RECEIPT_RECORDED",
		SchemaVersion:  1,
		Ts:             time.Date(2026, 11, 1, 9, 30, 15, 250_000_000, time.UTC),
		Producer:       "quarry-edge",
		CorrelationID:  "corr-1",
		CausationID:    "cause-1",
		IdempotencyKey: "key-1",
		OrgID:          "org1",
		UserID:         "user1",
		ResourceRef:    "run/01HRUN",
		Payload:        json.RawMessage(`{"step":7}`),
		Zdr:            envelope.ZDRFlag(false),
		ProducerSeq:    7,
		Agent:          &envelope.AgentRef{ID: "general-v1", Role: "primary"},
		Origin:         "AGENT",
		RunID:          "01HRUN",
		RequestID:      "req_abc123",
	}
}

func TestEnvelopeRoundTripsTraceFields(t *testing.T) {
	data, err := traceEnvelope().Encode()
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	got, err := envelope.Decode(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if got.ProducerSeq != 7 || got.Origin != "AGENT" || got.RunID != "01HRUN" || got.RequestID != "req_abc123" {
		t.Fatalf("trace scalars lost: %+v", got)
	}
	if got.Agent == nil || got.Agent.ID != "general-v1" || got.Agent.Role != "primary" || got.Agent.ParentID != "" {
		t.Fatalf("agent lost: %+v", got.Agent)
	}
	if got.Zdr == nil || *got.Zdr {
		t.Fatalf("explicit zdr=false must survive, got %v", got.Zdr)
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatalf("unmarshal raw: %v", err)
	}
	if string(raw["producer_seq"]) != "7" {
		t.Fatalf("producer_seq must be a JSON number, got %s", raw["producer_seq"])
	}
	if strings.Contains(string(raw["agent"]), "parent_id") {
		t.Fatalf("an empty parent_id must be omitted: %s", raw["agent"])
	}
}

func TestEnvelopeOmitsEmptyTraceFields(t *testing.T) {
	env, err := envelope.Decode([]byte(preTraceJSON))
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	data, err := env.Encode()
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatalf("unmarshal raw: %v", err)
	}
	for _, key := range []string{"producer_seq", "agent", "origin", "run_id", "request_id"} {
		if _, present := raw[key]; present {
			t.Errorf("%s must be omitted when empty, got %s", key, raw[key])
		}
	}
}

func TestEnvelopeStillDecodesPreTraceJSON(t *testing.T) {
	env, err := envelope.Decode([]byte(preTraceJSON))
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if err := env.Validate(); err != nil {
		t.Fatalf("a pre-trace envelope must still validate: %v", err)
	}
	if env.ProducerSeq != 0 || env.Agent != nil || env.Origin != "" || env.RunID != "" || env.RequestID != "" {
		t.Fatalf("trace fields must be zero on a pre-trace envelope: %+v", env)
	}
	if env.Zdr != nil {
		t.Fatalf("an absent zdr must stay undeclared (nil), got %v", *env.Zdr)
	}
}

func TestRunTraceGoldenDecodes(t *testing.T) {
	data, err := os.ReadFile("testdata/run-trace-envelope.golden.json")
	if err != nil {
		t.Fatalf("the vendored golden must exist (a skipped contract test is not a test): %v", err)
	}
	pin, err := os.ReadFile("testdata/run-trace-envelope.golden.json.sha256")
	if err != nil {
		t.Fatalf("the vendored pin must exist: %v", err)
	}
	sum := sha256.Sum256(data)
	if !strings.HasPrefix(string(pin), hex.EncodeToString(sum[:])) {
		t.Fatalf("golden does not match its pin %q", strings.TrimSpace(string(pin)))
	}

	env, err := envelope.Decode(data)
	if err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if err := env.Validate(); err != nil {
		t.Fatalf("golden must validate: %v", err)
	}
	if env.EventType != "BROWSER_STEP_RECEIPT_RECORDED" || env.Producer != "quarry-edge" {
		t.Fatalf("unexpected golden identity: %s / %s", env.EventType, env.Producer)
	}
	if env.Zdr == nil || *env.Zdr {
		t.Fatalf("the golden must carry an explicit zdr=false, got %v", env.Zdr)
	}
	if env.ProducerSeq != 7 || env.RunID == "" || env.RequestID == "" || env.Agent == nil || env.Agent.ParentID == "" {
		t.Fatalf("the golden must carry all five trace fields: %+v", env)
	}
	known := false
	for _, o := range envelope.RunOrigins {
		known = known || o == env.Origin
	}
	if !known {
		t.Fatalf("golden origin %q is not a RunOrigin name", env.Origin)
	}
	// Re-encoding and decoding again must not lose any of the 18 fields.
	again, err := env.Encode()
	if err != nil {
		t.Fatalf("re-encode: %v", err)
	}
	var keys map[string]json.RawMessage
	if err := json.Unmarshal(again, &keys); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(keys) != 18 {
		t.Fatalf("expected all 18 envelope fields after re-encode, got %d", len(keys))
	}
}
