package api

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/actions"
	"github.com/triodelab/quarry/internal/scraper"
	quarrysearch "github.com/triodelab/quarry/internal/search"
)

func TestSearchURLsLocalSiteSearch(t *testing.T) {
	t.Parallel()

	handler := &Handler{
		fetchFormatsFn: func(ctx context.Context, targetURL string, opts *scraper.FormatOptions) (map[string]interface{}, []actions.ActionResult, error) {
			if targetURL != "https://example.com" {
				t.Fatalf("targetURL = %q, want https://example.com", targetURL)
			}
			if len(opts.Formats) != 1 || opts.Formats[0] != "links" {
				t.Fatalf("Formats = %v, want [links]", opts.Formats)
			}
			return map[string]interface{}{
				"links": []string{
					"https://example.com/guides/machine-learning",
					"https://docs.example.com/machine-learning/overview",
					"https://other.example.net/machine-learning",
				},
			}, nil, nil
		},
	}

	app := fiber.New()
	app.Post("/v1/search", handler.searchURLs)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"site":              "example.com",
		"query":             "machine-learning",
		"includeSubdomains": true,
		"limit":             10,
	})

	var payload struct {
		Success bool           `json:"success"`
		Query   string         `json:"query"`
		Count   int            `json:"count"`
		Results []searchResult `json:"results"`
	}
	decodeJSONResponse(t, resp, &payload)

	if !payload.Success {
		t.Fatalf("success = false, want true")
	}
	if payload.Count != 2 {
		t.Fatalf("count = %d, want 2", payload.Count)
	}
	if len(payload.Results) != 2 {
		t.Fatalf("len(results) = %d, want 2", len(payload.Results))
	}
	for _, result := range payload.Results {
		if result.Source != "site" {
			t.Fatalf("source = %q, want site", result.Source)
		}
		if result.Type != "link" {
			t.Fatalf("type = %q, want link", result.Type)
		}
	}
}

func TestSearchURLsGlobalSearchPreservesRequestedSourceOrder(t *testing.T) {
	t.Parallel()

	searchClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			if r.URL.Query().Get("q") != "site:example.com quarry" {
				t.Fatalf("q = %q, want %q", r.URL.Query().Get("q"), "site:example.com quarry")
			}

			switch r.URL.Path {
			case "/res/v1/images/search":
				time.Sleep(25 * time.Millisecond)
				return jsonHTTPResponse(http.StatusOK, `{"images":{"results":[{"title":"Diagram","url":"https://cdn.example.com/diagram.png","description":"visual"}]}}`), nil
			case "/res/v1/web/search":
				time.Sleep(5 * time.Millisecond)
				return jsonHTTPResponse(http.StatusOK, `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"article"}]}}`), nil
			default:
				return jsonHTTPResponse(http.StatusNotFound, `{"error":"not found"}`), nil
			}
		}),
	}, time.Second)

	handler := &Handler{
		searchClient: searchClient,
	}

	app := fiber.New()
	app.Post("/v1/search", handler.searchURLs)

	resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", map[string]interface{}{
		"site":    "example.com",
		"query":   "quarry",
		"sources": []string{"images", "web"},
		"limit":   5,
	})

	var payload struct {
		Success bool           `json:"success"`
		Count   int            `json:"count"`
		Results []searchResult `json:"results"`
	}
	decodeJSONResponse(t, resp, &payload)

	if !payload.Success {
		t.Fatalf("success = false, want true")
	}
	if payload.Count != 2 {
		t.Fatalf("count = %d, want 2", payload.Count)
	}
	if len(payload.Results) != 2 {
		t.Fatalf("len(results) = %d, want 2", len(payload.Results))
	}
	if payload.Results[0].Type != "images" || payload.Results[1].Type != "web" {
		t.Fatalf("result order = [%s %s], want [images web]", payload.Results[0].Type, payload.Results[1].Type)
	}
}

func TestSearchURLsErrors(t *testing.T) {
	t.Parallel()

	rateLimitedClient := quarrysearch.NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusTooManyRequests, "too many requests"), nil
		}),
	}, time.Second)

	tests := []struct {
		name       string
		handler    *Handler
		body       map[string]interface{}
		wantStatus int
		wantError  string
	}{
		{
			name:       "unsupported source",
			handler:    &Handler{},
			body:       map[string]interface{}{"query": "quarry", "sources": []string{"video"}},
			wantStatus: http.StatusBadRequest,
			wantError:  "sources contains unsupported value",
		},
		{
			name:       "global search missing config",
			handler:    &Handler{},
			body:       map[string]interface{}{"query": "quarry", "sources": []string{"web"}},
			wantStatus: http.StatusServiceUnavailable,
			wantError:  "global search is not configured (BRAVE_SEARCH_API_KEY)",
		},
		{
			name:       "global search upstream rate limit",
			handler:    &Handler{searchClient: rateLimitedClient},
			body:       map[string]interface{}{"query": "quarry", "sources": []string{"web"}},
			wantStatus: http.StatusServiceUnavailable,
			wantError:  "global search failed",
		},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			app := fiber.New()
			app.Post("/v1/search", tt.handler.searchURLs)

			resp := performJSONRequest(t, app, http.MethodPost, "/v1/search", tt.body)
			if resp.StatusCode != tt.wantStatus {
				t.Fatalf("status = %d, want %d", resp.StatusCode, tt.wantStatus)
			}

			var payload ErrorEnvelope
			decodeJSONResponse(t, resp, &payload)
			if payload.Error != tt.wantError {
				t.Fatalf("error = %q, want %q", payload.Error, tt.wantError)
			}
		})
	}
}

func performJSONRequest(t *testing.T, app *fiber.App, method, path string, body interface{}) *http.Response {
	return performJSONRequestWithHeaders(t, app, method, path, body, nil)
}

func performJSONRequestWithHeaders(t *testing.T, app *fiber.App, method, path string, body interface{}, headers map[string]string) *http.Response {
	t.Helper()

	var reader io.Reader
	if body != nil {
		payload, err := json.Marshal(body)
		if err != nil {
			t.Fatalf("json.Marshal() error = %v", err)
		}
		reader = bytes.NewReader(payload)
	}

	req, err := http.NewRequest(method, path, reader)
	if err != nil {
		t.Fatalf("http.NewRequest() error = %v", err)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	for key, value := range headers {
		req.Header.Set(key, value)
	}

	resp, err := app.Test(req, int((2 * time.Second).Milliseconds()))
	if err != nil {
		t.Fatalf("app.Test() error = %v", err)
	}
	return resp
}

func decodeJSONResponse(t *testing.T, resp *http.Response, target interface{}) {
	t.Helper()
	defer resp.Body.Close()

	body, err := io.ReadAll(resp.Body)
	if err != nil {
		t.Fatalf("io.ReadAll() error = %v", err)
	}
	if err := json.Unmarshal(body, target); err != nil {
		t.Fatalf("json.Unmarshal() error = %v; body=%s", err, string(body))
	}
}

type roundTripFunc func(*http.Request) (*http.Response, error)

func (fn roundTripFunc) RoundTrip(r *http.Request) (*http.Response, error) {
	return fn(r)
}

func jsonHTTPResponse(status int, body string) *http.Response {
	return &http.Response{
		StatusCode: status,
		Header:     http.Header{"Content-Type": []string{"application/json"}},
		Body:       io.NopCloser(strings.NewReader(body)),
	}
}
