package quarrycontracts

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestNewIDPrefixAndKind(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name string
		kind IDKind
	}{
		{"run", KindRun},
		{"queue", KindQueue},
		{"job", KindJob},
		{"schedule", KindSchedule},
		{"event", KindEvent},
		{"artifact", KindArtifact},
		{"lease", KindLease},
		{"profile", KindProfile},
		{"snapshot", KindSnapshot},
		{"checkpoint", KindCheckpoint},
		{"store", KindStore},
		{"webhook", KindWebhook},
		{"request", KindRequest},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			id := NewID(tc.kind)
			if !strings.HasPrefix(string(id), string(tc.kind)) {
				t.Fatalf("expected prefix %q, got id %q", tc.kind, id)
			}
			got, ok := id.Kind()
			if !ok || got != tc.kind {
				t.Fatalf("Kind() = %q, %v; want %q", got, ok, tc.kind)
			}
			if err := id.MustKind(tc.kind); err != nil {
				t.Fatalf("MustKind: %v", err)
			}
		})
	}
}

func TestMustKindRejectsWrongPrefix(t *testing.T) {
	t.Parallel()
	run := NewID(KindRun)
	if err := run.MustKind(KindJob); err == nil {
		t.Fatal("expected mismatch error, got nil")
	}
}

func TestIDJSONRoundtrip(t *testing.T) {
	t.Parallel()
	type payload struct {
		ID ID `json:"id"`
	}
	in := payload{ID: NewID(KindRun)}
	raw, err := json.Marshal(in)
	if err != nil {
		t.Fatal(err)
	}
	var out payload
	if err := json.Unmarshal(raw, &out); err != nil {
		t.Fatal(err)
	}
	if out.ID != in.ID {
		t.Fatalf("roundtrip mismatch: %q vs %q", out.ID, in.ID)
	}
}

func TestErrorCodeHTTPStatus(t *testing.T) {
	t.Parallel()
	cases := []struct {
		code ErrorCode
		want int
	}{
		{CodeBadRequest, 400},
		{CodeUnauthorized, 401},
		{CodeForbidden, 403},
		{CodeSecurityBlocked, 403},
		{CodeNotFound, 404},
		{CodeConflict, 409},
		{CodeRateLimited, 429},
		{CodeTimeout, 504},
		{CodeDriverFailed, 502},
		{CodeUpstreamBlocked, 502},
		{CodeInternal, 500},
	}
	for _, tc := range cases {
		if got := tc.code.HTTPStatus(); got != tc.want {
			t.Errorf("HTTPStatus(%q) = %d, want %d", tc.code, got, tc.want)
		}
	}
}

func TestDefaultRunPolicy(t *testing.T) {
	t.Parallel()
	p := DefaultRunPolicy()
	if p.Concurrency.PerRun == 0 || p.Concurrency.PerDomain == 0 {
		t.Error("concurrency must be non-zero")
	}
	if p.Retry.Backoff != BackoffExp {
		t.Errorf("default backoff = %q, want %q", p.Retry.Backoff, BackoffExp)
	}
	if !p.Delay.Jitter {
		t.Error("default jitter should be true")
	}
	if p.Robots != RobotsRespect {
		t.Errorf("default robots = %q, want %q", p.Robots, RobotsRespect)
	}
}

func TestOKEnvelopeShape(t *testing.T) {
	t.Parallel()
	env := OK("req_abc", map[string]string{"hello": "world"})
	raw, _ := json.Marshal(env)
	var m map[string]any
	_ = json.Unmarshal(raw, &m)
	meta := m["meta"].(map[string]any)
	if meta["request_id"] != "req_abc" {
		t.Fatalf("request_id = %v", meta["request_id"])
	}
	if m["error"] != nil {
		t.Fatalf("expected null error, got %v", m["error"])
	}
}
