// Package catalogwatch contains the read-only Data.norge catalog boundary.
//
// It deliberately stops at a normalized snapshot and diff. Quarry scheduling,
// review policy, and Data Plane persistence remain separate responsibilities.
package catalogwatch

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"sort"
	"strings"
	"time"
)

const (
	DefaultSPARQLEndpoint = "https://sparql.fellesdatakatalog.digdir.no"
	defaultMaxResponse    = 8 << 20

	catalogQuery = `PREFIX dcat: <http://www.w3.org/ns/dcat#>
PREFIX dct: <http://purl.org/dc/terms/>

SELECT ?dataset ?title ?publisher ?modified ?access
WHERE {
  ?dataset a dcat:Dataset .
  OPTIONAL { ?dataset dct:title ?title . }
  OPTIONAL { ?dataset dct:publisher ?publisher . }
  OPTIONAL { ?dataset dct:modified ?modified . }
  OPTIONAL { ?dataset dcat:accessURL ?access . }
}`
)

type Resource struct {
	URI       string `json:"uri"`
	Title     string `json:"title,omitempty"`
	Publisher string `json:"publisher,omitempty"`
	Modified  string `json:"modified,omitempty"`
	AccessURL string `json:"access_url,omitempty"`
}

type Snapshot struct {
	RetrievedAt time.Time  `json:"retrieved_at"`
	Resources   []Resource `json:"resources"`
}

type Diff struct {
	Added   []Resource `json:"added"`
	Removed []Resource `json:"removed"`
	Changed []Resource `json:"changed"`
}

type Client struct {
	HTTPClient       *http.Client
	Endpoint         string
	MaxResponseBytes int64
}

func NewClient(httpClient *http.Client, endpoint string) *Client {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	if strings.TrimSpace(endpoint) == "" {
		endpoint = DefaultSPARQLEndpoint
	}
	return &Client{
		HTTPClient:       httpClient,
		Endpoint:         endpoint,
		MaxResponseBytes: defaultMaxResponse,
	}
}

func (c *Client) Snapshot(ctx context.Context) (Snapshot, error) {
	if c == nil {
		return Snapshot{}, errors.New("catalogwatch: nil client")
	}
	endpoint, err := url.Parse(c.Endpoint)
	if err != nil || endpoint.Scheme == "" || endpoint.Host == "" {
		return Snapshot{}, fmt.Errorf("catalogwatch: invalid SPARQL endpoint %q", c.Endpoint)
	}

	form := url.Values{"query": {catalogQuery}}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, endpoint.String(), strings.NewReader(form.Encode()))
	if err != nil {
		return Snapshot{}, fmt.Errorf("catalogwatch: create request: %w", err)
	}
	req.Header.Set("Accept", "application/sparql-results+json")
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return Snapshot{}, fmt.Errorf("catalogwatch: query catalog: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Snapshot{}, fmt.Errorf("catalogwatch: catalog returned HTTP %d", resp.StatusCode)
	}

	maxBytes := c.MaxResponseBytes
	if maxBytes <= 0 {
		maxBytes = defaultMaxResponse
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))
	if err != nil {
		return Snapshot{}, fmt.Errorf("catalogwatch: read response: %w", err)
	}
	if int64(len(body)) > maxBytes {
		return Snapshot{}, fmt.Errorf("catalogwatch: response exceeds %d bytes", maxBytes)
	}

	var response sparqlResponse
	if err := json.Unmarshal(body, &response); err != nil {
		return Snapshot{}, fmt.Errorf("catalogwatch: decode response: %w", err)
	}

	resources := make(map[string]Resource, len(response.Results.Bindings))
	for _, row := range response.Results.Bindings {
		uri := row["dataset"].Value
		if uri == "" {
			continue
		}
		resource := Resource{
			URI:       uri,
			Title:     row["title"].Value,
			Publisher: row["publisher"].Value,
			Modified:  row["modified"].Value,
			AccessURL: row["access"].Value,
		}
		if existing, ok := resources[uri]; ok {
			resource = mergeResource(existing, resource)
		}
		resources[uri] = resource
	}

	ordered := make([]Resource, 0, len(resources))
	for _, resource := range resources {
		ordered = append(ordered, resource)
	}
	sortResources(ordered)
	return Snapshot{RetrievedAt: time.Now().UTC(), Resources: ordered}, nil
}

func DiffSnapshots(previous, current Snapshot) Diff {
	previousByURI := indexResources(previous.Resources)
	currentByURI := indexResources(current.Resources)
	diff := Diff{}

	for uri, currentResource := range currentByURI {
		previousResource, exists := previousByURI[uri]
		if !exists {
			diff.Added = append(diff.Added, currentResource)
			continue
		}
		if previousResource != currentResource {
			diff.Changed = append(diff.Changed, currentResource)
		}
	}
	for uri, previousResource := range previousByURI {
		if _, exists := currentByURI[uri]; !exists {
			diff.Removed = append(diff.Removed, previousResource)
		}
	}

	sortResources(diff.Added)
	sortResources(diff.Removed)
	sortResources(diff.Changed)
	return diff
}

type sparqlResponse struct {
	Results struct {
		Bindings []map[string]binding `json:"bindings"`
	} `json:"results"`
}

type binding struct {
	Value string `json:"value"`
}

func indexResources(resources []Resource) map[string]Resource {
	indexed := make(map[string]Resource, len(resources))
	for _, resource := range resources {
		if resource.URI != "" {
			indexed[resource.URI] = resource
		}
	}
	return indexed
}

func mergeResource(existing, candidate Resource) Resource {
	if existing.Title == "" {
		existing.Title = candidate.Title
	}
	if existing.Publisher == "" {
		existing.Publisher = candidate.Publisher
	}
	if existing.Modified == "" {
		existing.Modified = candidate.Modified
	}
	if existing.AccessURL == "" {
		existing.AccessURL = candidate.AccessURL
	}
	return existing
}

func sortResources(resources []Resource) {
	sort.Slice(resources, func(i, j int) bool {
		return resources[i].URI < resources[j].URI
	})
}
