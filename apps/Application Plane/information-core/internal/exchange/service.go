package exchange

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
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

const defaultBaseURL = "https://data.norges-bank.no/api/data"

var seriesPattern = regexp.MustCompile(`^[A-Za-z0-9,._/-]{3,160}$`)
var ErrInvalidRequest = errors.New("exchange: invalid request")

type Service struct {
	client  *http.Client
	cache   *cache.Store
	baseURL string
}

type Request struct {
	Series            string `json:"series"`
	StartPeriod       string `json:"startPeriod,omitempty"`
	EndPeriod         string `json:"endPeriod,omitempty"`
	LastNObservations int    `json:"lastNObservations,omitempty"`
}

type Response struct {
	Source    provenance.Source `json:"source"`
	Series    string            `json:"series"`
	QueryHash string            `json:"queryHash"`
	Data      json.RawMessage   `json:"data"`
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore, baseURL: defaultBaseURL}
}

func NewServiceWithURL(client *http.Client, cacheStore *cache.Store, baseURL string) *Service {
	service := NewService(client, cacheStore)
	service.baseURL = strings.TrimRight(baseURL, "/")
	return service
}

func (s *Service) Series(ctx context.Context, input Request) (Response, error) {
	input.Series = strings.TrimSpace(input.Series)
	seriesPath, validPath := escapedSeriesPath(input.Series)
	if !seriesPattern.MatchString(input.Series) || !validPath || len(input.StartPeriod) > 10 || len(input.EndPeriod) > 10 {
		return Response{}, fmt.Errorf("%w: series and periods are invalid", ErrInvalidRequest)
	}
	if input.LastNObservations < 0 || input.LastNObservations > 100 {
		return Response{}, fmt.Errorf("%w: lastNObservations must be between 0 and 100", ErrInvalidRequest)
	}
	query := url.Values{}
	query.Set("format", "sdmx-json")
	if input.StartPeriod != "" {
		query.Set("startPeriod", input.StartPeriod)
	}
	if input.EndPeriod != "" {
		query.Set("endPeriod", input.EndPeriod)
	}
	if input.LastNObservations > 0 {
		query.Set("lastNObservations", fmt.Sprint(input.LastNObservations))
	}
	canonical := input.Series + "?" + query.Encode()
	hash := sha256.Sum256([]byte(canonical))
	queryHash := hex.EncodeToString(hash[:])
	key := "norges-bank:" + queryHash
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.baseURL+"/"+seriesPath+"?"+query.Encode(), nil)
	if err != nil {
		return Response{}, fmt.Errorf("exchange: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("exchange: upstream: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return Response{}, fmt.Errorf("exchange: read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("exchange: upstream returned HTTP %d", resp.StatusCode)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("exchange: upstream returned invalid JSON")
	}
	result := Response{Source: provenance.Source{Provider: "norges-bank", Dataset: "sdmx-open-data", SourceURL: defaultBaseURL, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: "selected-economic-series", Status: "measured", APIVersion: "sdmx-rest"}, Series: input.Series, QueryHash: queryHash, Data: json.RawMessage(data)}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}

func escapedSeriesPath(series string) (string, bool) {
	parts := strings.Split(series, "/")
	if len(parts) < 2 {
		return "", false
	}
	for index, part := range parts {
		if part == "" {
			return "", false
		}
		parts[index] = url.PathEscape(part)
	}
	return strings.Join(parts, "/"), true
}
