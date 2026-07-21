package address_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/address"
	"coresystem/apps/application-plane/information-core/internal/cache"
)

func TestLookup_ReturnsBoundedKartverketResultsAndProvenance(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.URL.Query().Get("sok"); got != "Storgata 2 Oslo" {
			t.Errorf("sok = %q, want %q", got, "Storgata 2 Oslo")
		}
		if got := r.URL.Query().Get("treffPerSide"); got != "10" {
			t.Errorf("treffPerSide = %q, want 10", got)
		}
		if got := r.URL.Query().Get("side"); got != "0" {
			t.Errorf("side = %q, want 0", got)
		}
		if got := r.URL.Query().Get("utkoordsys"); got != "4258" {
			t.Errorf("utkoordsys = %q, want 4258", got)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"metadata": {"sokeStreng": "Storgata 2 Oslo", "totaltAntallTreff": 1, "treffPerSide": 10, "side": 0},
			"adresser": [{
				"adressetekst": "Storgata 2",
				"adressenavn": "Storgata",
				"nummer": 2,
				"kommunenummer": "0301",
				"kommunenavn": "Oslo",
				"postnummer": "0155",
				"poststed": "Oslo",
				"objtype": "Vegadresse",
				"representasjonspunkt": {"epsg": "4258", "lat": 59.9139, "lon": 10.7522}
			}]
		}`))
	}))
	defer server.Close()

	svc := address.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL)
	result, err := svc.Lookup(context.Background(), "Storgata 2 Oslo", 10, 0, false)
	if err != nil {
		t.Fatalf("Lookup() error = %v", err)
	}
	if len(result.Data) != 1 || result.Data[0].MunicipalityCode != "0301" {
		t.Fatalf("unexpected address data: %+v", result.Data)
	}
	if result.Source.Provider != "kartverket" || result.Source.Dataset != "address-rest-v1.2.0" {
		t.Fatalf("unexpected source envelope: %+v", result.Source)
	}
	if result.Source.Status != "measured" {
		t.Fatalf("source status = %q, want measured", result.Source.Status)
	}
}

func TestLookup_RejectsUnboundedQueries(t *testing.T) {
	svc := address.NewServiceWithURL(http.DefaultClient, cache.New(), "http://unused")
	if _, err := svc.Lookup(context.Background(), "", 10, 0, false); err == nil {
		t.Fatal("expected empty query to fail")
	}
	longQuery := make([]byte, 201)
	for i := range longQuery {
		longQuery[i] = 'a'
	}
	if _, err := svc.Lookup(context.Background(), string(longQuery), 10, 0, false); err == nil {
		t.Fatal("expected oversized query to fail")
	}
}

func TestLookup_CachesSuccessfulResponse(t *testing.T) {
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"metadata":{},"adresser":[]}`))
	}))
	defer server.Close()

	svc := address.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL)
	ctx := context.Background()
	for i := 0; i < 2; i++ {
		if _, err := svc.Lookup(ctx, "Oslo", 10, 0, false); err != nil {
			t.Fatalf("Lookup() call %d error = %v", i+1, err)
		}
	}
	if calls != 1 {
		t.Fatalf("upstream calls = %d, want 1", calls)
	}
}

func TestLookup_ReportsUpstreamFailure(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()

	svc := address.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL)
	if _, err := svc.Lookup(context.Background(), "Oslo", 10, 0, false); err == nil {
		t.Fatal("expected upstream failure")
	}
}
