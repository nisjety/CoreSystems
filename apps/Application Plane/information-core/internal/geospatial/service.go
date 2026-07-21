package geospatial

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const (
	defaultPropertyURL = "https://api.kartverket.no/eiendom/v1/geokoding"
	defaultAirURL      = "https://api-luftmalinger.miljodirektoratet.no"
	defaultNVDBURL     = "https://nvdbapiles.atlas.vegvesen.no/vegobjekter/api/v4"
	defaultNVEURL      = "https://api01.nve.no/hydrology/forecast/avalanche/v6.3.2/api"
	defaultHeritageURL = "https://api.ra.no/KulturminnerKulturmiljoer"
	defaultNVDBClient  = "coresystem-information-core"
)

var (
	ErrInvalidRequest   = errors.New("geospatial: invalid request")
	municipalityPattern = regexp.MustCompile(`^[0-9]{4}$`)
	datePattern         = regexp.MustCompile(`^[0-9]{4}-[0-9]{2}-[0-9]{2}(T[0-9]{2}:[0-9]{2})?$`)
	languagePattern     = regexp.MustCompile(`^(no|en)$`)
)

type Service struct {
	client      *http.Client
	cache       *cache.Store
	propertyURL string
	airURL      string
	nvdbURL     string
	nveURL      string
	heritageURL string
	nvdbClient  string
}

type Response struct {
	Source provenance.Source `json:"source"`
	Data   json.RawMessage   `json:"data"`
}

type PropertyRequest struct {
	MatrikkelNumber string `json:"matrikkelnummer,omitempty"`
}

type RoadRequest struct {
	ObjectType   int    `json:"objectType"`
	Municipality string `json:"municipality"`
	Limit        int    `json:"limit,omitempty"`
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore, propertyURL: defaultPropertyURL, airURL: defaultAirURL, nvdbURL: defaultNVDBURL, nveURL: defaultNVEURL, heritageURL: defaultHeritageURL, nvdbClient: defaultNVDBClient}
}

func NewServiceWithURLs(client *http.Client, cacheStore *cache.Store, propertyURL, airURL, nvdbURL string) *Service {
	service := NewService(client, cacheStore)
	service.propertyURL, service.airURL, service.nvdbURL = strings.TrimRight(propertyURL, "/"), strings.TrimRight(airURL, "/"), strings.TrimRight(nvdbURL, "/")
	return service
}

func NewServiceWithProviderURLs(client *http.Client, cacheStore *cache.Store, propertyURL, airURL, nvdbURL, nveURL, heritageURL string) *Service {
	service := NewServiceWithURLs(client, cacheStore, propertyURL, airURL, nvdbURL)
	service.nveURL, service.heritageURL = strings.TrimRight(nveURL, "/"), strings.TrimRight(heritageURL, "/")
	return service
}

func (s *Service) PropertyLookup(ctx context.Context, input PropertyRequest) (Response, error) {
	matrikkel := strings.TrimSpace(input.MatrikkelNumber)
	if matrikkel == "" || len([]rune(matrikkel)) > 80 || strings.ContainsAny(matrikkel, "?&#\r\n") {
		return Response{}, fmt.Errorf("%w: matrikkelnummer is required and bounded", ErrInvalidRequest)
	}
	query := url.Values{"matrikkelnummer": []string{matrikkel}, "utkoordsys": []string{"4258"}}
	return s.getJSON(ctx, s.propertyURL+"?"+query.Encode(), "kartverket", "open-property-location-v1.1.0", s.propertyURL, "open_property_location", "partial", 3600)
}

func (s *Service) AirQuality(ctx context.Context, lat, lon float64, radius int) (Response, error) {
	if !finite(lat, -90, 90) || !finite(lon, -180, 180) || radius < 1 || radius > 100 {
		return Response{}, fmt.Errorf("%w: coordinates or radius are outside bounds", ErrInvalidRequest)
	}
	path := fmt.Sprintf("%s/public/obs/utd/%s/%s/%d", s.airURL, formatCoordinate(lat), formatCoordinate(lon), radius)
	return s.getJSON(ctx, path, "miljodirektoratet", "air-quality-observations", s.airURL, "local_air_quality_observations", "measured", 60)
}

func (s *Service) RoadObjects(ctx context.Context, input RoadRequest) (Response, error) {
	if input.ObjectType < 1 || input.ObjectType > 99999 || !municipalityPattern.MatchString(strings.TrimSpace(input.Municipality)) {
		return Response{}, fmt.Errorf("%w: objectType or municipality is invalid", ErrInvalidRequest)
	}
	if input.Limit == 0 {
		input.Limit = 50
	}
	if input.Limit < 1 || input.Limit > 50 {
		return Response{}, fmt.Errorf("%w: limit must be between 1 and 50", ErrInvalidRequest)
	}
	query := url.Values{"kommune": []string{input.Municipality}, "antall": []string{strconv.Itoa(input.Limit)}, "inkluder": []string{"egenskaper,geometri"}}
	path := fmt.Sprintf("%s/vegobjekter/%d?%s", s.nvdbURL, input.ObjectType, query.Encode())
	return s.getJSON(ctx, path, "statens-vegvesen", "nvdb-api-les-v4", s.nvdbURL, "road-object-attributes", "measured", 300)
}

// AvalancheWarnings returns NVE's published avalanche warning for a bounded
// coordinate/date window. It is a warning product, not a property-safety or
// landslide conclusion.
func (s *Service) AvalancheWarnings(ctx context.Context, lat, lon float64, language, startDate, endDate string) (Response, error) {
	if !finite(lat, -90, 90) || !finite(lon, -180, 180) || !languagePattern.MatchString(language) || !datePattern.MatchString(startDate) || !datePattern.MatchString(endDate) {
		return Response{}, fmt.Errorf("%w: avalanche warning coordinates, language, or dates are invalid", ErrInvalidRequest)
	}
	path := fmt.Sprintf("%s/Warning/Coordinate/%s/%s/%s/%s/%s", s.nveURL, formatCoordinate(lon), formatCoordinate(lat), language, startDate, endDate)
	return s.getJSONWithCRS(ctx, path, "nve", "avalanche-warning-v6.3.2", s.nveURL, "avalanche_warning", "forecast", 300, "EPSG:4326")
}

// HeritageFeatures returns a bounded GeoJSON feature collection from the
// public Riksantikvaren kulturmiljoer OGC collection.
func (s *Service) HeritageFeatures(ctx context.Context, minLon, minLat, maxLon, maxLat float64, limit int) (Response, error) {
	if !finite(minLon, -180, 180) || !finite(maxLon, -180, 180) || !finite(minLat, -90, 90) || !finite(maxLat, -90, 90) || minLon >= maxLon || minLat >= maxLat {
		return Response{}, fmt.Errorf("%w: heritage bbox is invalid", ErrInvalidRequest)
	}
	if limit == 0 {
		limit = 50
	}
	if limit < 1 || limit > 100 {
		return Response{}, fmt.Errorf("%w: heritage limit must be between 1 and 100", ErrInvalidRequest)
	}
	query := url.Values{
		"f":     []string{"json"},
		"bbox":  []string{fmt.Sprintf("%s,%s,%s,%s", formatCoordinate(minLon), formatCoordinate(minLat), formatCoordinate(maxLon), formatCoordinate(maxLat))},
		"limit": []string{strconv.Itoa(limit)},
	}
	path := s.heritageURL + "/collections/kulturmiljoer/items?" + query.Encode()
	return s.getJSONWithCRS(ctx, path, "riksantikvaren", "kulturmiljoer-ogc-api-features", s.heritageURL, "cultural_heritage_features", "measured", 900, "CRS84")
}

// AggregateAirQuality returns a bounded, time-windowed aggregate. The source
// supports at most 20 km radius for this endpoint; no synthetic interpolation
// is introduced here.
func (s *Service) AggregateAirQuality(ctx context.Context, meanType int, fromTime, toTime string, lat, lon float64, radius float64, method string) (Response, error) {
	if meanType < 1 || meanType > 9999 || !datePattern.MatchString(fromTime) || !datePattern.MatchString(toTime) || !finite(lat, -90, 90) || !finite(lon, -180, 180) || !finite(radius, 0.001, 20) {
		return Response{}, fmt.Errorf("%w: aggregate air-quality input is invalid", ErrInvalidRequest)
	}
	method = strings.TrimSpace(method)
	if method == "" {
		method = "within"
	}
	if method != "within" {
		return Response{}, fmt.Errorf("%w: aggregate air-quality method must be within", ErrInvalidRequest)
	}
	path := fmt.Sprintf("%s/public/agg/%d/%s/%s/%s/%s/%s?method=%s", s.airURL, meanType, fromTime, toTime, formatCoordinate(lat), formatCoordinate(lon), formatCoordinate(radius), url.QueryEscape(method))
	return s.getJSONWithCRS(ctx, path, "miljodirektoratet", "air-quality-aggregate", s.airURL, "local_air_quality_aggregate", "measured", 300, "EPSG:4258")
}

func (s *Service) getJSON(ctx context.Context, endpoint, provider, dataset, sourceURL, coverage, status string, ttl int) (Response, error) {
	return s.getJSONWithCRS(ctx, endpoint, provider, dataset, sourceURL, coverage, status, ttl, "EPSG:4258")
}

func (s *Service) getJSONWithCRS(ctx context.Context, endpoint, provider, dataset, sourceURL, coverage, status string, ttl int, crs string) (Response, error) {
	key := provider + ":" + endpoint
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return Response{}, fmt.Errorf("geospatial: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	if provider == "statens-vegvesen" {
		req.Header.Set("X-Client", s.nvdbClient)
	}
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("geospatial: upstream: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return Response{}, fmt.Errorf("geospatial: read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("geospatial: upstream returned HTTP %d", resp.StatusCode)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("geospatial: upstream returned invalid JSON")
	}
	result := Response{Source: provenance.Source{Provider: provider, Dataset: dataset, SourceURL: sourceURL, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: coverage, Status: status, CRS: crs}, Data: json.RawMessage(data)}
	s.cache.Set(key, config.TTL(ttl), result)
	return result, nil
}

func finite(value, min, max float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0) && value >= min && value <= max
}
func formatCoordinate(value float64) string { return strconv.FormatFloat(value, 'f', 6, 64) }
