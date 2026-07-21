package frost

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestObservationsUsesBasicClientIDAndBoundedParameters(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		username, password, ok := r.BasicAuth()
		if !ok || username != "frost-client" || password != "" {
			t.Fatalf("auth = %q/%q/%v", username, password, ok)
		}
		query := r.URL.Query()
		if query.Get("sources") != "SN18700" || query.Get("elements") != "air_temperature" || query.Get("referencetime") != "2026-07-21/2026-07-22" {
			t.Fatalf("query = %v", query)
		}
		w.Header().Set("Content-Type", "application/ld+json")
		_, _ = w.Write([]byte(`{"data":[{"sourceId":"SN18700"}]}`))
	}))
	defer server.Close()

	service := NewService(server.Client(), server.URL, "frost-client")
	response, err := service.Observations(context.Background(), Request{Sources: "SN18700", Elements: "air_temperature", ReferenceTime: "2026-07-21/2026-07-22"})
	if err != nil {
		t.Fatalf("Observations() error = %v", err)
	}
	if response.Source.Provider != "met-norway" || response.Source.APIVersion != "v0" || len(response.Data) == 0 {
		t.Fatalf("response = %+v", response)
	}
}

func TestObservationsFailsClosedWithoutClientID(t *testing.T) {
	service := NewService(http.DefaultClient, "https://frost.met.no/observations/v0.jsonld", "")
	if _, err := service.Observations(context.Background(), Request{Sources: "SN18700", Elements: "air_temperature", ReferenceTime: "latest"}); err != ErrNotConfigured {
		t.Fatalf("error = %v, want ErrNotConfigured", err)
	}
}

func TestObservationsRejectsInvalidBounds(t *testing.T) {
	service := NewService(http.DefaultClient, "https://example.test/frost", "client")
	if _, err := service.Observations(context.Background(), Request{Sources: "all", Elements: "air_temperature", ReferenceTime: "latest"}); err == nil {
		t.Fatal("Observations() error = nil for invalid sources")
	}
}
