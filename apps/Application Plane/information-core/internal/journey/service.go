package journey

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const defaultURL = "https://api.entur.io/journey-planner/v3/graphql"

var ErrInvalidRequest = errors.New("journey: invalid request")

type Service struct {
	client     *http.Client
	cache      *cache.Store
	endpoint   string
	clientName string
}

type Place struct {
	ID   string  `json:"id,omitempty"`
	Name string  `json:"name,omitempty"`
	Lat  float64 `json:"lat,omitempty"`
	Lon  float64 `json:"lon,omitempty"`
}

type PlanRequest struct {
	From            Place `json:"from"`
	To              Place `json:"to"`
	NumTripPatterns int   `json:"numTripPatterns,omitempty"`
}

type Response struct {
	Source provenance.Source `json:"source"`
	Data   json.RawMessage   `json:"data"`
}

type graphQLRequest struct {
	Query     string         `json:"query"`
	Variables map[string]any `json:"variables"`
}

func NewService(client *http.Client, cacheStore *cache.Store, clientName string) *Service {
	return &Service{client: client, cache: cacheStore, endpoint: defaultURL, clientName: strings.TrimSpace(clientName)}
}

func NewServiceWithURL(client *http.Client, cacheStore *cache.Store, endpoint, clientName string) *Service {
	service := NewService(client, cacheStore, clientName)
	service.endpoint = endpoint
	return service
}

func (s *Service) Plan(ctx context.Context, input PlanRequest) (Response, error) {
	if s.clientName == "" || !validPlace(input.From) || !validPlace(input.To) {
		return Response{}, fmt.Errorf("%w: client name and both places are required", ErrInvalidRequest)
	}
	if input.NumTripPatterns == 0 {
		input.NumTripPatterns = 3
	}
	if input.NumTripPatterns < 1 || input.NumTripPatterns > 5 {
		return Response{}, fmt.Errorf("%w: numTripPatterns must be between 1 and 5", ErrInvalidRequest)
	}
	body, err := json.Marshal(graphQLRequest{
		Query: `query($from: Location!, $to: Location!, $numTripPatterns: Int) { trip(from: $from, to: $to, numTripPatterns: $numTripPatterns) { tripPatterns { startTime endTime duration walkDistance legs { mode distance aimedStartTime aimedEndTime expectedStartTime expectedEndTime fromPlace { name } toPlace { name } line { id publicCode name } serviceJourney { id } } } } }`,
		Variables: map[string]any{
			"from": placeInput(input.From), "to": placeInput(input.To), "numTripPatterns": input.NumTripPatterns,
		},
	})
	if err != nil {
		return Response{}, fmt.Errorf("journey: encode request: %w", err)
	}
	hash := sha256.Sum256(body)
	key := "entur:journey:" + hex.EncodeToString(hash[:])
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, s.endpoint, bytes.NewReader(body))
	if err != nil {
		return Response{}, fmt.Errorf("journey: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("ET-Client-Name", s.clientName)
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("journey: upstream: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if err != nil {
		return Response{}, fmt.Errorf("journey: read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("journey: upstream returned HTTP %d", resp.StatusCode)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("journey: upstream returned invalid JSON")
	}
	var envelope struct {
		Errors []struct {
			Message string `json:"message"`
		} `json:"errors"`
	}
	if err := json.Unmarshal(data, &envelope); err != nil {
		return Response{}, fmt.Errorf("journey: decode response: %w", err)
	}
	if len(envelope.Errors) > 0 {
		return Response{}, fmt.Errorf("journey: upstream GraphQL error: %s", envelope.Errors[0].Message)
	}
	result := Response{Source: provenance.Source{Provider: "entur", Dataset: "journey-planner-v3", SourceURL: defaultURL, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: "national_public_transport", Status: "partial", APIVersion: "3"}, Data: json.RawMessage(data)}
	s.cache.Set(key, config.TTL(30), result)
	return result, nil
}

func validPlace(place Place) bool {
	if len([]rune(place.ID)) > 200 || len([]rune(place.Name)) > 200 {
		return false
	}
	if strings.TrimSpace(place.ID) != "" {
		return true
	}
	return finiteCoordinate(place.Lat, -90, 90) && finiteCoordinate(place.Lon, -180, 180)
}

func finiteCoordinate(value, min, max float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0) && value >= min && value <= max
}

func placeInput(place Place) map[string]any {
	input := map[string]any{}
	if strings.TrimSpace(place.ID) != "" {
		input["place"] = strings.TrimSpace(place.ID)
	} else {
		input["coordinates"] = map[string]float64{"latitude": place.Lat, "longitude": place.Lon}
	}
	if strings.TrimSpace(place.Name) != "" {
		input["name"] = strings.TrimSpace(place.Name)
	}
	return input
}
