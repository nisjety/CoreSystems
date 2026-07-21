// Package officialseries collects bounded official statistics for a later
// Data Plane handoff. It does not write directly to Data Plane storage.
package officialseries

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
)

const (
	defaultSSBURL        = "https://data.ssb.no/api/pxwebapi/v2"
	defaultNorgesBankURL = "https://data.norges-bank.no"
	maxQueryBytes        = 16 << 10
	defaultMaxResponse   = 4 << 20
)

var (
	tableIDPattern = regexp.MustCompile(`^[0-9]{5}$`)
	seriesPattern  = regexp.MustCompile(`^[A-Za-z0-9,._/-]{3,160}$`)
	periodPattern  = regexp.MustCompile(`^[0-9]{4}(-[0-9]{2})?(-[0-9]{2})?$`)
)

type SSBSelection struct {
	VariableCode string   `json:"variableCode"`
	ValueCodes   []string `json:"valueCodes"`
}

type SSBQuery struct {
	TableID   string         `json:"table"`
	Selection []SSBSelection `json:"selection"`
}

type NorgesBankRequest struct {
	Series            string
	StartPeriod       string
	EndPeriod         string
	LastNObservations int
}

type Snapshot struct {
	Provider    string          `json:"provider"`
	Dataset     string          `json:"dataset"`
	SourceURL   string          `json:"source_url"`
	License     string          `json:"license"`
	QueryHash   string          `json:"query_hash"`
	RetrievedAt time.Time       `json:"retrieved_at"`
	Payload     json.RawMessage `json:"payload"`
}

type Client struct {
	HTTPClient        *http.Client
	SSBBaseURL        string
	NorgesBankBaseURL string
	MaxResponseBytes  int64
}

func NewClient(httpClient *http.Client, ssbBaseURL, norgesBankBaseURL string) *Client {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	if strings.TrimSpace(ssbBaseURL) == "" {
		ssbBaseURL = defaultSSBURL
	}
	if strings.TrimSpace(norgesBankBaseURL) == "" {
		norgesBankBaseURL = defaultNorgesBankURL
	}
	return &Client{
		HTTPClient:        httpClient,
		SSBBaseURL:        strings.TrimRight(ssbBaseURL, "/"),
		NorgesBankBaseURL: strings.TrimRight(norgesBankBaseURL, "/"),
		MaxResponseBytes:  defaultMaxResponse,
	}
}

func (c *Client) FetchSSB(ctx context.Context, query SSBQuery) (Snapshot, error) {
	query.TableID = strings.TrimSpace(query.TableID)
	if !tableIDPattern.MatchString(query.TableID) || len(query.Selection) == 0 || len(query.Selection) > 20 {
		return Snapshot{}, errors.New("officialseries: SSB table and selection bounds are invalid")
	}
	for _, selection := range query.Selection {
		if strings.TrimSpace(selection.VariableCode) == "" || len(selection.ValueCodes) == 0 || len(selection.ValueCodes) > 100 {
			return Snapshot{}, errors.New("officialseries: SSB variable selection bounds are invalid")
		}
		for _, value := range selection.ValueCodes {
			if strings.TrimSpace(value) == "" || len([]rune(value)) > 100 {
				return Snapshot{}, errors.New("officialseries: SSB value-code bounds are invalid")
			}
		}
	}

	body, err := json.Marshal(query)
	if err != nil {
		return Snapshot{}, fmt.Errorf("officialseries: encode SSB query: %w", err)
	}
	if len(body) > maxQueryBytes {
		return Snapshot{}, fmt.Errorf("officialseries: SSB query exceeds %d bytes", maxQueryBytes)
	}
	hash := sha256.Sum256(body)
	queryHash := hex.EncodeToString(hash[:])
	endpoint := fmt.Sprintf("%s/tables/%s/data?lang=en&outputFormat=json-stat2", c.SSBBaseURL, url.PathEscape(query.TableID))
	return c.fetchJSON(ctx, http.MethodPost, endpoint, bytes.NewReader(body), "ssb", "pxwebapi-v2", c.SSBBaseURL, "CC-BY-4.0", queryHash)
}

func (c *Client) FetchNorgesBank(ctx context.Context, request NorgesBankRequest) (Snapshot, error) {
	request.Series = strings.TrimSpace(request.Series)
	request.StartPeriod = strings.TrimSpace(request.StartPeriod)
	request.EndPeriod = strings.TrimSpace(request.EndPeriod)
	if !seriesPattern.MatchString(request.Series) || request.LastNObservations < 0 || request.LastNObservations > 100 {
		return Snapshot{}, errors.New("officialseries: Norges Bank series bounds are invalid")
	}
	if (request.StartPeriod == "") != (request.EndPeriod == "") {
		return Snapshot{}, errors.New("officialseries: Norges Bank start and end periods must be supplied together")
	}
	if request.StartPeriod == "" && request.LastNObservations == 0 {
		return Snapshot{}, errors.New("officialseries: Norges Bank request must have a bounded period or lastNObservations")
	}
	if request.StartPeriod != "" {
		start, err := parsePeriod(request.StartPeriod)
		if err != nil {
			return Snapshot{}, fmt.Errorf("officialseries: invalid Norges Bank start period: %w", err)
		}
		end, err := parsePeriod(request.EndPeriod)
		if err != nil {
			return Snapshot{}, fmt.Errorf("officialseries: invalid Norges Bank end period: %w", err)
		}
		if start.After(end) {
			return Snapshot{}, errors.New("officialseries: Norges Bank period is inverted")
		}
	}

	query := url.Values{"format": []string{"sdmx-json"}}
	if request.StartPeriod != "" {
		query.Set("startPeriod", request.StartPeriod)
		query.Set("endPeriod", request.EndPeriod)
	}
	if request.LastNObservations > 0 {
		query.Set("lastNObservations", fmt.Sprint(request.LastNObservations))
	}
	canonical := request.Series + "?" + query.Encode()
	hash := sha256.Sum256([]byte(canonical))
	queryHash := hex.EncodeToString(hash[:])
	endpoint := c.NorgesBankBaseURL + "/api/data/" + url.PathEscape(request.Series) + "?" + query.Encode()
	return c.fetchJSON(ctx, http.MethodGet, endpoint, nil, "norges-bank", "sdmx-open-data", c.NorgesBankBaseURL+"/api/data", "NLOD-2.0", queryHash)
}

func (c *Client) fetchJSON(ctx context.Context, method, endpoint string, body io.Reader, provider, dataset, sourceURL, license, queryHash string) (Snapshot, error) {
	req, err := http.NewRequestWithContext(ctx, method, endpoint, body)
	if err != nil {
		return Snapshot{}, fmt.Errorf("officialseries: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	if method == http.MethodPost {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return Snapshot{}, fmt.Errorf("officialseries: upstream %s: %w", provider, err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Snapshot{}, fmt.Errorf("officialseries: %s returned HTTP %d", provider, resp.StatusCode)
	}

	maxBytes := c.MaxResponseBytes
	if maxBytes <= 0 {
		maxBytes = defaultMaxResponse
	}
	payload, err := io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))
	if err != nil {
		return Snapshot{}, fmt.Errorf("officialseries: read %s response: %w", provider, err)
	}
	if int64(len(payload)) > maxBytes {
		return Snapshot{}, fmt.Errorf("officialseries: %s response exceeds %d bytes", provider, maxBytes)
	}
	if !json.Valid(payload) {
		return Snapshot{}, fmt.Errorf("officialseries: %s returned invalid JSON", provider)
	}
	return Snapshot{
		Provider:    provider,
		Dataset:     dataset,
		SourceURL:   sourceURL,
		License:     license,
		QueryHash:   queryHash,
		RetrievedAt: time.Now().UTC(),
		Payload:     append(json.RawMessage(nil), payload...),
	}, nil
}

func parsePeriod(value string) (time.Time, error) {
	if !periodPattern.MatchString(value) {
		return time.Time{}, errors.New("must be YYYY, YYYY-MM, or YYYY-MM-DD")
	}
	format := "2006"
	switch len(value) {
	case 7:
		format = "2006-01"
	case 10:
		format = "2006-01-02"
	}
	parsed, err := time.Parse(format, value)
	if err != nil {
		return time.Time{}, err
	}
	return parsed, nil
}
