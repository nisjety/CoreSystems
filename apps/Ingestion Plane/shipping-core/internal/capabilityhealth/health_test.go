package capabilityhealth

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"shipping-core/internal/carrier"
	"shipping-core/internal/quoteengine"
)

func testLogger() *slog.Logger {
	return slog.New(slog.NewTextHandler(io.Discard, nil))
}

func TestBearer_MintsSendsCredentialsAndCaches(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		if r.URL.Path != "/api/capability-core/internal-token" {
			t.Fatalf("unexpected path %q", r.URL.Path)
		}
		if r.Header.Get("X-Service-ID") != "shipping-core" || r.Header.Get("X-Service-API-Key") != "shipping-secret" {
			t.Fatal("service credential headers missing")
		}
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body["orgId"] != "global" {
			t.Errorf("orgId = %v, want global", body["orgId"])
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token": "cap-token", "expiresInSeconds": 300, "audience": "capability-core",
		})
	}))
	defer server.Close()

	a := NewAttestor(Config{AuthCoreURL: server.URL, CapabilityCoreURL: "http://unused.invalid", ServiceID: "shipping-core", ServiceCredential: "shipping-secret"}, testLogger())

	tok, err := a.bearer(context.Background())
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if tok != "cap-token" {
		t.Errorf("token = %q", tok)
	}
	if _, err := a.bearer(context.Background()); err != nil {
		t.Fatalf("unexpected error on cached call: %v", err)
	}
	if calls != 1 {
		t.Errorf("token endpoint called %d times, want 1 (second call should be cached)", calls)
	}
}

func TestBearer_RejectsWrongAudience(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"token": "cap-token", "expiresInSeconds": 300, "audience": "model-gateway",
		})
	}))
	defer server.Close()

	a := NewAttestor(Config{AuthCoreURL: server.URL, CapabilityCoreURL: "http://unused.invalid", ServiceID: "shipping-core", ServiceCredential: "k"}, testLogger())
	if _, err := a.bearer(context.Background()); err == nil {
		t.Fatal("expected an error for a token minted with the wrong audience")
	}
}

func TestAttest_ReadsVersionThenPostsExactBody(t *testing.T) {
	var gotVersionRequestPath, gotAuth string
	var gotBody attestationBody
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/api/capability-core/internal-token":
			_ = json.NewEncoder(w).Encode(map[string]any{
				"token": "cap-token", "expiresInSeconds": 300, "audience": "capability-core",
			})
		case r.Method == http.MethodGet && r.URL.Path == "/api/v1/capabilities/cap.tool.shipping.read":
			gotVersionRequestPath = r.URL.Path
			_ = json.NewEncoder(w).Encode(map[string]any{"id": "cap.tool.shipping.read", "version": "1.0.0"})
		case r.Method == http.MethodPost && r.URL.Path == "/api/v1/capabilities/availability":
			gotAuth = r.Header.Get("Authorization")
			_ = json.NewDecoder(r.Body).Decode(&gotBody)
			_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]any{"id": gotBody.ID, "state": "available"}})
		default:
			t.Fatalf("unexpected request %s %s", r.Method, r.URL.Path)
		}
	}))
	defer server.Close()

	a := NewAttestor(Config{AuthCoreURL: server.URL, CapabilityCoreURL: server.URL, ServiceID: "shipping-core", ServiceCredential: "k"}, testLogger())
	err := a.attest(context.Background(), ReadCapabilityID, "shipping_quote_probe_succeeded", "carrier bring verified")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotVersionRequestPath != "/api/v1/capabilities/cap.tool.shipping.read" {
		t.Errorf("version read path = %q", gotVersionRequestPath)
	}
	if gotAuth != "Bearer cap-token" {
		t.Errorf("Authorization = %q", gotAuth)
	}
	if gotBody != (attestationBody{
		ID: "cap.tool.shipping.read", Version: "1.0.0", State: "available",
		ReasonCode: "shipping_quote_probe_succeeded", Reason: "carrier bring verified",
		ExecutionMode: "agentic", CostClass: "variable",
	}) {
		t.Errorf("attestation body = %+v", gotBody)
	}
}

func TestAttest_UpstreamRefusalSurfacedAsError(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.URL.Path == "/api/capability-core/internal-token":
			_ = json.NewEncoder(w).Encode(map[string]any{
				"token": "cap-token", "expiresInSeconds": 300, "audience": "capability-core",
			})
		case r.URL.Path == "/api/v1/capabilities/cap.tool.shipping.book":
			_ = json.NewEncoder(w).Encode(map[string]any{"id": "cap.tool.shipping.book", "version": "1.0.0"})
		default:
			w.WriteHeader(http.StatusConflict)
			_, _ = w.Write([]byte(`{"error":"version mismatch"}`))
		}
	}))
	defer server.Close()

	a := NewAttestor(Config{AuthCoreURL: server.URL, CapabilityCoreURL: server.URL, ServiceID: "shipping-core", ServiceCredential: "k"}, testLogger())
	if err := a.attest(context.Background(), BookCapabilityID, "x", "y"); err == nil {
		t.Fatal("expected an error when capability-core refuses the attestation")
	}
}

// probeQuoter is a minimal, test-local Quoter, matching the pattern
// quoteengine's own fan-out tests use, so Probe's mock-skipping and
// verified-carrier logic can be asserted independently of any real
// carrier adapter.
type probeQuoter struct {
	code string
	mode carrier.Mode
	err  error
}

func (p *probeQuoter) Info() carrier.Info {
	return carrier.Info{Code: p.code, Name: p.code, Segment: carrier.SegmentBoth, Mode: p.mode}
}

func (p *probeQuoter) Quote(_ context.Context, _ carrier.QuoteRequest) ([]carrier.Quote, error) {
	if p.err != nil {
		return nil, p.err
	}
	return []carrier.Quote{{CarrierCode: p.code, CarrierName: p.code}}, nil
}

func TestProbe_TrueWhenANonMockCarrierIsVerified(t *testing.T) {
	engine := quoteengine.New([]quoteengine.Quoter{
		&probeQuoter{code: "mock-bring", mode: carrier.ModeMock},
		&probeQuoter{code: "bring", mode: carrier.ModeProduction},
	}, time.Second)

	ok, detail := Probe(context.Background(), engine)
	if !ok {
		t.Fatalf("expected ok=true, detail=%q", detail)
	}
	if detail == "" {
		t.Error("expected a non-empty detail describing which carrier verified")
	}
}

func TestProbe_FalseWhenOnlyTheMockCarrierSucceeds(t *testing.T) {
	engine := quoteengine.New([]quoteengine.Quoter{
		&probeQuoter{code: "mock-bring", mode: carrier.ModeMock},
		&probeQuoter{code: "bring", mode: carrier.ModeProduction, err: context.DeadlineExceeded},
	}, time.Second)

	ok, _ := Probe(context.Background(), engine)
	if ok {
		t.Fatal("expected ok=false when only the mock carrier answers")
	}
}

func TestConfig_ConfiguredRequiresAllFourFields(t *testing.T) {
	full := Config{AuthCoreURL: "a", CapabilityCoreURL: "b", ServiceID: "c", ServiceCredential: "d"}
	if !full.Configured() {
		t.Fatal("expected Configured()=true with all fields set")
	}
	missing := full
	missing.CapabilityCoreURL = ""
	if missing.Configured() {
		t.Fatal("expected Configured()=false with CapabilityCoreURL missing")
	}
}
