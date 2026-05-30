package api

import (
	"context"
	"net/http"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	quarrybrowser "github.com/triodelab/quarry/internal/browser"
	"github.com/triodelab/quarry/internal/session"
)

type stubAPIBrowserRuntime struct {
	state          quarrybrowser.SessionState
	html           string
	liveScreenshot string
	createCalls    []quarrybrowser.CreateRequest
	executeCalls   []quarrybrowser.ExecuteRequest
	deleteCalls    []string
	listResponses  []session.SessionInfo
}

func (s *stubAPIBrowserRuntime) Create(_ context.Context, req quarrybrowser.CreateRequest) (*quarrybrowser.CreateResponse, error) {
	s.createCalls = append(s.createCalls, req)
	s.state.Session.URL = req.URL
	if s.state.CurrentURL == "" {
		s.state.CurrentURL = req.URL
	}
	return &quarrybrowser.CreateResponse{
		Success: true,
		State:   s.state,
		HTML:    s.html,
	}, nil
}

func (s *stubAPIBrowserRuntime) Get(context.Context, string) (*quarrybrowser.SessionState, error) {
	state := s.state
	return &state, nil
}

func (s *stubAPIBrowserRuntime) List(context.Context) ([]session.SessionInfo, error) {
	if len(s.listResponses) > 0 {
		return append([]session.SessionInfo(nil), s.listResponses...), nil
	}
	return []session.SessionInfo{s.state.Session}, nil
}

func (s *stubAPIBrowserRuntime) Execute(_ context.Context, _ string, req quarrybrowser.ExecuteRequest) (*quarrybrowser.ExecuteResponse, error) {
	s.executeCalls = append(s.executeCalls, req)
	s.state.Session.StepCount += len(req.Actions)
	return &quarrybrowser.ExecuteResponse{
		Success: true,
		State:   s.state,
		Results: []session.ActionResult{{Type: session.ActionClick, Success: true}},
	}, nil
}

func (s *stubAPIBrowserRuntime) HTML(context.Context, string) (*quarrybrowser.HTMLResponse, error) {
	return &quarrybrowser.HTMLResponse{
		Success:    true,
		CurrentURL: s.state.CurrentURL,
		HTML:       s.html,
	}, nil
}

func (s *stubAPIBrowserRuntime) Live(context.Context, string) (*quarrybrowser.LiveResponse, error) {
	return &quarrybrowser.LiveResponse{
		Success:       true,
		CurrentURL:    s.state.CurrentURL,
		ScreenshotB64: s.liveScreenshot,
	}, nil
}

func (s *stubAPIBrowserRuntime) Delete(_ context.Context, id string) error {
	s.deleteCalls = append(s.deleteCalls, id)
	return nil
}

func (s *stubAPIBrowserRuntime) Close() error {
	return nil
}

func TestV1BrowserEndpointsUseBrowserRuntime(t *testing.T) {
	t.Parallel()

	now := time.Now().UTC()
	runtime := &stubAPIBrowserRuntime{
		state: quarrybrowser.SessionState{
			Session: session.SessionInfo{
				ID:        "browser-1",
				URL:       "https://example.com",
				Profile:   "acct-a",
				CreatedAt: now,
				ExpiresAt: now.Add(time.Hour),
				LastUsed:  now,
			},
			CurrentURL: "https://example.com/app",
		},
		html:           "<html>app</html>",
		liveScreenshot: "ZmFrZS1saXZl",
	}

	handler := &Handler{browserRuntime: runtime}
	app := fiber.New()
	handler.registerPlatformV1(app)

	createResp := performJSONRequest(t, app, http.MethodPost, "/v1/browser", map[string]interface{}{
		"url":     "https://example.com",
		"profile": "acct-a",
	})
	if createResp.StatusCode != http.StatusCreated {
		t.Fatalf("status = %d, want %d", createResp.StatusCode, http.StatusCreated)
	}

	var created browserSessionEnvelope
	decodeJSONResponse(t, createResp, &created)
	if created.ID != "browser-1" {
		t.Fatalf("create id = %q, want browser-1", created.ID)
	}
	if created.Session.URL != "https://example.com/app" {
		t.Fatalf("create url = %q, want current url", created.Session.URL)
	}
	if len(runtime.createCalls) != 1 || runtime.createCalls[0].Profile != "acct-a" {
		t.Fatalf("create calls = %+v, want profile acct-a", runtime.createCalls)
	}

	listResp := performJSONRequest(t, app, http.MethodGet, "/v1/browser", nil)
	var listed browserListEnvelope
	decodeJSONResponse(t, listResp, &listed)
	if listed.Count != 1 || listed.Data[0].ID != "browser-1" {
		t.Fatalf("list = %+v, want one browser-1 session", listed)
	}

	getResp := performJSONRequest(t, app, http.MethodGet, "/v1/browser/browser-1", nil)
	var current browserSessionEnvelope
	decodeJSONResponse(t, getResp, &current)
	if current.Session.URL != "https://example.com/app" {
		t.Fatalf("session url = %q, want current url", current.Session.URL)
	}

	liveResp := performJSONRequest(t, app, http.MethodGet, "/v1/browser/browser-1/live", nil)
	var live quarrybrowser.LiveResponse
	decodeJSONResponse(t, liveResp, &live)
	if live.ScreenshotB64 != "ZmFrZS1saXZl" {
		t.Fatalf("live screenshot = %q, want fake screenshot", live.ScreenshotB64)
	}

	execResp := performJSONRequest(t, app, http.MethodPost, "/v1/browser/browser-1/execute", map[string]interface{}{
		"actions": []map[string]interface{}{
			{"type": "click", "selector": "#go"},
		},
		"return_html": true,
	})
	var interacted InteractResponse
	decodeJSONResponse(t, execResp, &interacted)
	if len(interacted.Results) != 1 || !interacted.Results[0].Success {
		t.Fatalf("results = %+v, want one successful action", interacted.Results)
	}
	if interacted.HTML != "<html>app</html>" {
		t.Fatalf("html = %q, want browser html", interacted.HTML)
	}

	deleteResp := performJSONRequest(t, app, http.MethodDelete, "/v1/browser/browser-1", nil)
	var deleted struct {
		Success bool `json:"success"`
	}
	decodeJSONResponse(t, deleteResp, &deleted)
	if !deleted.Success {
		t.Fatal("delete success = false, want true")
	}
	if len(runtime.deleteCalls) != 1 || runtime.deleteCalls[0] != "browser-1" {
		t.Fatalf("delete calls = %v, want [browser-1]", runtime.deleteCalls)
	}
}

func TestV2InteractiveEndpointsUseBrowserRuntime(t *testing.T) {
	t.Parallel()

	now := time.Now().UTC()
	runtime := &stubAPIBrowserRuntime{
		state: quarrybrowser.SessionState{
			Session: session.SessionInfo{
				ID:        "scrape-1",
				URL:       "https://example.com",
				CreatedAt: now,
				ExpiresAt: now.Add(time.Hour),
				LastUsed:  now,
			},
			CurrentURL: "https://example.com/dashboard",
		},
		html: "<html>dashboard</html>",
	}

	handler := &Handler{browserRuntime: runtime}
	app := fiber.New()
	handler.RegisterV2(app)

	scrapeResp := performJSONRequest(t, app, http.MethodPost, "/v2/scrape", map[string]interface{}{
		"url": "https://example.com",
	})
	if scrapeResp.StatusCode != http.StatusCreated {
		t.Fatalf("status = %d, want %d", scrapeResp.StatusCode, http.StatusCreated)
	}

	var scraped V2ScrapeResponse
	decodeJSONResponse(t, scrapeResp, &scraped)
	if scraped.ScrapeID != "scrape-1" {
		t.Fatalf("scrape id = %q, want scrape-1", scraped.ScrapeID)
	}
	if scraped.HTML != "<html>dashboard</html>" {
		t.Fatalf("html = %q, want dashboard html", scraped.HTML)
	}

	interactResp := performJSONRequest(t, app, http.MethodPost, "/v2/scrape/scrape-1/interact", map[string]interface{}{
		"actions": []map[string]interface{}{
			{"type": "click", "selector": "#submit"},
		},
		"return_html": true,
	})
	var interacted InteractResponse
	decodeJSONResponse(t, interactResp, &interacted)
	if len(interacted.Results) != 1 || !interacted.Results[0].Success {
		t.Fatalf("results = %+v, want one successful action", interacted.Results)
	}
	if interacted.Session.URL != "https://example.com/dashboard" {
		t.Fatalf("session url = %q, want current url", interacted.Session.URL)
	}

	sessionsResp := performJSONRequest(t, app, http.MethodGet, "/v2/sessions", nil)
	var listed SessionListResponse
	decodeJSONResponse(t, sessionsResp, &listed)
	if listed.Count != 1 || listed.Sessions[0].ID != "scrape-1" {
		t.Fatalf("sessions = %+v, want one scrape-1 session", listed)
	}

	destroyResp := performJSONRequest(t, app, http.MethodDelete, "/v2/scrape/scrape-1", nil)
	var destroyed struct {
		Success bool `json:"success"`
	}
	decodeJSONResponse(t, destroyResp, &destroyed)
	if !destroyed.Success {
		t.Fatal("destroy success = false, want true")
	}
	if len(runtime.deleteCalls) != 1 || runtime.deleteCalls[0] != "scrape-1" {
		t.Fatalf("delete calls = %v, want [scrape-1]", runtime.deleteCalls)
	}
}
