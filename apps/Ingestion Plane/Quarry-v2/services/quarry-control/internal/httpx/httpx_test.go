package httpx

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func TestRequestID_GeneratesAndPropagates(t *testing.T) {
	t.Parallel()

	var seen string
	h := RequestID(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen = RequestIDOf(r.Context())
		w.WriteHeader(http.StatusNoContent)
	}))

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	rr := httptest.NewRecorder()
	h.ServeHTTP(rr, req)

	if rr.Code != http.StatusNoContent {
		t.Fatalf("status=%d want=%d", rr.Code, http.StatusNoContent)
	}
	if seen == "" {
		t.Fatal("expected request id in context")
	}
	if !strings.HasPrefix(seen, "req_") {
		t.Fatalf("generated id missing req_ prefix: %q", seen)
	}
	if got := rr.Header().Get("X-Request-Id"); got != seen {
		t.Fatalf("response header request id=%q want=%q", got, seen)
	}
}

func TestRequestID_UsesIncomingHeader(t *testing.T) {
	t.Parallel()

	const incoming = "req-fixed"
	var seen string
	h := RequestID(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seen = RequestIDOf(r.Context())
		w.WriteHeader(http.StatusNoContent)
	}))

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Header.Set("X-Request-Id", incoming)
	rr := httptest.NewRecorder()
	h.ServeHTTP(rr, req)

	if seen != incoming {
		t.Fatalf("context request id=%q want=%q", seen, incoming)
	}
	if got := rr.Header().Get("X-Request-Id"); got != incoming {
		t.Fatalf("response request id=%q want=%q", got, incoming)
	}
}

func TestBearerAuth_AllPaths(t *testing.T) {
	t.Parallel()

	t.Run("no key configured is pass-through", func(t *testing.T) {
		t.Parallel()
		h := BearerAuth("")(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.WriteHeader(http.StatusAccepted)
		}))

		rr := httptest.NewRecorder()
		h.ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/", nil))
		if rr.Code != http.StatusAccepted {
			t.Fatalf("status=%d want=%d", rr.Code, http.StatusAccepted)
		}
	})

	t.Run("invalid header is unauthorized", func(t *testing.T) {
		t.Parallel()
		h := BearerAuth("secret")(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.WriteHeader(http.StatusAccepted)
		}))

		rr := httptest.NewRecorder()
		h.ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/", nil))
		if rr.Code != http.StatusUnauthorized {
			t.Fatalf("status=%d want=%d", rr.Code, http.StatusUnauthorized)
		}
		if !strings.Contains(rr.Body.String(), "invalid bearer") {
			t.Fatalf("expected invalid bearer error, body=%q", rr.Body.String())
		}
	})

	t.Run("valid bearer reaches next", func(t *testing.T) {
		t.Parallel()
		h := BearerAuth("secret")(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.WriteHeader(http.StatusAccepted)
		}))

		req := httptest.NewRequest(http.MethodGet, "/", nil)
		req.Header.Set("Authorization", "Bearer secret")
		rr := httptest.NewRecorder()
		h.ServeHTTP(rr, req)

		if rr.Code != http.StatusAccepted {
			t.Fatalf("status=%d want=%d", rr.Code, http.StatusAccepted)
		}
	})
}

func TestRecover_ConvertsPanicToInternalError(t *testing.T) {
	t.Parallel()

	h := Recover(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		panic("boom")
	}))
	rr := httptest.NewRecorder()
	h.ServeHTTP(rr, httptest.NewRequest(http.MethodGet, "/", nil))

	if rr.Code != http.StatusInternalServerError {
		t.Fatalf("status=%d want=%d", rr.Code, http.StatusInternalServerError)
	}
	if !strings.Contains(rr.Body.String(), "internal error") {
		t.Fatalf("expected internal error body, got=%q", rr.Body.String())
	}
}

func TestWriteJSONAndWriteErr_SetJSONEnvelope(t *testing.T) {
	t.Parallel()

	r := httptest.NewRequest(http.MethodGet, "/", nil)
	rr := httptest.NewRecorder()
	ctxReq := r.WithContext(context.WithValue(r.Context(), ctxRequestID, "req-test"))

	WriteJSON(rr, ctxReq, http.StatusCreated, map[string]string{"ok": "true"})
	if got := rr.Header().Get("Content-Type"); got != "application/json" {
		t.Fatalf("content-type=%q want=application/json", got)
	}
	if rr.Code != http.StatusCreated {
		t.Fatalf("status=%d want=%d", rr.Code, http.StatusCreated)
	}

	errRR := httptest.NewRecorder()
	WriteErr(errRR, ctxReq, quarrycontracts.CodeUnauthorized, "bad auth", nil)
	if errRR.Code != http.StatusUnauthorized {
		t.Fatalf("status=%d want=%d", errRR.Code, http.StatusUnauthorized)
	}
	var payload map[string]any
	if err := json.Unmarshal(errRR.Body.Bytes(), &payload); err != nil {
		t.Fatalf("unmarshal error payload: %v", err)
	}
}
