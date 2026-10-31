package siem

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"
)

// The golden is org-core's own contract file (WS06-S7), vendored byte for byte:
// the requests the exporter must send and the answers it must read. Replaying
// it here means a header, path or field drift on either side fails a test.
const (
	siemContractGolden     = "testdata/audit-core-org-core-siem.contract.json"
	orgCoreGoldenOriginal  = "../../../org-core/internal/http/testdata/audit-core-org-core-siem.contract.json"
	contractTokenSecretVal = "contract-test-service-token-value"
)

type contractExchange struct {
	Name    string `json:"name"`
	Request struct {
		Method  string            `json:"method"`
		Path    string            `json:"path"`
		Headers map[string]string `json:"headers"`
	} `json:"request"`
	ExpectUpdatedSince string `json:"expectUpdatedSince"`
	Response           struct {
		Status       int            `json:"status"`
		CacheControl string         `json:"cacheControl"`
		Body         map[string]any `json:"body"`
	} `json:"response"`
}

type contractGolden struct {
	Version          int                `json:"version"`
	TokenPlaceholder string             `json:"tokenPlaceholder"`
	Exchanges        []contractExchange `json:"exchanges"`
}

func sha256LF(data []byte) string {
	sum := sha256.Sum256(bytes.ReplaceAll(data, []byte("\r\n"), []byte("\n")))
	return fmt.Sprintf("%x", sum)
}

func loadGolden(t *testing.T) (contractGolden, []byte) {
	t.Helper()
	raw, err := os.ReadFile(siemContractGolden)
	if err != nil {
		t.Fatalf("read golden: %v", err)
	}
	var golden contractGolden
	if err := json.Unmarshal(raw, &golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if golden.Version != 1 || len(golden.Exchanges) == 0 {
		t.Fatalf("golden is not a version 1 contract with exchanges")
	}
	return golden, raw
}

func TestGoldenMatchesItsPinAndOrgCoresCopy(t *testing.T) {
	_, raw := loadGolden(t)
	pin, err := os.ReadFile(siemContractGolden + ".sha256")
	if err != nil {
		t.Fatalf("read pin: %v", err)
	}
	fields := strings.Fields(string(pin))
	if len(fields) == 0 || fields[0] != sha256LF(raw) {
		t.Fatalf("golden differs from its pin; re-vendor it from org-core together with the pin")
	}
	// Outside the module (a Docker build context) org-core's file is not there.
	original, err := os.ReadFile(orgCoreGoldenOriginal)
	if err != nil {
		t.Skip("org-core's copy of the golden is not reachable from here")
	}
	if sha256LF(original) != fields[0] {
		t.Fatalf("org-core's golden changed after it was vendored here: re-vendor it and the pin")
	}
}

func TestClientReplaysTheOrgCoreGolden(t *testing.T) {
	golden, _ := loadGolden(t)
	for _, exchange := range golden.Exchanges {
		t.Run(exchange.Name, func(t *testing.T) {
			var seen *http.Request
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				seen = r
				if r.Method != exchange.Request.Method || r.URL.RequestURI() != exchange.Request.Path {
					t.Errorf("request %s %s, golden wants %s %s", r.Method, r.URL.RequestURI(), exchange.Request.Method, exchange.Request.Path)
				}
				for name, want := range exchange.Request.Headers {
					if want == golden.TokenPlaceholder {
						want = contractTokenSecretVal
					}
					if got := r.Header.Get(name); got != want {
						t.Errorf("header %s = %q, golden wants %q", name, got, want)
					}
				}
				w.Header().Set("Cache-Control", exchange.Response.CacheControl)
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(exchange.Response.Status)
				_ = json.NewEncoder(w).Encode(exchange.Response.Body)
			}))
			defer server.Close()

			client, err := NewOrgCoreClient(server.URL, "audit-core", contractTokenSecretVal, nil)
			if err != nil {
				t.Fatal(err)
			}
			var since time.Time
			if exchange.ExpectUpdatedSince != "" {
				if since, err = time.Parse(time.RFC3339, exchange.ExpectUpdatedSince); err != nil {
					t.Fatal(err)
				}
			}
			destinations, err := client.ListDestinations(context.Background(), since)
			if err != nil {
				t.Fatal(err)
			}
			if seen == nil {
				t.Fatal("the client sent no request")
			}
			if exchange.ExpectUpdatedSince != "" {
				values, _ := url.ParseQuery(seen.URL.RawQuery)
				if values.Get("updated_since") != exchange.ExpectUpdatedSince {
					t.Errorf("updated_since = %q", values.Get("updated_since"))
				}
			}

			wantData, _ := exchange.Response.Body["data"].([]any)
			if len(destinations) != len(wantData) {
				t.Fatalf("read %d destinations, golden has %d", len(destinations), len(wantData))
			}
			for i, raw := range wantData {
				want := raw.(map[string]any)
				got := destinations[i]
				if got.ID != want["id"] || got.OrgID != want["org_id"] || got.Kind != want["kind"] ||
					got.URL != want["url"] || got.Secret != want["secret"] || float64(got.Revision) != want["revision"] {
					t.Errorf("destination %d = %+v, golden %v", i, got, want)
				}
				filter, _ := want["event_filter"].(map[string]any)
				if events, _ := filter["events"].([]any); len(events) != len(got.EventFilter.Events) {
					t.Errorf("destination %d event filter = %+v, golden %v", i, got.EventFilter, filter)
				}
				if got.UpdatedAt.IsZero() {
					t.Errorf("destination %d has no updated_at", i)
				}
			}
		})
	}
}

func TestClientFailureNeverCarriesTheTokenOrURL(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "forbidden "+contractTokenSecretVal, http.StatusForbidden)
	}))
	defer server.Close()
	client, err := NewOrgCoreClient(server.URL, "audit-core", contractTokenSecretVal, nil)
	if err != nil {
		t.Fatal(err)
	}
	_, err = client.ListDestinations(context.Background(), time.Time{})
	if err == nil || strings.Contains(err.Error(), contractTokenSecretVal) || strings.Contains(err.Error(), server.URL) {
		t.Fatalf("a refused read must fail without the token or URL: %v", err)
	}

	server.Close()
	_, err = client.ListDestinations(context.Background(), time.Time{})
	if err == nil || strings.Contains(err.Error(), server.URL) || strings.Contains(err.Error(), contractTokenSecretVal) {
		t.Fatalf("a transport failure must not embed the URL or token: %v", err)
	}
}

func TestClientDoesNotFollowARedirectWithItsToken(t *testing.T) {
	var elsewhereHits int
	elsewhere := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { elsewhereHits++ }))
	defer elsewhere.Close()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, elsewhere.URL, http.StatusFound)
	}))
	defer server.Close()
	client, _ := NewOrgCoreClient(server.URL, "audit-core", contractTokenSecretVal, nil)
	if _, err := client.ListDestinations(context.Background(), time.Time{}); err == nil || elsewhereHits != 0 {
		t.Fatalf("redirect followed (hits=%d) or accepted (err=%v)", elsewhereHits, err)
	}
}

func TestNewOrgCoreClientRequiresItsIdentity(t *testing.T) {
	for _, args := range [][3]string{{"", "audit-core", "t"}, {"http://x", "", "t"}, {"http://x", "audit-core", ""}} {
		if _, err := NewOrgCoreClient(args[0], args[1], args[2], nil); err == nil {
			t.Errorf("accepted %v", args)
		}
	}
}

func TestDestinationRedactsItselfEverywhere(t *testing.T) {
	d := Destination{ID: "d1", OrgID: "o1", Kind: "splunk_hec", URL: "https://hec.example.com/x?token=URLSECRET", Secret: "THE-SECRET-VALUE"}
	encoded, err := json.Marshal(d)
	if err != nil {
		t.Fatal(err)
	}
	for _, rendering := range []string{
		d.String(), fmt.Sprintf("%v", d), fmt.Sprintf("%+v", d), fmt.Sprintf("%#v", d), fmt.Sprintf("%s", d), string(encoded),
		fmt.Sprintf("%v", &d), fmt.Sprintf("%+v", []Destination{d}),
	} {
		if strings.Contains(rendering, "THE-SECRET-VALUE") || strings.Contains(rendering, "URLSECRET") || strings.Contains(rendering, "hec.example.com") {
			t.Fatalf("destination rendering leaks: %s", rendering)
		}
	}
	if !reflect.DeepEqual(d.Secret, "THE-SECRET-VALUE") {
		t.Fatal("redaction must not alter the value used for delivery")
	}
}
