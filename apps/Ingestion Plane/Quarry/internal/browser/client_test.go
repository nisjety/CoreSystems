package browser

import (
	"context"
	"net"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/session"
)

type stubRuntime struct {
	state          SessionState
	html           string
	liveScreenshot string
	createRequests []CreateRequest
	executeCalls   []ExecuteRequest
	deleted        []string
}

func (s *stubRuntime) Create(_ context.Context, req CreateRequest) (*CreateResponse, error) {
	s.createRequests = append(s.createRequests, req)
	s.state.Session.URL = req.URL
	if s.state.CurrentURL == "" {
		s.state.CurrentURL = req.URL
	}
	return &CreateResponse{
		Success: true,
		State:   s.state,
		HTML:    s.html,
	}, nil
}

func (s *stubRuntime) Get(context.Context, string) (*SessionState, error) {
	state := s.state
	return &state, nil
}

func (s *stubRuntime) List(context.Context) ([]session.SessionInfo, error) {
	return []session.SessionInfo{s.state.Session}, nil
}

func (s *stubRuntime) Execute(_ context.Context, _ string, req ExecuteRequest) (*ExecuteResponse, error) {
	s.executeCalls = append(s.executeCalls, req)
	s.state.Session.StepCount += len(req.Actions)
	return &ExecuteResponse{
		Success: true,
		State:   s.state,
		Results: []session.ActionResult{{Type: session.ActionClick, Success: true}},
		HTML:    s.html,
	}, nil
}

func (s *stubRuntime) HTML(context.Context, string) (*HTMLResponse, error) {
	return &HTMLResponse{
		Success:    true,
		CurrentURL: s.state.CurrentURL,
		HTML:       s.html,
	}, nil
}

func (s *stubRuntime) Live(context.Context, string) (*LiveResponse, error) {
	return &LiveResponse{
		Success:       true,
		CurrentURL:    s.state.CurrentURL,
		ScreenshotB64: s.liveScreenshot,
	}, nil
}

func (s *stubRuntime) Delete(_ context.Context, id string) error {
	s.deleted = append(s.deleted, id)
	return nil
}

func (s *stubRuntime) Close() error {
	return nil
}

func TestClientRoundTripAgainstBrowserServer(t *testing.T) {
	t.Parallel()

	now := time.Now().UTC()
	runtime := &stubRuntime{
		state: SessionState{
			Session: session.SessionInfo{
				ID:        "browser-1",
				URL:       "https://example.com",
				Profile:   "team-a",
				CreatedAt: now,
				ExpiresAt: now.Add(time.Hour),
				LastUsed:  now,
			},
			CurrentURL: "https://example.com/dashboard",
		},
		html:           "<html>dashboard</html>",
		liveScreenshot: "ZmFrZS1wbmc=",
	}

	app := fiber.New()
	NewServer(runtime, "secret").Register(app)

	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("net.Listen() error = %v", err)
	}
	defer ln.Close()

	go func() {
		_ = app.Listener(ln)
	}()
	t.Cleanup(func() {
		_ = app.Shutdown()
	})

	time.Sleep(25 * time.Millisecond)

	client := NewClient("http://"+ln.Addr().String(), "secret", time.Second)
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()

	createResp, err := client.Create(ctx, CreateRequest{URL: "https://example.com", Profile: "team-a"})
	if err != nil {
		t.Fatalf("Create() error = %v", err)
	}
	if createResp.State.Session.ID != "browser-1" {
		t.Fatalf("create id = %q, want browser-1", createResp.State.Session.ID)
	}
	if len(runtime.createRequests) != 1 || runtime.createRequests[0].Profile != "team-a" {
		t.Fatalf("create requests = %+v, want profile team-a", runtime.createRequests)
	}

	state, err := client.Get(ctx, "browser-1")
	if err != nil {
		t.Fatalf("Get() error = %v", err)
	}
	if state.CurrentURL != "https://example.com/dashboard" {
		t.Fatalf("current url = %q, want dashboard", state.CurrentURL)
	}

	sessions, err := client.List(ctx)
	if err != nil {
		t.Fatalf("List() error = %v", err)
	}
	if len(sessions) != 1 || sessions[0].ID != "browser-1" {
		t.Fatalf("sessions = %+v, want one browser-1 session", sessions)
	}

	execResp, err := client.Execute(ctx, "browser-1", ExecuteRequest{
		Actions: []session.Action{{Type: session.ActionClick, Selector: "#go"}},
	})
	if err != nil {
		t.Fatalf("Execute() error = %v", err)
	}
	if len(execResp.Results) != 1 || !execResp.Results[0].Success {
		t.Fatalf("results = %+v, want one successful action", execResp.Results)
	}

	htmlResp, err := client.HTML(ctx, "browser-1")
	if err != nil {
		t.Fatalf("HTML() error = %v", err)
	}
	if htmlResp.HTML != "<html>dashboard</html>" {
		t.Fatalf("html = %q, want dashboard html", htmlResp.HTML)
	}

	liveResp, err := client.Live(ctx, "browser-1")
	if err != nil {
		t.Fatalf("Live() error = %v", err)
	}
	if liveResp.ScreenshotB64 != "ZmFrZS1wbmc=" {
		t.Fatalf("live screenshot = %q, want fake screenshot", liveResp.ScreenshotB64)
	}

	if err := client.Delete(ctx, "browser-1"); err != nil {
		t.Fatalf("Delete() error = %v", err)
	}
	if len(runtime.deleted) != 1 || runtime.deleted[0] != "browser-1" {
		t.Fatalf("deleted = %v, want [browser-1]", runtime.deleted)
	}
}
