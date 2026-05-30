package search

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestBraveClientSearch(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		searchType SearchType
		limit      int
		site       string
		payload    string
		wantPath   string
		wantQuery  string
		wantCount  string
		want       Result
	}{
		{
			name:       "web results",
			searchType: SearchTypeWeb,
			limit:      50,
			site:       "example.com",
			payload:    `{"web":{"results":[{"title":"Guide","url":"https://example.com/guide","description":"structured data"}]}}`,
			wantPath:   "/res/v1/web/search",
			wantQuery:  "site:example.com quarry",
			wantCount:  "20",
			want: Result{
				Title:   "Guide",
				URL:     "https://example.com/guide",
				Snippet: "structured data",
				Source:  "brave",
				Type:    "web",
			},
		},
		{
			name:       "news results",
			searchType: SearchTypeNews,
			limit:      3,
			payload:    `{"news":{"results":[{"title":"Launch","url":"https://news.example.com/launch","description":"breaking"}]}}`,
			wantPath:   "/res/v1/news/search",
			wantQuery:  "quarry",
			wantCount:  "3",
			want: Result{
				Title:   "Launch",
				URL:     "https://news.example.com/launch",
				Snippet: "breaking",
				Source:  "brave",
				Type:    "news",
			},
		},
		{
			name:       "image results fall back to name",
			searchType: SearchTypeImages,
			limit:      1,
			payload:    `{"images":{"results":[{"name":"Quarry Diagram","source_url":"https://cdn.example.com/image.png","thumbnail":{"src":"https://cdn.example.com/thumb.png"},"description":"diagram"}]}}`,
			wantPath:   "/res/v1/images/search",
			wantQuery:  "quarry",
			wantCount:  "1",
			want: Result{
				Title:   "Quarry Diagram",
				URL:     "https://cdn.example.com/thumb.png",
				Snippet: "diagram",
				Source:  "brave",
				Type:    "images",
			},
		},
	}

	for _, tt := range tests {
		tt := tt
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			httpClient := &http.Client{Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
				if got := r.Header.Get("X-Subscription-Token"); got != "test-key" {
					t.Fatalf("X-Subscription-Token = %q, want test-key", got)
				}
				if got := r.URL.Path; got != tt.wantPath {
					t.Fatalf("path = %q, want %q", got, tt.wantPath)
				}
				if got := r.URL.Query().Get("q"); got != tt.wantQuery {
					t.Fatalf("q = %q, want %q", got, tt.wantQuery)
				}
				if got := r.URL.Query().Get("count"); got != tt.wantCount {
					t.Fatalf("count = %q, want %q", got, tt.wantCount)
				}
				return jsonHTTPResponse(http.StatusOK, tt.payload), nil
			})}

			client := NewBraveClientWithHTTPClient("test-key", "https://unit.test", httpClient, time.Second)
			results, err := client.Search(context.Background(), tt.searchType, SearchOptions{
				Query: "quarry",
				Limit: tt.limit,
				Site:  tt.site,
			})
			if err != nil {
				t.Fatalf("Search() error = %v", err)
			}
			if len(results) != 1 {
				t.Fatalf("len(results) = %d, want 1", len(results))
			}
			if results[0] != tt.want {
				t.Fatalf("result = %+v, want %+v", results[0], tt.want)
			}
		})
	}
}

func TestBraveClientSearchHTTPError(t *testing.T) {
	t.Parallel()

	client := NewBraveClientWithHTTPClient("test-key", "https://unit.test", &http.Client{
		Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
			return jsonHTTPResponse(http.StatusTooManyRequests, "rate limited"), nil
		}),
	}, time.Second)
	_, err := client.Search(context.Background(), SearchTypeWeb, SearchOptions{Query: "quarry"})
	if err == nil {
		t.Fatal("Search() error = nil, want APIError")
	}

	var apiErr *APIError
	if !errors.As(err, &apiErr) {
		t.Fatalf("error type = %T, want *APIError", err)
	}
	if apiErr.StatusCode != http.StatusTooManyRequests {
		t.Fatalf("StatusCode = %d, want %d", apiErr.StatusCode, http.StatusTooManyRequests)
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
