// Package navacancies collects the authenticated NAV job-vacancy change feed.
// It intentionally keeps only public vacancy metadata and exposes explicit
// inactive tombstones so downstream storage can delete promptly.
package navacancies

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
	"sort"
	"strings"
	"time"
)

const defaultFeedURL = "https://pam-stilling-feed.nav.no/api/v1/feed"

type Client struct {
	HTTPClient       *http.Client
	BaseURL          string
	BearerToken      string
	MaxResponseBytes int64
}

type Page struct {
	Version      string   `json:"version,omitempty"`
	ID           string   `json:"id,omitempty"`
	NextURL      string   `json:"next_url,omitempty"`
	NextID       string   `json:"next_id,omitempty"`
	ETag         string   `json:"etag,omitempty"`
	LastModified string   `json:"last_modified,omitempty"`
	NotModified  bool     `json:"not_modified,omitempty"`
	Items        []Change `json:"items"`
}

type Change struct {
	ID           string `json:"id"`
	URL          string `json:"url,omitempty"`
	Title        string `json:"title,omitempty"`
	BusinessName string `json:"business_name,omitempty"`
	Municipal    string `json:"municipal,omitempty"`
	DateModified string `json:"date_modified,omitempty"`
	Status       string `json:"status"`
	ContentHash  string `json:"content_hash"`
}

type Vacancy struct {
	ID           string `json:"id"`
	URL          string `json:"url,omitempty"`
	Title        string `json:"title,omitempty"`
	BusinessName string `json:"business_name,omitempty"`
	Municipal    string `json:"municipal,omitempty"`
	DateModified string `json:"date_modified,omitempty"`
	Status       string `json:"status"`
	ContentHash  string `json:"content_hash"`
}

type State struct {
	Vacancies []Vacancy `json:"vacancies"`
}

type Diff struct {
	Upserted []Vacancy `json:"upserted"`
	Removed  []string  `json:"removed"`
}

type rawPage struct {
	Version string    `json:"version"`
	ID      string    `json:"id"`
	NextURL string    `json:"next_url"`
	NextID  string    `json:"next_id"`
	Items   []rawItem `json:"items"`
}

type rawItem struct {
	ID           string `json:"id"`
	URL          string `json:"url"`
	Title        string `json:"title"`
	DateModified string `json:"date_modified"`
	FeedEntry    struct {
		UUID         string `json:"uuid"`
		Status       string `json:"status"`
		Title        string `json:"title"`
		BusinessName string `json:"businessName"`
		Municipal    string `json:"municipal"`
		LastChanged  string `json:"sistEndret"`
	} `json:"_feed_entry"`
}

func NewClient(httpClient *http.Client, baseURL, bearerToken string) *Client {
	if httpClient == nil {
		httpClient = &http.Client{Timeout: 30 * time.Second}
	}
	if strings.TrimSpace(baseURL) == "" {
		baseURL = defaultFeedURL
	}
	return &Client{
		HTTPClient:       httpClient,
		BaseURL:          strings.TrimRight(baseURL, "/"),
		BearerToken:      strings.TrimSpace(bearerToken),
		MaxResponseBytes: 8 << 20,
	}
}

func (c *Client) FetchPage(ctx context.Context, nextURL string) (Page, error) {
	return c.FetchPageWithHeaders(ctx, nextURL, "", "")
}

func (c *Client) FetchPageWithHeaders(ctx context.Context, nextURL, etag, lastModified string) (Page, error) {
	if c == nil || strings.TrimSpace(c.BearerToken) == "" {
		return Page{}, errors.New("navacancies: bearer token is not configured")
	}
	endpoint, err := c.resolveURL(nextURL)
	if err != nil {
		return Page{}, err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return Page{}, fmt.Errorf("navacancies: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.BearerToken)
	if strings.TrimSpace(etag) != "" {
		req.Header.Set("If-None-Match", etag)
	}
	if strings.TrimSpace(lastModified) != "" {
		req.Header.Set("If-Modified-Since", lastModified)
	}
	resp, err := c.HTTPClient.Do(req)
	if err != nil {
		return Page{}, fmt.Errorf("navacancies: upstream: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode == http.StatusNotModified {
		return Page{ETag: resp.Header.Get("ETag"), LastModified: resp.Header.Get("Last-Modified"), NotModified: true}, nil
	}
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Page{}, fmt.Errorf("navacancies: upstream returned HTTP %d", resp.StatusCode)
	}
	maxBytes := c.MaxResponseBytes
	if maxBytes <= 0 {
		maxBytes = 8 << 20
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxBytes+1))
	if err != nil {
		return Page{}, fmt.Errorf("navacancies: read response: %w", err)
	}
	if int64(len(body)) > maxBytes {
		return Page{}, fmt.Errorf("navacancies: response exceeds %d bytes", maxBytes)
	}
	var raw rawPage
	if err := json.Unmarshal(body, &raw); err != nil {
		return Page{}, fmt.Errorf("navacancies: decode response: %w", err)
	}
	if raw.NextURL != "" {
		if _, err := c.resolveURL(raw.NextURL); err != nil {
			return Page{}, err
		}
	}
	items := make([]Change, 0, len(raw.Items))
	for _, item := range raw.Items {
		id := strings.TrimSpace(item.FeedEntry.UUID)
		if id == "" {
			id = strings.TrimSpace(item.ID)
		}
		status := strings.ToUpper(strings.TrimSpace(item.FeedEntry.Status))
		if id == "" || (status != "ACTIVE" && status != "INACTIVE") {
			return Page{}, errors.New("navacancies: feed item has invalid id or status")
		}
		modified := strings.TrimSpace(item.DateModified)
		if modified == "" {
			modified = strings.TrimSpace(item.FeedEntry.LastChanged)
		}
		change := Change{
			ID:           id,
			URL:          strings.TrimSpace(item.URL),
			Title:        firstNonEmpty(item.Title, item.FeedEntry.Title),
			BusinessName: strings.TrimSpace(item.FeedEntry.BusinessName),
			Municipal:    strings.TrimSpace(item.FeedEntry.Municipal),
			DateModified: modified,
			Status:       status,
		}
		change.ContentHash = contentHash(change)
		items = append(items, change)
	}
	return Page{Version: raw.Version, ID: raw.ID, NextURL: raw.NextURL, NextID: raw.NextID, ETag: resp.Header.Get("ETag"), LastModified: resp.Header.Get("Last-Modified"), Items: items}, nil
}

func Apply(state State, page Page) (State, Diff) {
	byID := make(map[string]Vacancy, len(state.Vacancies))
	for _, vacancy := range state.Vacancies {
		if vacancy.ID != "" && strings.EqualFold(vacancy.Status, "ACTIVE") {
			byID[vacancy.ID] = vacancy
		}
	}
	diff := Diff{}
	removed := make(map[string]struct{})
	for _, change := range page.Items {
		if change.Status == "INACTIVE" {
			delete(byID, change.ID)
			if _, ok := removed[change.ID]; !ok {
				diff.Removed = append(diff.Removed, change.ID)
				removed[change.ID] = struct{}{}
			}
			continue
		}
		vacancy := Vacancy{ID: change.ID, URL: change.URL, Title: change.Title, BusinessName: change.BusinessName, Municipal: change.Municipal, DateModified: change.DateModified, Status: "ACTIVE", ContentHash: change.ContentHash}
		if existing, ok := byID[change.ID]; !ok || existing != vacancy {
			byID[change.ID] = vacancy
			diff.Upserted = append(diff.Upserted, vacancy)
		}
	}
	next := State{Vacancies: make([]Vacancy, 0, len(byID))}
	for _, vacancy := range byID {
		next.Vacancies = append(next.Vacancies, vacancy)
	}
	sort.Slice(next.Vacancies, func(i, j int) bool { return next.Vacancies[i].ID < next.Vacancies[j].ID })
	sort.Slice(diff.Upserted, func(i, j int) bool { return diff.Upserted[i].ID < diff.Upserted[j].ID })
	sort.Strings(diff.Removed)
	return next, diff
}

func (c *Client) resolveURL(nextURL string) (string, error) {
	base, err := url.Parse(c.BaseURL)
	if err != nil || base.Scheme == "" || base.Host == "" {
		return "", fmt.Errorf("navacancies: invalid base URL %q", c.BaseURL)
	}
	if strings.TrimSpace(nextURL) == "" {
		return base.String(), nil
	}
	next, err := url.Parse(nextURL)
	if err != nil || next.User != nil {
		return "", errors.New("navacancies: invalid next URL")
	}
	resolved := base.ResolveReference(next)
	if resolved.Scheme != base.Scheme || resolved.Host != base.Host || resolved.User != nil {
		return "", errors.New("navacancies: next URL is outside the configured NAV host")
	}
	return resolved.String(), nil
}

func contentHash(change Change) string {
	canonical := strings.Join([]string{change.ID, change.URL, change.Title, change.BusinessName, change.Municipal, change.DateModified, change.Status}, "\x00")
	hash := sha256.Sum256([]byte(canonical))
	return hex.EncodeToString(hash[:])
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return strings.TrimSpace(value)
		}
	}
	return ""
}
