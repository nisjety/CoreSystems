package traffic

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"
	"net/http"
	"strings"
	"testing"

	"coresystem/apps/application-plane/information-core/internal/cache"
)

type roundTripFunc func(*http.Request) (*http.Response, error)

func (fn roundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return fn(request)
}

func TestLatestDoesNotInventTrafficMeasurements(t *testing.T) {
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{
			StatusCode: http.StatusOK,
			Body: io.NopCloser(strings.NewReader(`{
				"data":{"trafficRegistrationPoints":[{
					"id":"station-1","name":"E6 Oslo",
					"location":{"coordinates":{"latLon":{"lat":59.9,"lon":10.7}}}
				}]}
			}`)),
			Header: make(http.Header),
		}, nil
	})}

	response, err := NewService(client, cache.New()).Latest(context.Background(), 0, 0, 0, "")
	if err != nil {
		t.Fatalf("Latest() error = %v", err)
	}
	if len(response.Data) != 1 {
		t.Fatalf("Latest() stations = %d, want 1", len(response.Data))
	}

	station := response.Data[0]
	if station.RoadReferenceProvenance.ObservationType != ObservationEstimated ||
		station.RoadReferenceProvenance.Source != "station_name_prefix" {
		t.Fatalf("road reference provenance = %+v", station.RoadReferenceProvenance)
	}
	if station.CountyProvenance.ObservationType != ObservationEstimated ||
		station.CountyProvenance.Quality != "low" {
		t.Fatalf("county provenance = %+v", station.CountyProvenance)
	}
	for name, observation := range map[string]Observation{
		"trafficVolume": station.TrafficVolume,
		"averageSpeed":  station.AverageSpeed,
	} {
		if observation.Value != nil {
			t.Errorf("%s value = %v, want unavailable/null", name, *observation.Value)
		}
		if observation.ObservationType != ObservationUnavailable {
			t.Errorf("%s observationType = %q, want %q", name, observation.ObservationType, ObservationUnavailable)
		}
		if observation.Provider != "statens_vegvesen_atlas" {
			t.Errorf("%s provider = %q", name, observation.Provider)
		}
		if observation.UnavailableReason == "" || observation.FetchedAt == "" || observation.Unit == "" {
			t.Errorf("%s missing provenance: %+v", name, observation)
		}
	}

	encoded, err := json.Marshal(station)
	if err != nil {
		t.Fatalf("json.Marshal() error = %v", err)
	}
	if strings.Contains(string(encoded), `"observationType":"measured"`) {
		t.Fatalf("synthetic value serialized as measured: %s", encoded)
	}
	if !strings.Contains(string(encoded), `"value":null`) {
		t.Fatalf("unavailable value was not explicit null: %s", encoded)
	}
}

func TestLatestRejectsUpstreamHTTPFailure(t *testing.T) {
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		return &http.Response{
			StatusCode: http.StatusServiceUnavailable,
			Body:       io.NopCloser(strings.NewReader(`{"error":"maintenance"}`)),
			Header:     make(http.Header),
		}, nil
	})}

	_, err := NewService(client, cache.New()).Latest(context.Background(), 0, 0, 0, "")
	if err == nil {
		t.Fatal("Latest() error = nil, want upstream status error")
	}
}

func TestLatestRejectsGraphQLErrorsInsteadOfCachingEmptySuccess(t *testing.T) {
	calls := 0
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		calls++
		return &http.Response{
			StatusCode: http.StatusOK,
			Body:       io.NopCloser(strings.NewReader(`{"errors":[{"message":"resolver unavailable"}],"data":{"trafficRegistrationPoints":[]}}`)),
			Header:     make(http.Header),
		}, nil
	})}
	service := NewService(client, cache.New())
	for attempt := 0; attempt < 2; attempt++ {
		if _, err := service.Latest(context.Background(), 0, 0, 0, ""); err == nil {
			t.Fatal("Latest() error = nil, want GraphQL error")
		}
	}
	if calls != 2 {
		t.Fatalf("upstream calls = %d, want 2 because failures must not be cached", calls)
	}
}

func TestLatestRejectsTransportAndMalformedPayload(t *testing.T) {
	t.Run("transport", func(t *testing.T) {
		client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
			return nil, errors.New("network unavailable")
		})}
		if _, err := NewService(client, cache.New()).Latest(context.Background(), 0, 0, 0, ""); err == nil {
			t.Fatal("Latest() error = nil, want transport error")
		}
	})

	t.Run("malformed JSON", func(t *testing.T) {
		client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
			return &http.Response{
				StatusCode: http.StatusOK,
				Body:       io.NopCloser(strings.NewReader(`{"data":`)),
				Header:     make(http.Header),
			}, nil
		})}
		if _, err := NewService(client, cache.New()).Latest(context.Background(), 0, 0, 0, ""); err == nil {
			t.Fatal("Latest() error = nil, want decode error")
		}
	})
}

func TestBuildGraphQLQueryAndMetadataHelpers(t *testing.T) {
	nearby := buildGraphQLQuery(59.9, 10.7, "oslo")
	if !strings.Contains(nearby, "roadCategoryIds: [R, E, F, K]") {
		t.Fatalf("search query did not request supported road categories: %s", nearby)
	}
	defaultQuery := buildGraphQLQuery(0, 0, "")
	if !strings.Contains(defaultQuery, "roadCategoryIds: [R, E]") {
		t.Fatalf("default query categories = %s", defaultQuery)
	}

	roadCases := map[string]string{
		"":             "",
		"E6 Oslo":      "E6",
		"Rv4 Nittedal": "Rv4",
		"Fv33 Gjøvik":  "Fv33",
		"Ring 3":       "",
	}
	for input, expected := range roadCases {
		if got := roadRefFromName(input); got != expected {
			t.Errorf("roadRefFromName(%q) = %q, want %q", input, got, expected)
		}
	}

	countyCases := []struct {
		lat, lon float64
		want     string
	}{
		{59.9, 10.7, "Oslo"},
		{59.5, 11.2, "Akershus"},
		{60.5, 10.5, "Innlandet"},
		{63.4, 10.4, ""},
	}
	for _, testCase := range countyCases {
		if got := countyNameFor(testCase.lat, testCase.lon); got != testCase.want {
			t.Errorf("countyNameFor(%v, %v) = %q, want %q", testCase.lat, testCase.lon, got, testCase.want)
		}
	}
}

func TestLatestRejectsInvalidOrUnboundedQueryCoordinates(t *testing.T) {
	service := NewService(http.DefaultClient, cache.New())
	for _, testCase := range []struct {
		name             string
		lat, lon, radius float64
		search           string
	}{
		{name: "nan", lat: math.NaN(), lon: 10},
		{name: "latitude", lat: 91, lon: 10},
		{name: "longitude", lat: 59, lon: 181},
		{name: "negative radius", lat: 59, lon: 10, radius: -1},
		{name: "search length", lat: 59, lon: 10, search: strings.Repeat("x", 101)},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			if _, err := service.Latest(context.Background(), testCase.lat, testCase.lon, testCase.radius, testCase.search); err == nil {
				t.Fatal("Latest() error = nil, want validation error")
			}
		})
	}
}

func TestLatestUsesCachedProvenanceWithoutRefetching(t *testing.T) {
	calls := 0
	client := &http.Client{Transport: roundTripFunc(func(*http.Request) (*http.Response, error) {
		calls++
		return &http.Response{
			StatusCode: http.StatusOK,
			Body: io.NopCloser(strings.NewReader(`{
				"data":{"trafficRegistrationPoints":[]}
			}`)),
			Header: make(http.Header),
		}, nil
	})}
	service := NewService(client, cache.New())
	first, err := service.Latest(context.Background(), 0, 0, 0, "")
	if err != nil {
		t.Fatalf("first Latest() error = %v", err)
	}
	second, err := service.Latest(context.Background(), 0, 0, 0, "")
	if err != nil {
		t.Fatalf("second Latest() error = %v", err)
	}
	if calls != 1 {
		t.Fatalf("upstream calls = %d, want 1", calls)
	}
	if first.Timestamp != second.Timestamp {
		t.Fatalf("cached provenance timestamp changed: %q != %q", first.Timestamp, second.Timestamp)
	}
}

func TestDistanceKm(t *testing.T) {
	got := round2(distanceKm(59.9139, 10.7522, 59.9127, 10.7461))
	if got <= 0 {
		t.Fatalf("distanceKm() = %v", got)
	}
}
