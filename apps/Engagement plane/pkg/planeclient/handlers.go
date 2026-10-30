package planeclient

import (
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"strings"
	"sync"
	"time"
)

func respond(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}
func handlerError(w http.ResponseWriter, status int, code string) {
	respond(w, status, map[string]any{"error": map[string]string{"code": code, "message": "The Studio plane request could not be completed"}})
}

// ConnectionsHandler reports local configuration/credential assertions only.
// It MUST be mounted behind the owning core's authentication, never publicly.
func ConnectionsHandler(client *Client) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "GET" {
			w.Header().Set("Allow", "GET")
			handlerError(w, 405, "method_not_allowed")
			return
		}
		if r.URL.RawQuery != "" {
			handlerError(w, 400, "invalid_request")
			return
		}
		respond(w, 200, map[string]any{"data": client.Readiness()})
	})
}

// DraftHandler is a bounded, non-persisting unary Model request. It does not
// register agent tools, ingest content or authorize domain/provider writes.
// A fixed deployment principal/org is used; caller-supplied org/tool fields fail.
func DraftHandler(client *Client) http.Handler {
	var mu sync.Mutex
	var until time.Time
	count := 0
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != "POST" {
			w.Header().Set("Allow", "POST")
			handlerError(w, 405, "method_not_allowed")
			return
		}
		media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
		if err != nil || media != "application/json" {
			handlerError(w, 415, "invalid_content_type")
			return
		}
		if r.URL.RawQuery != "" {
			handlerError(w, 400, "invalid_request")
			return
		}
		prompt, err := draftPrompt(http.MaxBytesReader(w, r.Body, 16384))
		if err != nil {
			handlerError(w, 400, "invalid_request")
			return
		}
		mu.Lock()
		if !time.Now().Before(until) {
			until = time.Now().Add(time.Minute)
			count = 0
		}
		count++
		limited := count > 6
		mu.Unlock()
		if limited {
			w.Header().Set("Retry-After", "60")
			handlerError(w, 429, "rate_limited")
			return
		}
		value, err := client.Draft(r.Context(), prompt)
		if err != nil {
			status, code := 502, "source_unavailable"
			if errors.Is(err, ErrUnavailable) {
				status = 503
				code = "not_configured"
			} else if errors.Is(err, ErrInvalidInput) {
				status = 400
				code = "invalid_request"
			}
			handlerError(w, status, code)
			return
		}
		respond(w, 200, map[string]any{"data": value})
	})
}
func draftPrompt(reader io.Reader) (string, error) {
	invalid := errors.New("invalid draft request")
	d := json.NewDecoder(reader)
	tok, err := d.Token()
	if err != nil || tok != json.Delim('{') {
		return "", invalid
	}
	if !d.More() {
		return "", invalid
	}
	key, err := d.Token()
	if err != nil || key != "prompt" {
		return "", invalid
	}
	var prompt string
	if d.Decode(&prompt) != nil || d.More() {
		return "", invalid
	}
	tok, err = d.Token()
	if err != nil || tok != json.Delim('}') || d.Decode(new(any)) != io.EOF || len(prompt) > 8192 || strings.TrimSpace(prompt) == "" {
		return "", invalid
	}
	return prompt, nil
}
