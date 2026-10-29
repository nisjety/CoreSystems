// Package frost provides a credential-gated MET Norway Frost observation
// lookup. Client IDs are required by the provider even for open observations.
package frost

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const defaultURL = "https://frost.met.no/observations/v0.jsonld"

var (
	ErrInvalidRequest = errors.New("frost: invalid request")
	ErrNotConfigured  = errors.New("frost: client ID is not configured")
	sourcePattern     = regexp.MustCompile(`^SN[0-9]+(?::(?:[0-9]+|all))?(?:,SN[0-9]+(?::(?:[0-9]+|all))?){0,19}$`)
)

type Service struct {
	clientID string
	client   *http.Client
	cache    *cache.Store
	url      string
}

type Request struct {
	Sources       string `json:"sources"`
	Elements      string `json:"elements"`
	ReferenceTime string `json:"referenceTime"`
	Limit         int    `json:"limit,omitempty"`
}

type Response struct {
	Source provenance.Source `json:"source"`
	Data   json.RawMessage   `json:"data"`
}

func NewService(client *http.Client, endpoint, clientID string) *Service {
	if strings.TrimSpace(endpoint) == "" {
		endpoint = defaultURL
	}
	return &Service{clientID: strings.TrimSpace(clientID), client: client, cache: cache.New(), url: strings.TrimRight(endpoint, "/")}
}

func NewServiceWithCache(client *http.Client, cacheStore *cache.Store, endpoint, clientID string) *Service {
	service := NewService(client, endpoint, clientID)
	if cacheStore != nil {
		service.cache = cacheStore
	}
	return service
}

func (s *Service) Observations(ctx context.Context, input Request) (Response, error) {
	if s == nil || s.client == nil || s.clientID == "" {
		return Response{}, ErrNotConfigured
	}
	input.Sources = strings.TrimSpace(input.Sources)
	input.Elements = strings.TrimSpace(input.Elements)
	input.ReferenceTime = strings.TrimSpace(input.ReferenceTime)
	if !sourcePattern.MatchString(input.Sources) || input.Elements == "" || len([]rune(input.Elements)) > 500 || input.ReferenceTime == "" || len([]rune(input.ReferenceTime)) > 100 || strings.ContainsAny(input.Elements+input.ReferenceTime, "\r\n") {
		return Response{}, fmt.Errorf("%w: sources, elements, or reference time are invalid", ErrInvalidRequest)
	}
	if input.Limit < 0 || input.Limit > 10000 {
		return Response{}, fmt.Errorf("%w: limit must be between 0 and 10000", ErrInvalidRequest)
	}
	query := url.Values{"sources": []string{input.Sources}, "elements": []string{input.Elements}, "referencetime": []string{input.ReferenceTime}}
	if input.Limit > 0 {
		query.Set("limit", fmt.Sprint(input.Limit))
	}
	canonical := query.Encode()
	key := "frost:" + canonical
	if cached, ok := s.cache.Get(key); ok {
		if response, ok := cached.(Response); ok {
			return response, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.url+"?"+canonical, nil)
	if err != nil {
		return Response{}, fmt.Errorf("frost: build request: %w", err)
	}
	req.SetBasicAuth(s.clientID, "")
	req.Header.Set("Accept", "application/ld+json")
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("frost: upstream: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("frost: upstream returned HTTP %d", resp.StatusCode)
	}
	data, err := io.ReadAll(io.LimitReader(resp.Body, 8<<20))
	if err != nil {
		return Response{}, fmt.Errorf("frost: read response: %w", err)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("frost: upstream returned invalid JSON")
	}
	result := Response{Source: provenance.Source{Provider: "met-norway", Dataset: "frost-observations", SourceURL: s.url, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: "selected-weather-observations", Status: "measured", APIVersion: "v0"}, Data: append(json.RawMessage(nil), data...)}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}
