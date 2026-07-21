// Package procurement contains bounded procurement-source collectors. TED is
// a live public search boundary; Doffin is treated as a versioned CSV batch
// source because the catalog does not publish a read API.
package procurement

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/csv"
	"encoding/hex"
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
	defaultTEDURL     = "https://api.ted.europa.eu/v3/notices/search"
	defaultDoffinURL  = "https://adaapnedataprodst.blob.core.windows.net/kunngjoringer"
	maxTEDResponse    = 8 << 20
	maxDoffinResponse = 32 << 20
	maxCSVRows        = 250000
)

type Client struct {
	HTTPClient    *http.Client
	tedURL        string
	doffinBaseURL string
}

type TEDSearchRequest struct {
	Query              string   `json:"query"`
	Fields             []string `json:"fields,omitempty"`
	Page               int      `json:"page,omitempty"`
	Limit              int      `json:"limit,omitempty"`
	Scope              string   `json:"scope,omitempty"`
	CheckQuerySyntax   bool     `json:"checkQuerySyntax,omitempty"`
	PaginationMode     string   `json:"paginationMode,omitempty"`
	IterationNextToken string   `json:"iterationNextToken,omitempty"`
}

type TEDSnapshot struct {
	Provider    string    `json:"provider"`
	Dataset     string    `json:"dataset"`
	SourceURL   string    `json:"source_url"`
	QueryHash   string    `json:"query_hash"`
	ContentHash string    `json:"content_hash"`
	RetrievedAt time.Time `json:"retrieved_at"`
	Payload     []byte    `json:"payload"`
}

type DoffinNotice struct {
	ID     string            `json:"id"`
	Fields map[string]string `json:"fields"`
	Hash   string            `json:"hash"`
}

type DoffinSnapshot struct {
	Provider    string         `json:"provider"`
	Dataset     string         `json:"dataset"`
	Year        int            `json:"year"`
	SourceURL   string         `json:"source_url"`
	ContentHash string         `json:"content_hash"`
	RetrievedAt time.Time      `json:"retrieved_at"`
	Complete    bool           `json:"complete"`
	Notices     []DoffinNotice `json:"notices"`
}

type DoffinDiff struct {
	Added   []DoffinNotice `json:"added"`
	Updated []DoffinNotice `json:"updated"`
	Removed []string       `json:"removed"`
}

func NewClient(httpClient *http.Client, tedURL, doffinBaseURL string) *Client {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	if strings.TrimSpace(tedURL) == "" {
		tedURL = defaultTEDURL
	}
	if strings.TrimSpace(doffinBaseURL) == "" {
		doffinBaseURL = defaultDoffinURL
	}
	return &Client{HTTPClient: httpClient, tedURL: strings.TrimRight(tedURL, "/"), doffinBaseURL: strings.TrimRight(doffinBaseURL, "/")}
}

func (c *Client) SearchTED(ctx context.Context, input TEDSearchRequest) (TEDSnapshot, error) {
	input.Query = strings.TrimSpace(input.Query)
	if input.Query == "" || len([]rune(input.Query)) > 2000 || input.Page < 0 {
		return TEDSnapshot{}, errors.New("procurement: TED query or page is invalid")
	}
	if input.Limit == 0 {
		input.Limit = 100
	}
	if input.Limit < 1 || input.Limit > 250 {
		return TEDSnapshot{}, errors.New("procurement: TED limit must be between 1 and 250")
	}
	if input.PaginationMode == "" {
		input.PaginationMode = "PAGE_NUMBER"
	}
	if input.PaginationMode != "PAGE_NUMBER" && input.PaginationMode != "ITERATION" {
		return TEDSnapshot{}, errors.New("procurement: TED pagination mode is invalid")
	}
	if input.PaginationMode == "PAGE_NUMBER" && input.Page*input.Limit >= 15000 {
		return TEDSnapshot{}, errors.New("procurement: TED page exceeds the 15000-notice pagination bound")
	}
	if len(input.Fields) > 100 {
		return TEDSnapshot{}, errors.New("procurement: too many TED fields")
	}
	for _, field := range input.Fields {
		if strings.TrimSpace(field) == "" || len([]rune(field)) > 120 {
			return TEDSnapshot{}, errors.New("procurement: TED field is invalid")
		}
	}
	if len([]rune(input.IterationNextToken)) > 2048 {
		return TEDSnapshot{}, errors.New("procurement: TED iteration token is too long")
	}
	body, err := json.Marshal(input)
	if err != nil {
		return TEDSnapshot{}, fmt.Errorf("procurement: encode TED request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.tedURL, bytes.NewReader(body))
	if err != nil {
		return TEDSnapshot{}, fmt.Errorf("procurement: build TED request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Content-Type", "application/json")
	payload, err := c.readJSON(req, maxTEDResponse)
	if err != nil {
		return TEDSnapshot{}, fmt.Errorf("procurement: TED: %w", err)
	}
	return TEDSnapshot{Provider: "ted", Dataset: "published-procurement-notices", SourceURL: c.tedURL, QueryHash: hashBytes(body), ContentHash: hashBytes(payload), RetrievedAt: time.Now().UTC(), Payload: append([]byte(nil), payload...)}, nil
}

func (c *Client) FetchDoffinYear(ctx context.Context, year int) (DoffinSnapshot, error) {
	if year < 2000 || year > time.Now().UTC().Year()+1 {
		return DoffinSnapshot{}, errors.New("procurement: Doffin year is outside bounds")
	}
	sourceURL := fmt.Sprintf("%s/%d/Kunngjoringer_%d.csv", c.doffinBaseURL, year, year)
	if parsed, err := url.Parse(sourceURL); err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return DoffinSnapshot{}, errors.New("procurement: Doffin source URL is invalid")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, sourceURL, nil)
	if err != nil {
		return DoffinSnapshot{}, fmt.Errorf("procurement: build Doffin request: %w", err)
	}
	req.Header.Set("Accept", "text/csv")
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return DoffinSnapshot{}, fmt.Errorf("procurement: Doffin upstream: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return DoffinSnapshot{}, fmt.Errorf("procurement: Doffin returned HTTP %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxDoffinResponse+1))
	if err != nil {
		return DoffinSnapshot{}, fmt.Errorf("procurement: read Doffin response: %w", err)
	}
	if int64(len(body)) > maxDoffinResponse {
		return DoffinSnapshot{}, fmt.Errorf("procurement: Doffin response exceeds %d bytes", maxDoffinResponse)
	}
	return parseDoffinCSV(body, sourceURL, year)
}

func ParseDoffinCSV(data []byte, sourceURL string) (DoffinSnapshot, error) {
	return parseDoffinCSV(data, sourceURL, 0)
}

func DiffDoffin(previous, current DoffinSnapshot) DoffinDiff {
	previousByID := make(map[string]DoffinNotice, len(previous.Notices))
	for _, notice := range previous.Notices {
		previousByID[notice.ID] = notice
	}
	currentByID := make(map[string]DoffinNotice, len(current.Notices))
	diff := DoffinDiff{}
	for _, notice := range current.Notices {
		currentByID[notice.ID] = notice
		old, exists := previousByID[notice.ID]
		if !exists {
			diff.Added = append(diff.Added, notice)
		} else if old.Hash != notice.Hash {
			diff.Updated = append(diff.Updated, notice)
		}
	}
	if current.Complete {
		for id := range previousByID {
			if _, exists := currentByID[id]; !exists {
				diff.Removed = append(diff.Removed, id)
			}
		}
	}
	sort.Slice(diff.Added, func(i, j int) bool { return diff.Added[i].ID < diff.Added[j].ID })
	sort.Slice(diff.Updated, func(i, j int) bool { return diff.Updated[i].ID < diff.Updated[j].ID })
	sort.Strings(diff.Removed)
	return diff
}

func parseDoffinCSV(data []byte, sourceURL string, year int) (DoffinSnapshot, error) {
	data = bytes.TrimPrefix(data, []byte{0xEF, 0xBB, 0xBF})
	if len(data) == 0 {
		return DoffinSnapshot{}, errors.New("procurement: Doffin CSV is empty")
	}
	comma := ';'
	firstLine := string(data)
	if newline := strings.IndexByte(firstLine, '\n'); newline >= 0 {
		firstLine = firstLine[:newline]
	}
	if strings.Count(firstLine, ";") < strings.Count(firstLine, ",") {
		comma = ','
	}
	reader := csv.NewReader(bytes.NewReader(data))
	reader.Comma = comma
	reader.FieldsPerRecord = -1
	reader.TrimLeadingSpace = true
	header, err := reader.Read()
	if err != nil {
		return DoffinSnapshot{}, fmt.Errorf("procurement: read Doffin header: %w", err)
	}
	normalizedHeaders := make([]string, len(header))
	idIndex := -1
	for i, value := range header {
		normalizedHeaders[i] = normalizeHeader(value)
		if idIndex < 0 && isIDHeader(normalizedHeaders[i]) {
			idIndex = i
		}
	}
	if idIndex < 0 {
		return DoffinSnapshot{}, errors.New("procurement: Doffin CSV has no stable notice ID column")
	}
	notices := make([]DoffinNotice, 0)
	for rowNumber := 2; ; rowNumber++ {
		row, readErr := reader.Read()
		if errors.Is(readErr, io.EOF) {
			break
		}
		if readErr != nil {
			return DoffinSnapshot{}, fmt.Errorf("procurement: read Doffin row %d: %w", rowNumber, readErr)
		}
		if rowNumber > maxCSVRows {
			return DoffinSnapshot{}, fmt.Errorf("procurement: Doffin CSV exceeds %d rows", maxCSVRows)
		}
		fields := make(map[string]string, len(normalizedHeaders))
		for i, key := range normalizedHeaders {
			if key == "" {
				continue
			}
			value := ""
			if i < len(row) {
				value = strings.TrimSpace(row[i])
			}
			if len([]rune(value)) > 10000 {
				return DoffinSnapshot{}, fmt.Errorf("procurement: Doffin field in row %d is too long", rowNumber)
			}
			fields[key] = value
		}
		id := strings.TrimSpace(rowValue(row, idIndex))
		if id == "" {
			return DoffinSnapshot{}, fmt.Errorf("procurement: Doffin row %d has empty notice ID", rowNumber)
		}
		notices = append(notices, DoffinNotice{ID: id, Fields: fields, Hash: hashFields(fields)})
	}
	sort.Slice(notices, func(i, j int) bool { return notices[i].ID < notices[j].ID })
	return DoffinSnapshot{Provider: "doffin", Dataset: "public-procurement-notices-csv", Year: year, SourceURL: sourceURL, ContentHash: hashBytes(data), RetrievedAt: time.Now().UTC(), Complete: true, Notices: notices}, nil
}

func (c *Client) readJSON(req *http.Request, maxBytes int64) ([]byte, error) {
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return nil, fmt.Errorf("upstream returned HTTP %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))
	if err != nil {
		return nil, err
	}
	if int64(len(body)) > maxBytes {
		return nil, fmt.Errorf("response exceeds %d bytes", maxBytes)
	}
	if !json.Valid(body) {
		return nil, errors.New("response is not valid JSON")
	}
	return body, nil
}

func normalizeHeader(value string) string {
	value = strings.ToLower(strings.TrimSpace(strings.TrimPrefix(value, "\ufeff")))
	return strings.NewReplacer(" ", "", "_", "", "-", "", ".", "", "å", "a", "ø", "o", "æ", "ae").Replace(value)
}

func isIDHeader(value string) bool {
	switch value {
	case "id", "noticeid", "kunngjoringsid", "dofinnummer", "reference", "publicationnumber":
		return true
	default:
		return false
	}
}

func rowValue(row []string, index int) string {
	if index < 0 || index >= len(row) {
		return ""
	}
	return row[index]
}

func hashFields(fields map[string]string) string {
	keys := make([]string, 0, len(fields))
	for key := range fields {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	var builder strings.Builder
	for _, key := range keys {
		builder.WriteString(key)
		builder.WriteByte(0)
		builder.WriteString(fields[key])
		builder.WriteByte(0)
	}
	return hashBytes([]byte(builder.String()))
}

func hashBytes(data []byte) string {
	hash := sha256.Sum256(data)
	return hex.EncodeToString(hash[:])
}
