package legal

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
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const defaultURL = "https://api.lovdata.no/v1/search"

var ErrInvalidRequest = errors.New("legal: invalid request")
var ErrNotConfigured = errors.New("legal: API key is not configured")

type Service struct {
	client *http.Client
	cache  *cache.Store
	url    string
	apiKey string
}

type Request struct {
	Terms  []string `json:"terms"`
	Base   string   `json:"base,omitempty"`
	Limit  int      `json:"limit,omitempty"`
	Offset int      `json:"offset,omitempty"`
}

type Response struct {
	Source    provenance.Source `json:"source"`
	QueryHash string            `json:"queryHash"`
	Data      json.RawMessage   `json:"data"`
}

func NewService(client *http.Client, cacheStore *cache.Store, apiKey string) *Service {
	return &Service{client: client, cache: cacheStore, url: defaultURL, apiKey: strings.TrimSpace(apiKey)}
}

func NewServiceWithURL(client *http.Client, cacheStore *cache.Store, endpoint, apiKey string) *Service {
	service := NewService(client, cacheStore, apiKey)
	service.url = endpoint
	return service
}

func (s *Service) Search(ctx context.Context, input Request) (Response, error) {
	if s.apiKey == "" {
		return Response{}, ErrNotConfigured
	}
	if len(input.Terms) == 0 || len(input.Terms) > 3 || input.Limit < 0 || input.Limit > 20 || input.Offset < 0 {
		return Response{}, fmt.Errorf("%w: use 1-3 terms, limit 0-20, non-negative offset", ErrInvalidRequest)
	}
	query := url.Values{}
	for i, term := range input.Terms {
		term = strings.TrimSpace(term)
		if term == "" || len([]rune(term)) > 100 {
			return Response{}, fmt.Errorf("%w: terms must be 1-100 characters", ErrInvalidRequest)
		}
		query.Set(fmt.Sprintf("emne%d", i+1), term)
	}
	if input.Limit == 0 {
		input.Limit = 10
	}
	query.Set("rows", fmt.Sprint(input.Limit))
	query.Set("offset", fmt.Sprint(input.Offset))
	if input.Base != "" {
		if len(input.Base) > 10 || strings.ContainsAny(input.Base, "&=\r\n") {
			return Response{}, fmt.Errorf("%w: invalid legal source", ErrInvalidRequest)
		}
		query.Set("base", input.Base)
	}
	canonical := query.Encode()
	hash := sha256.Sum256([]byte(canonical))
	queryHash := hex.EncodeToString(hash[:])
	key := "lovdata:" + queryHash
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.url+"?"+canonical, nil)
	if err != nil {
		return Response{}, fmt.Errorf("legal: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("X-API-Key", s.apiKey)
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("legal: upstream: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return Response{}, fmt.Errorf("legal: read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("legal: upstream returned HTTP %d", resp.StatusCode)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("legal: upstream returned invalid JSON")
	}
	result := Response{Source: provenance.Source{Provider: "lovdata", Dataset: "current-laws-and-central-regulations", SourceURL: defaultURL, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: "current_legal_sources", Status: "measured", APIVersion: "v1"}, QueryHash: queryHash, Data: json.RawMessage(data)}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}
