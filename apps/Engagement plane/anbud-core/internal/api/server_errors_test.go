package api

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"
	"github.com/rs/zerolog"
)

func do(t *testing.T, app *fiber.App, method, path string, headers map[string]string) (int, string) {
	t.Helper()
	req := httptest.NewRequest(method, path, nil)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	return resp.StatusCode, string(raw)
}

func errorBody(t *testing.T, raw string) (code, message string) {
	t.Helper()
	var envelope struct {
		Error struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal([]byte(raw), &envelope); err != nil {
		t.Fatalf("not an error envelope: %q (%v)", raw, err)
	}
	return envelope.Error.Code, envelope.Error.Message
}

func TestUnhandledErrorsAreHiddenBehindAnInternalEnvelope(t *testing.T) {
	t.Parallel()

	app := newServer(ServerConfig{})
	app.Get("/boom", func(*fiber.Ctx) error {
		return errors.New(`pq: password authentication failed for user "ingestion_user"`)
	})

	status, raw := do(t, app, http.MethodGet, "/boom", nil)
	code, message := errorBody(t, raw)
	if status != 500 || code != "internal" || message != "internal error" {
		t.Fatalf("got %d %s", status, raw)
	}
	if strings.Contains(raw, "password") || strings.Contains(raw, "ingestion_user") {
		t.Fatalf("an internal error leaked into the response: %s", raw)
	}
}

func TestPanicsAreRecoveredIntoTheInternalEnvelope(t *testing.T) {
	t.Parallel()

	app := newServer(ServerConfig{})
	app.Get("/panic", func(*fiber.Ctx) error { panic("kaboom with a secret detail") })

	status, raw := do(t, app, http.MethodGet, "/panic", nil)
	if code, _ := errorBody(t, raw); status != 500 || code != "internal" {
		t.Fatalf("got %d %s", status, raw)
	}
	if strings.Contains(raw, "kaboom") {
		t.Fatalf("panic detail leaked: %s", raw)
	}
}

func TestWrongMethodUsesTheErrorEnvelope(t *testing.T) {
	t.Parallel()

	status, raw := do(t, newServer(ServerConfig{}), http.MethodPost, "/health", nil)
	if code, _ := errorBody(t, raw); status != 405 || code != "method_not_allowed" {
		t.Fatalf("got %d %s", status, raw)
	}
}

func TestErrorCodeMapping(t *testing.T) {
	t.Parallel()

	tests := map[int]string{
		400: "bad_request",
		401: "unauthorized",
		403: "forbidden",
		404: "not_found",
		405: "method_not_allowed",
		413: "payload_too_large",
		429: "rate_limited",
		500: "internal",
		503: "internal",
		418: "error",
	}
	for status, want := range tests {
		if got := errorCode(status); got != want {
			t.Errorf("errorCode(%d) = %q, want %q", status, got, want)
		}
	}
}

func TestRequestLogRecordsMethodPathAndStatusButNotQueryOrKeys(t *testing.T) {
	t.Parallel()

	var logs bytes.Buffer
	logger := zerolog.New(&logs)
	app := newServer(ServerConfig{Logger: &logger})

	do(t, app, http.MethodGet, "/health?searchString=confidential-tender-text", map[string]string{"X-API-Key": serverKey})

	line := logs.String()
	for _, want := range []string{`"path":"/health"`, `"method":"GET"`, `"status":200`} {
		if !strings.Contains(line, want) {
			t.Errorf("request log %q should contain %s", line, want)
		}
	}
	for _, leak := range []string{"confidential-tender-text", serverKey} {
		if strings.Contains(line, leak) {
			t.Errorf("request log leaked %q: %s", leak, line)
		}
	}
}

func TestReadinessLogsTheDatabaseErrorButDoesNotReturnIt(t *testing.T) {
	t.Parallel()

	var logs bytes.Buffer
	logger := zerolog.New(&logs)
	app := newServer(ServerConfig{
		DB:     fakePinger{err: errors.New(`password authentication failed for user "ingestion_user"`)},
		Logger: &logger,
	})

	status, raw := do(t, app, http.MethodGet, "/ready", nil)
	if status != 503 {
		t.Fatalf("status = %d", status)
	}
	if strings.Contains(raw, "password") {
		t.Fatalf("response leaked the database error: %s", raw)
	}
	if !strings.Contains(logs.String(), "password authentication failed") {
		t.Fatalf("the database error should reach the server log: %s", logs.String())
	}
}
