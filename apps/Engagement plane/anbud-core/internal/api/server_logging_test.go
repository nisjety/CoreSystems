package api

import (
	"bytes"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"
)

// Fiber only runs the ErrorHandler after the middleware chain has returned, so
// a naive access log records 200 for every error, and a panic that unwinds
// through it is never logged at all.
func TestRequestLogShowsTheRealStatusOfErrorPaths(t *testing.T) {
	t.Parallel()

	var logs bytes.Buffer
	logger := zerolog.New(&logs)
	app := newServer(ServerConfig{Logger: &logger})
	app.Get("/boom", func(*fiber.Ctx) error { return errors.New("db exploded") })
	app.Get("/teapot", func(*fiber.Ctx) error { return fiber.NewError(http.StatusTeapot, "short and stout") })
	app.Get("/panic", func(*fiber.Ctx) error { panic("kaboom") })

	cases := []struct {
		method string
		path   string
		want   int
	}{
		{http.MethodGet, "/health", 200},
		{http.MethodGet, "/nope", 404},
		{http.MethodPost, "/health", 405},
		{http.MethodGet, "/boom", 500},
		{http.MethodGet, "/teapot", 418},
		{http.MethodGet, "/panic", 500},
	}
	for _, tc := range cases {
		if status, _ := do(t, app, tc.method, tc.path, nil); status != tc.want {
			t.Fatalf("%s %s: status = %d, want %d", tc.method, tc.path, status, tc.want)
		}
	}

	logged := map[string]int{}
	for line := range strings.SplitSeq(strings.TrimSpace(logs.String()), "\n") {
		var entry struct {
			Message string `json:"message"`
			Method  string `json:"method"`
			Path    string `json:"path"`
			Status  int    `json:"status"`
		}
		if json.Unmarshal([]byte(line), &entry) != nil || entry.Message != "http_request" {
			continue
		}
		logged[entry.Method+" "+entry.Path] = entry.Status
	}
	for _, tc := range cases {
		key := tc.method + " " + tc.path
		got, ok := logged[key]
		if !ok {
			t.Errorf("%s was never logged (a panic unwinds past a logger registered inside recover)", key)
			continue
		}
		if got != tc.want {
			t.Errorf("%s logged status %d, want %d", key, got, tc.want)
		}
	}
}

func TestServerSetsConnectionTimeoutsAndABodyLimit(t *testing.T) {
	t.Parallel()

	cfg := newServer(ServerConfig{}).Config()
	if cfg.ReadTimeout <= 0 || cfg.WriteTimeout <= 0 || cfg.IdleTimeout <= 0 {
		t.Errorf("timeouts read/write/idle = %v/%v/%v, want all set", cfg.ReadTimeout, cfg.WriteTimeout, cfg.IdleTimeout)
	}
	if cfg.BodyLimit <= 0 || cfg.BodyLimit > 1<<20 {
		t.Errorf("BodyLimit = %d, want a limit of at most 1 MiB (the API takes no request bodies yet)", cfg.BodyLimit)
	}
}
