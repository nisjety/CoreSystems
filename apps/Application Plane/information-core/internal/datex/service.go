// Package datex provides the registered DATEX II pull boundary. The feed is
// kept as provider XML because the full DATEX schema is owned by the source;
// this service does not silently flatten or invent measurements.
package datex

import (
	"context"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const defaultDataset = "datex-ii-v3.1"

var ErrNotConfigured = errors.New("datex: registered credentials or endpoint are not configured")

type Service struct {
	client   *http.Client
	cache    *cache.Store
	endpoint string
	username string
	password string
}

type Response struct {
	Source      provenance.Source `json:"source"`
	ContentType string            `json:"contentType"`
	Data        string            `json:"data"`
}

func NewService(client *http.Client, endpoint, username, password string) *Service {
	return &Service{client: client, cache: cache.New(), endpoint: strings.TrimRight(strings.TrimSpace(endpoint), "/"), username: strings.TrimSpace(username), password: password}
}

func NewServiceWithCache(client *http.Client, cacheStore *cache.Store, endpoint, username, password string) *Service {
	service := NewService(client, endpoint, username, password)
	if cacheStore != nil {
		service.cache = cacheStore
	}
	return service
}

func (s *Service) PullSituation(ctx context.Context) (Response, error) {
	if s == nil || s.client == nil || s.endpoint == "" || s.username == "" || s.password == "" {
		return Response{}, ErrNotConfigured
	}
	key := "datex:situation:" + s.endpoint
	if cached, ok := s.cache.Get(key); ok {
		if response, ok := cached.(Response); ok {
			return response, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.endpoint, nil)
	if err != nil {
		return Response{}, fmt.Errorf("datex: build request: %w", err)
	}
	req.Header.Set("Accept", "application/xml")
	req.SetBasicAuth(s.username, s.password)
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("datex: upstream: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("datex: upstream returned HTTP %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, 8<<20))
	if err != nil {
		return Response{}, fmt.Errorf("datex: read response: %w", err)
	}
	if err := validateXML(body); err != nil {
		return Response{}, fmt.Errorf("datex: invalid XML: %w", err)
	}
	contentType := strings.TrimSpace(strings.Split(resp.Header.Get("Content-Type"), ";")[0])
	if contentType == "" {
		contentType = "application/xml"
	}
	result := Response{Source: provenance.Source{Provider: "statens-vegvesen", Dataset: defaultDataset, SourceURL: s.endpoint, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: "road_operational_situation", Status: "measured", APIVersion: "3.1"}, ContentType: contentType, Data: string(body)}
	s.cache.Set(key, config.TTL(30), result)
	return result, nil
}

func validateXML(data []byte) error {
	decoder := xml.NewDecoder(strings.NewReader(string(data)))
	for {
		_, err := decoder.Token()
		if errors.Is(err, io.EOF) {
			return nil
		}
		if err != nil {
			return err
		}
	}
}
