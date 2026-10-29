package legal_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/legal"
)

func TestSearch_UsesLovdataAPIKeyAndBoundedTerms(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.Header.Get("X-API-Key"); got != "test-key" {
			t.Errorf("X-API-Key = %q", got)
		}
		if r.URL.Query().Get("emne1") != "arbeidsmiljø" || r.URL.Query().Get("rows") != "5" {
			t.Errorf("unexpected query: %s", r.URL.RawQuery)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"hits":1,"results":[{"refID":"NL/lov/2005-06-17-62"}]}`))
	}))
	defer server.Close()

	svc := legal.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL, "test-key")
	result, err := svc.Search(context.Background(), legal.Request{Terms: []string{"arbeidsmiljø"}, Limit: 5})
	if err != nil {
		t.Fatalf("Search() error = %v", err)
	}
	if result.Source.Provider != "lovdata" || len(result.Data) == 0 {
		t.Fatalf("unexpected result: %+v", result)
	}
}

func TestSearch_RequiresConfiguredAPIKey(t *testing.T) {
	svc := legal.NewServiceWithURL(http.DefaultClient, cache.New(), "http://unused", "")
	_, err := svc.Search(context.Background(), legal.Request{Terms: []string{"law"}})
	if err == nil {
		t.Fatal("expected missing API key to fail")
	}
}
