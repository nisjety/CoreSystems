// Package revisions captures bounded revision snapshots from official legal
// and parliamentary sources. It does not interpret legal meaning or write
// directly to Data Plane storage.
package revisions

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"regexp"
	"sort"
	"strings"
	"time"
)

const (
	defaultLovdataURL  = "https://api.lovdata.no/v1/search"
	defaultStortingURL = "https://data.stortinget.no/eksport"
	defaultMaxResponse = 8 << 20
)

var (
	parameterPattern          = regexp.MustCompile(`^[A-Za-z][A-Za-z0-9_]*$`)
	approvedStortingResources = map[string]struct{}{
		"dagensrepresentanter": {},
		"dagsorden":            {},
		"enkeltsporsmal":       {},
		"moter":                {},
		"publikasjon":          {},
		"publikasjoner":        {},
		"representanter":       {},
		"sak":                  {},
		"saksganger":           {},
		"saker":                {},
		"sporsmal":             {},
		"vedtak":               {},
		"votering":             {},
		"voteringsforslag":     {},
		"voteringsresultat":    {},
		"voteringsvedtak":      {},
		"voteringer":           {},
	}
)

type LovdataSearchRequest struct {
	Terms  []string
	Base   string
	Limit  int
	Offset int
}

type StortingExportRequest struct {
	Resource string
	Params   map[string]string
	Format   string
}

type Snapshot struct {
	Provider    string    `json:"provider"`
	Dataset     string    `json:"dataset"`
	ResourceID  string    `json:"resource_id,omitempty"`
	Version     string    `json:"version,omitempty"`
	ContentType string    `json:"content_type"`
	SourceURL   string    `json:"source_url"`
	License     string    `json:"license"`
	QueryHash   string    `json:"query_hash"`
	ContentHash string    `json:"content_hash"`
	RetrievedAt time.Time `json:"retrieved_at"`
	Payload     []byte    `json:"payload"`
}

type Client struct {
	HTTPClient       *http.Client
	lovdataURL       string
	stortingURL      string
	MaxResponseBytes int64
	apiKey           string
}

func NewClient(httpClient *http.Client, lovdataURL, stortingURL, apiKey string) *Client {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	if strings.TrimSpace(lovdataURL) == "" {
		lovdataURL = defaultLovdataURL
	}
	if strings.TrimSpace(stortingURL) == "" {
		stortingURL = defaultStortingURL
	}
	return &Client{
		HTTPClient:       httpClient,
		lovdataURL:       strings.TrimRight(lovdataURL, "/"),
		stortingURL:      strings.TrimRight(stortingURL, "/"),
		MaxResponseBytes: defaultMaxResponse,
		apiKey:           strings.TrimSpace(apiKey),
	}
}

func (c *Client) FetchLovdataSearch(ctx context.Context, input LovdataSearchRequest) (Snapshot, error) {
	if c.apiKey == "" {
		return Snapshot{}, errors.New("revisions: Lovdata API key is not configured")
	}
	if len(input.Terms) == 0 || len(input.Terms) > 3 || input.Limit < 0 || input.Limit > 20 || input.Offset < 0 {
		return Snapshot{}, errors.New("revisions: Lovdata search bounds are invalid")
	}
	query := url.Values{}
	for i, term := range input.Terms {
		term = strings.TrimSpace(term)
		if term == "" || len([]rune(term)) > 100 {
			return Snapshot{}, errors.New("revisions: Lovdata terms must be 1-100 characters")
		}
		query.Set(fmt.Sprintf("emne%d", i+1), term)
	}
	if input.Limit == 0 {
		input.Limit = 10
	}
	if input.Base != "" && (len(input.Base) > 10 || strings.ContainsAny(input.Base, "&=\r\n")) {
		return Snapshot{}, errors.New("revisions: Lovdata base is invalid")
	}
	query.Set("rows", fmt.Sprint(input.Limit))
	query.Set("offset", fmt.Sprint(input.Offset))
	if input.Base != "" {
		query.Set("base", input.Base)
	}
	canonical := query.Encode()
	queryHash := hashBytes([]byte(canonical))
	endpoint := c.lovdataURL + "?" + canonical
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return Snapshot{}, fmt.Errorf("revisions: build Lovdata request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("X-API-Key", c.apiKey)
	payload, contentType, err := c.read(ctx, req, true)
	if err != nil {
		return Snapshot{}, fmt.Errorf("revisions: Lovdata: %w", err)
	}
	return newSnapshot("lovdata", "current-laws-and-central-regulations", "", "", contentType, c.lovdataURL, "NLOD-2.0", queryHash, payload), nil
}

func (c *Client) FetchStortingExport(ctx context.Context, input StortingExportRequest) (Snapshot, error) {
	input.Resource = strings.TrimSpace(input.Resource)
	if _, ok := approvedStortingResources[input.Resource]; !ok {
		return Snapshot{}, errors.New("revisions: Storting resource is not approved")
	}
	format := strings.ToUpper(strings.TrimSpace(input.Format))
	if format == "" {
		format = "XML"
	}
	if format != "XML" && format != "JSON" {
		return Snapshot{}, errors.New("revisions: Storting format must be XML or JSON")
	}
	query := url.Values{"format": []string{format}}
	keys := make([]string, 0, len(input.Params))
	for key := range input.Params {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		value := strings.TrimSpace(input.Params[key])
		if !parameterPattern.MatchString(key) || value == "" || len([]rune(value)) > 200 || strings.ContainsAny(value, "\r\n") {
			return Snapshot{}, errors.New("revisions: Storting parameter is invalid")
		}
		query.Set(key, value)
	}
	canonical := input.Resource + "?" + query.Encode()
	queryHash := hashBytes([]byte(canonical))
	endpoint := c.stortingURL + "/" + input.Resource + "?" + query.Encode()
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return Snapshot{}, fmt.Errorf("revisions: build Storting request: %w", err)
	}
	if format == "JSON" {
		req.Header.Set("Accept", "application/json")
	} else {
		req.Header.Set("Accept", "application/xml")
	}
	payload, contentType, err := c.read(ctx, req, false)
	if err != nil {
		return Snapshot{}, fmt.Errorf("revisions: Storting: %w", err)
	}
	resourceID := firstResourceID(input.Params)
	version := extractVersion(payload, format)
	return newSnapshot("stortinget", "open-data-export", resourceID, version, contentType, c.stortingURL+"/"+input.Resource, "NLOD-2.0", queryHash, payload), nil
}

func (c *Client) read(_ context.Context, req *http.Request, requireJSON bool) ([]byte, string, error) {
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return nil, "", err
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return nil, "", fmt.Errorf("upstream returned HTTP %d", resp.StatusCode)
	}
	maxBytes := c.MaxResponseBytes
	if maxBytes <= 0 {
		maxBytes = defaultMaxResponse
	}
	payload, err := io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))
	if err != nil {
		return nil, "", fmt.Errorf("read response: %w", err)
	}
	if int64(len(payload)) > maxBytes {
		return nil, "", fmt.Errorf("response exceeds %d bytes", maxBytes)
	}
	if requireJSON && !json.Valid(payload) {
		return nil, "", errors.New("response is not valid JSON")
	}
	contentType := strings.TrimSpace(strings.Split(resp.Header.Get("Content-Type"), ";")[0])
	if contentType == "" {
		if requireJSON {
			contentType = "application/json"
		} else {
			contentType = http.DetectContentType(payload)
		}
	}
	return append([]byte(nil), payload...), contentType, nil
}

func newSnapshot(provider, dataset, resourceID, version, contentType, sourceURL, license, queryHash string, payload []byte) Snapshot {
	return Snapshot{
		Provider:    provider,
		Dataset:     dataset,
		ResourceID:  resourceID,
		Version:     version,
		ContentType: contentType,
		SourceURL:   sourceURL,
		License:     license,
		QueryHash:   queryHash,
		ContentHash: hashBytes(payload),
		RetrievedAt: time.Now().UTC(),
		Payload:     append([]byte(nil), payload...),
	}
}

func firstResourceID(params map[string]string) string {
	for _, key := range []string{"sakid", "moteid", "publikasjonid", "voteringid", "personid"} {
		if value := strings.TrimSpace(params[key]); value != "" {
			return value
		}
	}
	return ""
}

func extractVersion(payload []byte, format string) string {
	if format == "JSON" {
		var value struct {
			Version string `json:"versjon"`
		}
		if json.Unmarshal(payload, &value) == nil {
			return strings.TrimSpace(value.Version)
		}
		return ""
	}
	decoder := xml.NewDecoder(strings.NewReader(string(payload)))
	for {
		token, err := decoder.Token()
		if err != nil {
			return ""
		}
		start, ok := token.(xml.StartElement)
		if !ok || start.Name.Local != "versjon" {
			continue
		}
		var version string
		if decoder.DecodeElement(&version, &start) == nil {
			return strings.TrimSpace(version)
		}
		return ""
	}
}

func hashBytes(payload []byte) string {
	hash := sha256.Sum256(payload)
	return hex.EncodeToString(hash[:])
}
