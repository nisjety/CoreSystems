package quarry_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/pkg/quarry"
)

func TestClient_Unavailable_WhenBaseURLEmpty(t *testing.T) {
	c := quarry.New(quarry.Config{})
	if c.Available() {
		t.Fatal("empty BaseURL must report Available() == false")
	}
	_, err := c.Scrape(context.Background(), quarry.ScrapeRequest{URL: "https://example.com"})
	if err != quarry.ErrUnavailable {
		t.Fatalf("expected ErrUnavailable, got %v", err)
	}
}

func TestClient_Scrape_HappyPath(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/scrape" {
			t.Errorf("unexpected path: %s", r.URL.Path)
		}
		if r.Header.Get("Authorization") != "Bearer secret" {
			t.Errorf("missing/wrong bearer token: %q", r.Header.Get("Authorization"))
		}
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body["url"] != "https://example.com" {
			t.Errorf("wrong url in body: %v", body["url"])
		}
		// Mimic Quarry envelope.
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{
			"data": {
				"status": 200,
				"content_type": "text/html",
				"fingerprint": "blake3:abc123",
				"url": {"final": "https://example.com/"},
				"formats": {"markdown": "# Example", "html": "<html></html>"},
				"metadata": {"title": "Example", "lang": "en"}
			}
		}`))
	}))
	defer srv.Close()

	c := quarry.New(quarry.Config{BaseURL: srv.URL, Token: "secret", Timeout: 5 * time.Second})
	res, err := c.Scrape(context.Background(), quarry.ScrapeRequest{URL: "https://example.com"})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if res.Status != 200 || res.Title != "Example" || res.Markdown != "# Example" {
		t.Fatalf("projection wrong: %+v", res)
	}
	if res.Language != "en" || res.Fingerprint != "blake3:abc123" {
		t.Fatalf("metadata wrong: %+v", res)
	}
	if res.FinalURL != "https://example.com/" {
		t.Fatalf("final_url wrong: %q", res.FinalURL)
	}
}

func TestClient_Scrape_RenderHints(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		render, ok := body["render"].(map[string]any)
		if !ok {
			t.Fatal("expected render block in body")
		}
		if render["waitForSelector"] != "#ready" {
			t.Errorf("wrong selector: %v", render["waitForSelector"])
		}
		if int(render["waitForTimeoutMs"].(float64)) != 7500 {
			t.Errorf("wrong timeout: %v", render["waitForTimeoutMs"])
		}
		_, _ = w.Write([]byte(`{"data":{"status":200,"formats":{"markdown":""}}}`))
	}))
	defer srv.Close()

	c := quarry.New(quarry.Config{BaseURL: srv.URL})
	_, err := c.Scrape(context.Background(), quarry.ScrapeRequest{
		URL:    "https://example.com",
		Render: &quarry.RenderHints{WaitForSelector: "#ready", WaitForTimeoutMS: 7500},
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestClient_Scrape_TypedError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"error":{"code":"SECURITY_BLOCKED","message":"private host"}}`))
	}))
	defer srv.Close()

	c := quarry.New(quarry.Config{BaseURL: srv.URL})
	_, err := c.Scrape(context.Background(), quarry.ScrapeRequest{URL: "http://10.0.0.1/"})
	qerr, ok := err.(*quarry.Error)
	if !ok {
		t.Fatalf("expected *quarry.Error, got %T: %v", err, err)
	}
	if qerr.Code != "SECURITY_BLOCKED" || qerr.StatusCode != 403 {
		t.Fatalf("wrong typed error: %+v", qerr)
	}
}

func TestClient_Scrape_PreferHTTP3FlagPropagates(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body["prefer_http3"] != true {
			t.Errorf("prefer_http3 not set in body: %v", body)
		}
		_, _ = w.Write([]byte(`{"data":{"status":200,"formats":{"markdown":""}}}`))
	}))
	defer srv.Close()

	c := quarry.New(quarry.Config{BaseURL: srv.URL})
	_, err := c.Scrape(context.Background(), quarry.ScrapeRequest{
		URL:         "https://example.com",
		PreferHTTP3: true,
	})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
}

func TestClient_Scrape_NonJSONErrorBody(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusBadGateway)
		_, _ = w.Write([]byte(`<html><body>upstream gateway timeout</body></html>`))
	}))
	defer srv.Close()

	c := quarry.New(quarry.Config{BaseURL: srv.URL})
	_, err := c.Scrape(context.Background(), quarry.ScrapeRequest{URL: "https://example.com"})
	qerr, ok := err.(*quarry.Error)
	if !ok {
		t.Fatalf("expected *quarry.Error, got %T", err)
	}
	if qerr.Code != "HTTP_502" {
		t.Fatalf("expected synthesized HTTP_502 code, got %q", qerr.Code)
	}
	if !strings.Contains(qerr.Message, "upstream") {
		t.Fatalf("error message did not include body excerpt: %q", qerr.Message)
	}
}
