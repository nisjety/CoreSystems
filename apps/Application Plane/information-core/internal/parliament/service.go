package parliament

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const defaultURL = "https://data.stortinget.no/eksport/dagensrepresentanter"

type Service struct {
	client *http.Client
	cache  *cache.Store
	url    string
}

type Representative struct {
	ID         string        `json:"id"`
	FirstName  string        `json:"firstName"`
	LastName   string        `json:"lastName"`
	Party      Affiliation   `json:"party"`
	County     Affiliation   `json:"county"`
	Committees []Affiliation `json:"committees"`
	Alternate  bool          `json:"alternate"`
	Email      string        `json:"-"`
}

type Affiliation struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

type Response struct {
	Source  provenance.Source `json:"source"`
	Version string            `json:"version"`
	Data    []Representative  `json:"data"`
}

type rawResponse struct {
	Version string `json:"versjon"`
	Items   []struct {
		ID        string `json:"id"`
		FirstName string `json:"fornavn"`
		LastName  string `json:"etternavn"`
		Alternate bool   `json:"vara_representant"`
		Party     struct {
			ID   string `json:"id"`
			Name string `json:"navn"`
		} `json:"parti"`
		County struct {
			ID   string `json:"id"`
			Name string `json:"navn"`
		} `json:"fylke"`
		Committees []struct {
			ID   string `json:"id"`
			Name string `json:"navn"`
		} `json:"komiteer_liste"`
	} `json:"dagensrepresentanter_liste"`
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore, url: defaultURL}
}

func NewServiceWithURL(client *http.Client, cacheStore *cache.Store, endpoint string) *Service {
	service := NewService(client, cacheStore)
	service.url = strings.TrimRight(endpoint, "/")
	return service
}

func (s *Service) CurrentRepresentatives(ctx context.Context) (Response, error) {
	if cached, ok := s.cache.Get("stortinget:representatives"); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.url+"?format=JSON", nil)
	if err != nil {
		return Response{}, fmt.Errorf("parliament: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("parliament: upstream: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("parliament: upstream returned HTTP %d", resp.StatusCode)
	}
	var raw rawResponse
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return Response{}, fmt.Errorf("parliament: decode response: %w", err)
	}
	items := make([]Representative, 0, len(raw.Items))
	for _, item := range raw.Items {
		committees := make([]Affiliation, 0, len(item.Committees))
		for _, committee := range item.Committees {
			committees = append(committees, Affiliation{ID: committee.ID, Name: committee.Name})
		}
		items = append(items, Representative{ID: item.ID, FirstName: item.FirstName, LastName: item.LastName, Alternate: item.Alternate, Party: Affiliation{ID: item.Party.ID, Name: item.Party.Name}, County: Affiliation{ID: item.County.ID, Name: item.County.Name}, Committees: committees})
	}
	result := Response{Source: provenance.Source{Provider: "stortinget", Dataset: "current-representatives", SourceURL: defaultURL, License: "NLOD-2.0", RetrievedAt: time.Now().UTC().Format(time.RFC3339), Quality: "authoritative_provider", Coverage: "current-parliament", Status: "measured", APIVersion: "1.6"}, Version: raw.Version, Data: items}
	s.cache.Set("stortinget:representatives", config.TTL(3600), result)
	return result, nil
}
