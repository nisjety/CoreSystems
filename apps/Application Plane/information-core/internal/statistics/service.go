package statistics

import (
	"bytes"
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

const defaultBaseURL = "https://data.ssb.no/api/pxwebapi/v2"

var (
	ErrInvalidQuery = errors.New("statistics: invalid query")
	tableIDPattern  = regexp.MustCompile(`^[0-9]{5}$`)
)

type Service struct {
	client  *http.Client
	cache   *cache.Store
	baseURL string
}

type Selection struct {
	VariableCode string   `json:"variableCode"`
	ValueCodes   []string `json:"valueCodes"`
}

type QueryRequest struct {
	TableID   string      `json:"table"`
	Selection []Selection `json:"selection"`
}

type Response struct {
	Source    provenance.Source `json:"source"`
	TableID   string            `json:"tableId"`
	QueryHash string            `json:"queryHash,omitempty"`
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

func (s *Service) Query(ctx context.Context, query QueryRequest) (Response, error) {
	query.TableID = strings.TrimSpace(query.TableID)
	if !tableIDPattern.MatchString(query.TableID) || len(query.Selection) == 0 || len(query.Selection) > 20 {
		return Response{}, fmt.Errorf("%w: table must be a five-digit id and selection must contain 1-20 variables", ErrInvalidQuery)
	}
	for _, selection := range query.Selection {
		if strings.TrimSpace(selection.VariableCode) == "" || len(selection.ValueCodes) == 0 || len(selection.ValueCodes) > 100 {
			return Response{}, fmt.Errorf("%w: each variable requires 1-100 value codes", ErrInvalidQuery)
		}
		for _, value := range selection.ValueCodes {
			if strings.TrimSpace(value) == "" || len([]rune(value)) > 100 {
				return Response{}, fmt.Errorf("%w: value code is empty or too long", ErrInvalidQuery)
			}
		}
	}
	body, err := json.Marshal(query)
	if err != nil || len(body) > 16<<10 {
		return Response{}, fmt.Errorf("%w: selection body exceeds 16 KiB", ErrInvalidQuery)
	}
	hash := sha256.Sum256(body)
	queryHash := hex.EncodeToString(hash[:])
	key := "ssb:data:" + queryHash
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}

	endpoint := fmt.Sprintf("%s/tables/%s/data?lang=en&outputFormat=json-stat2", s.baseURL, url.PathEscape(query.TableID))
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint, bytes.NewReader(body))
	if err != nil {
		return Response{}, fmt.Errorf("statistics: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("statistics: upstream: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return Response{}, fmt.Errorf("statistics: read response: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("statistics: upstream returned HTTP %d", resp.StatusCode)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("statistics: upstream returned invalid JSON")
	}

	result := Response{Source: sourceEnvelope(time.Now().UTC().Format(time.RFC3339)), TableID: query.TableID, QueryHash: queryHash, Data: json.RawMessage(data)}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}

func (s *Service) Metadata(ctx context.Context, tableID string) (Response, error) {
	tableID = strings.TrimSpace(tableID)
	if !tableIDPattern.MatchString(tableID) {
		return Response{}, fmt.Errorf("%w: table must be a five-digit id", ErrInvalidQuery)
	}
	key := "ssb:metadata:" + tableID
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}
	endpoint := fmt.Sprintf("%s/tables/%s/metadata?lang=en", s.baseURL, url.PathEscape(tableID))
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return Response{}, fmt.Errorf("statistics: build metadata request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("statistics: metadata upstream: %w", err)
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(io.LimitReader(resp.Body, 4<<20))
	if err != nil {
		return Response{}, fmt.Errorf("statistics: read metadata: %w", err)
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("statistics: metadata upstream returned HTTP %d", resp.StatusCode)
	}
	if !json.Valid(data) {
		return Response{}, errors.New("statistics: metadata upstream returned invalid JSON")
	}
	result := Response{Source: sourceEnvelope(time.Now().UTC().Format(time.RFC3339)), TableID: tableID, Data: json.RawMessage(data)}
	s.cache.Set(key, config.TTL(3600), result)
	return result, nil
}

func sourceEnvelope(retrievedAt string) provenance.Source {
	return provenance.Source{Provider: "ssb", Dataset: "pxwebapi-v2", SourceURL: defaultBaseURL, License: "CC-BY-4.0", RetrievedAt: retrievedAt, Quality: "authoritative_provider", Coverage: "official_statistics", Status: "measured", APIVersion: "2"}
}
