package navacancies

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestFetchPageAndApplyIsDeletionAware(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer nav-token" {
			t.Fatalf("authorization = %q", r.Header.Get("Authorization"))
		}
		w.Header().Set("ETag", `"page-1"`)
		w.Header().Set("Last-Modified", "Tue, 21 Jul 2026 10:00:00 GMT")
		_, _ = w.Write([]byte(`{
  "version": "1",
  "id": "page-1",
  "next_url": "/api/v1/feed?page=2",
  "next_id": "page-2",
  "items": [
    {"id":"entry-active","url":"/api/v1/feed/active","title":"Engineer","date_modified":"2026-07-21T10:00:00Z","_feed_entry":{"uuid":"ad-active","status":"ACTIVE","title":"Engineer","businessName":"Example AS","municipal":"Oslo","sistEndret":"2026-07-21T10:00:00Z"}},
    {"id":"entry-inactive","url":"/api/v1/feed/inactive","title":"Old role","date_modified":"2026-07-21T10:01:00Z","_feed_entry":{"uuid":"ad-removed","status":"INACTIVE","title":"Old role","businessName":"Example AS","municipal":"Oslo","sistEndret":"2026-07-21T10:01:00Z"}}
  ]
}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL, "nav-token")
	page, err := client.FetchPage(context.Background(), "")
	if err != nil {
		t.Fatalf("FetchPage() error = %v", err)
	}
	if page.NextURL != "/api/v1/feed?page=2" || page.ETag != `"page-1"` {
		t.Fatalf("page cursor metadata = %+v", page)
	}

	state := State{Vacancies: []Vacancy{{ID: "ad-removed", Title: "Old role", Status: "ACTIVE"}}}
	next, diff := Apply(state, page)
	if len(diff.Upserted) != 1 || diff.Upserted[0].ID != "ad-active" {
		t.Fatalf("upserted = %+v", diff.Upserted)
	}
	if len(diff.Removed) != 1 || diff.Removed[0] != "ad-removed" {
		t.Fatalf("removed = %+v", diff.Removed)
	}
	if len(next.Vacancies) != 1 || next.Vacancies[0].ID != "ad-active" {
		t.Fatalf("next state = %+v", next.Vacancies)
	}
}

func TestFetchPageHonorsNotModifiedAndRejectsExternalNextURL(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("If-None-Match") == `"page-1"` {
			w.WriteHeader(http.StatusNotModified)
			return
		}
		_, _ = w.Write([]byte(`{"next_url":"https://not-nav.example/feed","items":[]}`))
	}))
	defer server.Close()

	client := NewClient(server.Client(), server.URL, "nav-token")
	page, err := client.FetchPageWithHeaders(context.Background(), "", `"page-1"`, "")
	if err != nil {
		t.Fatalf("FetchPageWithHeaders() error = %v", err)
	}
	if !page.NotModified {
		t.Fatal("NotModified = false")
	}
	if _, err := client.FetchPage(context.Background(), ""); err == nil {
		t.Fatal("FetchPage() error = nil for external next URL")
	}
}

func TestVacancyCollectorRejectsMissingTokenAndOversizedPage(t *testing.T) {
	client := NewClient(http.DefaultClient, "http://127.0.0.1:1", "")
	if _, err := client.FetchPage(context.Background(), ""); err == nil {
		t.Fatal("FetchPage() error = nil without token")
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(strings.Repeat("x", 64)))
	}))
	defer server.Close()
	client = NewClient(server.Client(), server.URL, "token")
	client.MaxResponseBytes = 32
	if _, err := client.FetchPage(context.Background(), ""); err == nil {
		t.Fatal("FetchPage() error = nil for oversized page")
	}
}

func TestApplyUpdatesChangedActiveVacancy(t *testing.T) {
	page := Page{Items: []Change{{ID: "ad-1", Status: "ACTIVE", Title: "New title", Municipal: "Bergen"}}}
	next, diff := Apply(State{Vacancies: []Vacancy{{ID: "ad-1", Status: "ACTIVE", Title: "Old title", Municipal: "Bergen"}}}, page)
	if len(diff.Upserted) != 1 || diff.Upserted[0].Title != "New title" {
		t.Fatalf("upserted = %+v", diff.Upserted)
	}
	if len(next.Vacancies) != 1 || next.Vacancies[0].Title != "New title" {
		t.Fatalf("next = %+v", next.Vacancies)
	}
}
