package capabilityhealth

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
)

// fakePlanes is Auth Core and Capability Core in one test server. It records the attestations it receives.
type fakePlanes struct {
	mu           sync.Mutex
	minted       int
	mintBody     tokenRequest
	mintHeaders  http.Header
	attested     []attestation
	attestStatus int
}

func (f *fakePlanes) handler(t *testing.T) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		f.mu.Lock()
		defer f.mu.Unlock()
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/api/capability-core/internal-token":
			f.minted++
			f.mintHeaders = r.Header.Clone()
			_ = json.NewDecoder(r.Body).Decode(&f.mintBody)
			_ = json.NewEncoder(w).Encode(map[string]any{"token": "cap-token", "audience": "capability-core"})
		case r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/api/v1/capabilities/"):
			if r.Header.Get("Authorization") != "Bearer cap-token" {
				w.WriteHeader(http.StatusUnauthorized)
				return
			}
			id := strings.TrimPrefix(r.URL.Path, "/api/v1/capabilities/")
			_ = json.NewEncoder(w).Encode(map[string]any{"id": id, "version": "3"})
		case r.Method == http.MethodPost && r.URL.Path == ownerHealthPath:
			raw, _ := io.ReadAll(r.Body)
			var body attestation
			if err := json.Unmarshal(raw, &body); err != nil {
				t.Errorf("attestation body: %v", err)
			}
			f.attested = append(f.attested, body)
			if f.attestStatus != 0 {
				w.WriteHeader(f.attestStatus)
				return
			}
			w.WriteHeader(http.StatusOK)
		default:
			w.WriteHeader(http.StatusNotFound)
		}
	})
}

func newTestReporter(t *testing.T, url string, probe Probe) *Reporter {
	t.Helper()
	reporter, err := New(Config{
		CapabilityCoreURL: url, AuthCoreURL: url, ServiceID: "service:wiki-store",
		Credential: strings.Repeat("k", 32), Probe: probe,
	})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return reporter
}

func readyProbe(context.Context) error { return nil }

// A ready canvas attests both canvas rows on the owner-action lane, with a global capability-core token that carries exactly the
// read and owner-action health scopes, minted as wiki-store's own service identity.
func TestReporterAttestsBothCanvasRowsOnTheOwnerLane(t *testing.T) {
	planes := &fakePlanes{}
	server := httptest.NewServer(planes.handler(t))
	defer server.Close()
	if err := newTestReporter(t, server.URL, readyProbe).Attest(context.Background()); err != nil {
		t.Fatalf("Attest: %v", err)
	}
	if planes.mintHeaders.Get("x-service-id") != "service:wiki-store" {
		t.Fatalf("minted as %q, want service:wiki-store", planes.mintHeaders.Get("x-service-id"))
	}
	if planes.mintBody.OrgID != "global" || strings.Join(planes.mintBody.Scopes, ",") != readScope+","+ownerHealthScope {
		t.Fatalf("token request = %+v", planes.mintBody)
	}
	if len(planes.attested) != 2 || planes.attested[0].ID != CanvasReadCapabilityID || planes.attested[1].ID != CanvasWriteCapabilityID {
		t.Fatalf("attested = %+v, want the read and the write rows", planes.attested)
	}
	for _, a := range planes.attested {
		if a.Version != "3" || a.State != "available" || a.ReasonCode != "runtime_healthy" {
			t.Fatalf("attestation = %+v", a)
		}
	}
}

// A failing probe attests nothing and mints nothing.
func TestReporterAttestsNothingWhenTheProbeFails(t *testing.T) {
	planes := &fakePlanes{}
	server := httptest.NewServer(planes.handler(t))
	defer server.Close()
	err := newTestReporter(t, server.URL, func(context.Context) error { return errors.New("down") }).Attest(context.Background())
	if err == nil {
		t.Fatal("Attest succeeded with a failing probe")
	}
	if planes.minted != 0 || len(planes.attested) != 0 {
		t.Fatalf("minted=%d attested=%d, want none", planes.minted, len(planes.attested))
	}
}

// Capability Core refusing the attester (for example while no principal carries service:wiki-store) is an error, never a success.
func TestReporterReportsARefusedAttestation(t *testing.T) {
	planes := &fakePlanes{attestStatus: http.StatusForbidden}
	server := httptest.NewServer(planes.handler(t))
	defer server.Close()
	if err := newTestReporter(t, server.URL, readyProbe).Attest(context.Background()); err == nil {
		t.Fatal("Attest succeeded although Capability Core refused it")
	}
}

// A partial block, a block without a probe, or a URL with credentials is refused; an unset block is not configured.
func TestNewRefusesAPartialOrUnsafeBlock(t *testing.T) {
	if (Config{}).Configured() {
		t.Fatal("an empty block reads as configured")
	}
	full := Config{CapabilityCoreURL: "http://cap:8080", AuthCoreURL: "http://auth:3011", ServiceID: "service:wiki-store",
		Credential: strings.Repeat("k", 32), Probe: readyProbe}
	if _, err := New(full); err != nil {
		t.Fatalf("full block refused: %v", err)
	}
	partial := full
	partial.Credential = ""
	if !partial.Configured() {
		t.Fatal("a partial block reads as unconfigured")
	}
	if _, err := New(partial); err == nil {
		t.Fatal("a partial block was accepted")
	}
	noProbe := full
	noProbe.Probe = nil
	if _, err := New(noProbe); err == nil {
		t.Fatal("a block without a probe was accepted")
	}
	unsafe := full
	unsafe.CapabilityCoreURL = "http://user:pass@cap:8080"
	if _, err := New(unsafe); err == nil {
		t.Fatal("a URL with credentials was accepted")
	}
}

// The canvas probe passes only with the switch on, a key loaded and a store that answers.
func TestCanvasProbe(t *testing.T) {
	ok := func(context.Context) error { return nil }
	down := func(context.Context) error { return errors.New("no") }
	cases := []struct {
		name    string
		probe   Probe
		wantErr bool
	}{
		{"ready", CanvasProbe(true, 1, ok), false},
		{"switch off", CanvasProbe(false, 1, ok), true},
		{"no key", CanvasProbe(true, 0, ok), true},
		{"store down", CanvasProbe(true, 1, down), true},
		{"no store", CanvasProbe(true, 1, nil), true},
	}
	for _, tc := range cases {
		if err := tc.probe(context.Background()); (err != nil) != tc.wantErr {
			t.Errorf("%s: err = %v, wantErr %v", tc.name, err, tc.wantErr)
		}
	}
}
