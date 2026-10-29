package parliament_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/parliament"
)

func TestCurrentRepresentatives_NormalizesPublicFieldsOnly(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/eksport/dagensrepresentanter" || r.URL.Query().Get("format") != "JSON" {
			t.Errorf("unexpected request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"versjon":"1.6","dagensrepresentanter_liste":[{"id":"ABC","fornavn":"Ada","etternavn":"Nord","epost":"private@example.test","parti":{"id":"P","navn":"Partiet"},"fylke":{"id":"OS","navn":"Oslo"},"komiteer_liste":[{"id":"K","navn":"Komiteen"}]}]}`))
	}))
	defer server.Close()

	svc := parliament.NewServiceWithURL(http.DefaultClient, cache.New(), server.URL+"/eksport/dagensrepresentanter")
	result, err := svc.CurrentRepresentatives(context.Background())
	if err != nil {
		t.Fatalf("CurrentRepresentatives() error = %v", err)
	}
	if len(result.Data) != 1 || result.Data[0].ID != "ABC" || result.Data[0].Email != "" {
		t.Fatalf("unexpected normalized data: %+v", result.Data)
	}
	if result.Source.Provider != "stortinget" {
		t.Fatalf("unexpected source: %+v", result.Source)
	}
}
