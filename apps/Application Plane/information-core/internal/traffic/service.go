package traffic

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"math"
	"net/http"
	"strconv"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
)

const atlasURL = "https://trafikkdata-api.atlas.vegvesen.no"

type Service struct {
	client *http.Client
	cache  *cache.Store
}

type Station struct {
	ID                      string                 `json:"id"`
	Name                    string                 `json:"name"`
	LocationName            string                 `json:"locationName"`
	County                  string                 `json:"county,omitempty"`
	CountyProvenance        DerivedFieldProvenance `json:"countyProvenance"`
	RoadRef                 string                 `json:"roadReference,omitempty"`
	RoadReferenceProvenance DerivedFieldProvenance `json:"roadReferenceProvenance"`
	TrafficVolume           Observation            `json:"trafficVolume"`
	AverageSpeed            Observation            `json:"averageSpeed"`
	Status                  string                 `json:"status"`
	DistanceKm              float64                `json:"distanceKm,omitempty"`
	LastUpdated             string                 `json:"lastUpdated"`
	Coordinates             struct {
		Lat float64 `json:"lat"`
		Lon float64 `json:"lon"`
	} `json:"coordinates"`
}

type DerivedFieldProvenance struct {
	ObservationType ObservationType `json:"observationType"`
	Source          string          `json:"source"`
	Quality         string          `json:"quality"`
}

type ObservationType string

const (
	ObservationMeasured    ObservationType = "measured"
	ObservationEstimated   ObservationType = "estimated"
	ObservationSynthetic   ObservationType = "synthetic"
	ObservationUnavailable ObservationType = "unavailable"
)

type Observation struct {
	Value             *float64        `json:"value"`
	Unit              string          `json:"unit"`
	ObservationType   ObservationType `json:"observationType"`
	Provider          string          `json:"provider"`
	Source            string          `json:"source"`
	ObservedAt        *string         `json:"observedAt"`
	FetchedAt         string          `json:"fetchedAt"`
	Confidence        *float64        `json:"confidence"`
	Quality           string          `json:"quality"`
	Freshness         string          `json:"freshness"`
	UnavailableReason string          `json:"unavailableReason,omitempty"`
}

type Response struct {
	Success   bool      `json:"success"`
	Data      []Station `json:"data"`
	Timestamp string    `json:"timestamp"`
}

type atlasResponse struct {
	Errors []struct {
		Message string `json:"message"`
	} `json:"errors"`
	Data struct {
		Points []struct {
			ID       string `json:"id"`
			Name     string `json:"name"`
			Location struct {
				Coordinates struct {
					LatLon struct {
						Lat float64 `json:"lat"`
						Lon float64 `json:"lon"`
					} `json:"latLon"`
				} `json:"coordinates"`
			} `json:"location"`
		} `json:"trafficRegistrationPoints"`
	} `json:"data"`
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore}
}

func (s *Service) Latest(ctx context.Context, lat, lon, radius float64, search string) (Response, error) {
	if !isFinite(lat) || !isFinite(lon) || !isFinite(radius) || lat < -90 || lat > 90 || lon < -180 || lon > 180 {
		return Response{}, fmt.Errorf("traffic coordinates are invalid")
	}
	if radius < 0 || radius > 1000 {
		return Response{}, fmt.Errorf("traffic radius must be between 0 and 1000 km")
	}
	if len([]rune(strings.TrimSpace(search))) > 100 {
		return Response{}, fmt.Errorf("traffic search must be at most 100 characters")
	}
	key := fmt.Sprintf("traffic:%0.4f:%0.4f:%0.1f:%s", lat, lon, radius, strings.ToLower(strings.TrimSpace(search)))
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}

	query := buildGraphQLQuery(lat, lon, search)
	body, _ := json.Marshal(map[string]string{"query": query})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, atlasURL, bytes.NewReader(body))
	if err != nil {
		return Response{}, err
	}
	req.Header.Set("Content-Type", "application/json")

	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("atlas traffic API returned HTTP %d", resp.StatusCode)
	}

	var atlas atlasResponse
	if err := json.NewDecoder(resp.Body).Decode(&atlas); err != nil {
		return Response{}, err
	}
	if len(atlas.Errors) > 0 {
		return Response{}, fmt.Errorf("atlas traffic API returned a GraphQL error")
	}

	fetchedAt := time.Now().UTC().Format(time.RFC3339)
	stations := make([]Station, 0, len(atlas.Data.Points))
	for _, point := range atlas.Data.Points {
		station := Station{
			ID:            point.ID,
			Name:          point.Name,
			LocationName:  point.Name,
			County:        countyNameFor(point.Location.Coordinates.LatLon.Lat, point.Location.Coordinates.LatLon.Lon),
			RoadRef:       roadRefFromName(point.Name),
			TrafficVolume: unavailableObservation("vehicles_per_hour", fetchedAt),
			AverageSpeed:  unavailableObservation("km/h", fetchedAt),
			Status:        "metadata_only",
			LastUpdated:   fetchedAt,
			CountyProvenance: DerivedFieldProvenance{
				ObservationType: ObservationEstimated,
				Source:          "application_geographic_bounding_box",
				Quality:         "low",
			},
			RoadReferenceProvenance: DerivedFieldProvenance{
				ObservationType: ObservationEstimated,
				Source:          "station_name_prefix",
				Quality:         "low",
			},
		}
		station.Coordinates.Lat = point.Location.Coordinates.LatLon.Lat
		station.Coordinates.Lon = point.Location.Coordinates.LatLon.Lon

		if lat != 0 && lon != 0 {
			station.DistanceKm = round2(distanceKm(lat, lon, station.Coordinates.Lat, station.Coordinates.Lon))
			if radius > 0 && station.DistanceKm > radius {
				continue
			}
		}

		if search != "" && !strings.Contains(strings.ToLower(station.Name), strings.ToLower(search)) {
			continue
		}

		stations = append(stations, station)
		if len(stations) == 6 {
			break
		}
	}

	result := Response{
		Success:   true,
		Data:      stations,
		Timestamp: time.Now().UTC().Format(time.RFC3339),
	}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}

func buildGraphQLQuery(lat, lon float64, search string) string {
	conditions := []string{"isOperational: true"}
	if lat != 0 && lon != 0 {
		conditions = append(conditions, "roadCategoryIds: [R, E, F]")
	} else {
		conditions = append(conditions, "roadCategoryIds: [R, E]")
	}
	if strings.TrimSpace(search) != "" {
		conditions = append(conditions, "roadCategoryIds: [R, E, F, K]")
	}
	return fmt.Sprintf(`
		query {
			trafficRegistrationPoints(searchQuery: {%s}) {
				id
				name
				location {
					coordinates {
						latLon {
							lat
							lon
						}
					}
				}
			}
		}
	`, strings.Join(conditions, ", "))
}

func unavailableObservation(unit, fetchedAt string) Observation {
	return Observation{
		Value:             nil,
		Unit:              unit,
		ObservationType:   ObservationUnavailable,
		Provider:          "statens_vegvesen_atlas",
		Source:            atlasURL,
		ObservedAt:        nil,
		FetchedAt:         fetchedAt,
		Confidence:        nil,
		Quality:           "provider_metadata_only",
		Freshness:         "unavailable",
		UnavailableReason: "provider_response_has_no_measurement",
	}
}

func roadRefFromName(name string) string {
	fields := strings.Fields(name)
	if len(fields) == 0 {
		return ""
	}
	prefix := fields[0]
	if strings.HasPrefix(prefix, "E") || strings.HasPrefix(prefix, "Rv") || strings.HasPrefix(prefix, "Fv") {
		return prefix
	}
	return ""
}

func countyNameFor(lat, lon float64) string {
	switch {
	case lat > 59.7 && lat < 60.1 && lon > 10.4 && lon < 11.1:
		return "Oslo"
	case lat > 59.0 && lat < 60.0 && lon > 10.0 && lon < 11.7:
		return "Akershus"
	case lat > 60.2 && lat < 61.0 && lon > 10.0 && lon < 11.5:
		return "Innlandet"
	default:
		return ""
	}
}

func isFinite(value float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0)
}

func distanceKm(lat1, lon1, lat2, lon2 float64) float64 {
	const earthRadius = 6371
	dLat := degreesToRadians(lat2 - lat1)
	dLon := degreesToRadians(lon2 - lon1)
	a := math.Sin(dLat/2)*math.Sin(dLat/2) +
		math.Cos(degreesToRadians(lat1))*math.Cos(degreesToRadians(lat2))*
			math.Sin(dLon/2)*math.Sin(dLon/2)
	return earthRadius * 2 * math.Atan2(math.Sqrt(a), math.Sqrt(1-a))
}

func degreesToRadians(deg float64) float64 {
	return deg * math.Pi / 180
}

func round2(value float64) float64 {
	v, _ := strconv.ParseFloat(fmt.Sprintf("%.2f", value), 64)
	return v
}
