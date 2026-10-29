package geospatial_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/geospatial"
)

func TestPropertyLookup_UsesOpenKartverketContract(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/geokoding" || r.URL.Query().Get("matrikkelnummer") != "0301-223/60" || r.URL.Query().Get("utkoordsys") != "4258" {
			t.Errorf("unexpected property request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"metadata":{"status":"ok"},"representasjonspunkt":{"lat":59.9,"lon":10.7}}`))
	}))
	defer server.Close()

	svc := geospatial.NewServiceWithURLs(http.DefaultClient, cache.New(), server.URL+"/geokoding", server.URL, server.URL)
	result, err := svc.PropertyLookup(context.Background(), geospatial.PropertyRequest{MatrikkelNumber: "0301-223/60"})
	if err != nil {
		t.Fatalf("PropertyLookup() error = %v", err)
	}
	if result.Source.Dataset != "open-property-location-v1.1.0" || len(result.Data) == 0 {
		t.Fatalf("unexpected property result: %+v", result)
	}
}

func TestAirQuality_ValidatesCoordinatesAndBoundsRadius(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/public/obs/utd/59.900000/10.700000/10" {
			t.Errorf("unexpected air-quality path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`[{"component":"PM10","value":12.3}]`))
	}))
	defer server.Close()

	svc := geospatial.NewServiceWithURLs(http.DefaultClient, cache.New(), "http://unused", server.URL, "http://unused")
	result, err := svc.AirQuality(context.Background(), 59.9, 10.7, 10)
	if err != nil || len(result.Data) == 0 {
		t.Fatalf("AirQuality() result=%+v error=%v", result, err)
	}
	if result.Source.Provider != "miljodirektoratet" {
		t.Fatalf("unexpected source: %+v", result.Source)
	}
	if _, err := svc.AirQuality(context.Background(), 59.9, 10.7, 1001); err == nil {
		t.Fatal("expected radius bound error")
	}
}

func TestRoadObjects_ResolvesNVDBV4Endpoint(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/vegobjekter/105" || r.URL.Query().Get("kommune") != "0301" || r.URL.Query().Get("antall") != "10" {
			t.Errorf("unexpected NVDB request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		if r.Header.Get("X-Client") != "coresystem-information-core" {
			t.Errorf("unexpected NVDB X-Client header: %q", r.Header.Get("X-Client"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"objekter":[]}`))
	}))
	defer server.Close()

	svc := geospatial.NewServiceWithURLs(http.DefaultClient, cache.New(), "http://unused", "http://unused", server.URL)
	result, err := svc.RoadObjects(context.Background(), geospatial.RoadRequest{ObjectType: 105, Municipality: "0301", Limit: 10})
	if err != nil || result.Source.Dataset != "nvdb-api-les-v4" {
		t.Fatalf("RoadObjects() result=%+v error=%v", result, err)
	}
}

func TestAvalancheWarnings_UsesVersionedNVEContract(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/nve/Warning/Coordinate/10.700000/59.900000/en/2026-07-21/2026-07-22" {
			t.Errorf("unexpected NVE path: %s", r.URL.Path)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"dangerLevel":2,"mainMessage":"Moderate"}`))
	}))
	defer server.Close()

	svc := geospatial.NewServiceWithProviderURLs(http.DefaultClient, cache.New(), "http://unused", "http://unused", "http://unused", server.URL+"/nve", "http://unused")
	result, err := svc.AvalancheWarnings(context.Background(), 59.9, 10.7, "en", "2026-07-21", "2026-07-22")
	if err != nil || result.Source.Dataset != "avalanche-warning-v6.3.2" {
		t.Fatalf("AvalancheWarnings() result=%+v error=%v", result, err)
	}
}

func TestHeritageFeatures_UsesBoundedOGCBBox(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ra/collections/kulturmiljoer/items" || r.URL.Query().Get("bbox") != "10.000000,59.000000,10.700000,59.900000" || r.URL.Query().Get("limit") != "25" {
			t.Errorf("unexpected heritage request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		w.Header().Set("Content-Type", "application/geo+json")
		_, _ = w.Write([]byte(`{"type":"FeatureCollection","features":[]}`))
	}))
	defer server.Close()

	svc := geospatial.NewServiceWithProviderURLs(http.DefaultClient, cache.New(), "http://unused", "http://unused", "http://unused", "http://unused", server.URL+"/ra")
	result, err := svc.HeritageFeatures(context.Background(), 10, 59, 10.7, 59.9, 25)
	if err != nil || result.Source.CRS != "CRS84" {
		t.Fatalf("HeritageFeatures() result=%+v error=%v", result, err)
	}
}

func TestAggregateAirQuality_ValidatesTwentyKilometerRadius(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/public/agg/1/2026-07-20/2026-07-21/59.900000/10.700000/5.000000" || r.URL.Query().Get("method") != "within" {
			t.Errorf("unexpected aggregate air-quality request: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`[{"value":12.3}]`))
	}))
	defer server.Close()

	svc := geospatial.NewServiceWithProviderURLs(http.DefaultClient, cache.New(), "http://unused", server.URL, "http://unused", "http://unused", "http://unused")
	result, err := svc.AggregateAirQuality(context.Background(), 1, "2026-07-20", "2026-07-21", 59.9, 10.7, 5, "")
	if err != nil || result.Source.Dataset != "air-quality-aggregate" {
		t.Fatalf("AggregateAirQuality() result=%+v error=%v", result, err)
	}
	if _, err := svc.AggregateAirQuality(context.Background(), 1, "2026-07-20", "2026-07-21", 59.9, 10.7, 20.1, ""); err == nil {
		t.Fatal("expected aggregate radius bound error")
	}
}
