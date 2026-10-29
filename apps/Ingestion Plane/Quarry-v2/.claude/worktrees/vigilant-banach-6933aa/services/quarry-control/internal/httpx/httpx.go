package httpx

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

type ctxKey string

const (
	ctxRequestID ctxKey = "request_id"
)

func RequestID(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		id := r.Header.Get("X-Request-Id")
		if id == "" {
			id = "req_" + uuid.NewString()
		}
		w.Header().Set("X-Request-Id", id)
		ctx := context.WithValue(r.Context(), ctxRequestID, id)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}

func RequestIDOf(ctx context.Context) string {
	if v, ok := ctx.Value(ctxRequestID).(string); ok {
		return v
	}
	return ""
}

func Logger(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		start := time.Now()
		sr := &statusRecorder{ResponseWriter: w, status: 200}
		next.ServeHTTP(sr, r)
		log.Info().
			Str("method", r.Method).
			Str("path", r.URL.Path).
			Int("status", sr.status).
			Dur("duration", time.Since(start)).
			Str("request_id", RequestIDOf(r.Context())).
			Msg("http")
	})
}

type statusRecorder struct {
	http.ResponseWriter
	status int
}

func (sr *statusRecorder) WriteHeader(code int) {
	sr.status = code
	sr.ResponseWriter.WriteHeader(code)
}

func Recover(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if rec := recover(); rec != nil {
				log.Error().Interface("panic", rec).Msg("handler panic")
				WriteErr(w, r, quarrycontracts.CodeInternal, "internal error", nil)
			}
		}()
		next.ServeHTTP(w, r)
	})
}

func Health(w http.ResponseWriter, _ *http.Request) { w.Write([]byte("ok")) }
func Ready(w http.ResponseWriter, _ *http.Request)  { w.Write([]byte("ready")) }

func WriteJSON[T any](w http.ResponseWriter, r *http.Request, status int, data T) {
	env := quarrycontracts.OK(RequestIDOf(r.Context()), data)
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(env)
}

func WriteErr(w http.ResponseWriter, r *http.Request, code quarrycontracts.ErrorCode, msg string, details map[string]any) {
	env := quarrycontracts.Err(RequestIDOf(r.Context()), quarrycontracts.ErrorEnvelope{
		Code: code, Message: msg, Details: details,
	})
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code.HTTPStatus())
	_ = json.NewEncoder(w).Encode(env)
}

// BearerAuth returns middleware that enforces `Authorization: Bearer <key>`.
// If key is empty, the middleware is a no-op (useful for local/dev).
func BearerAuth(key string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if key == "" {
				next.ServeHTTP(w, r)
				return
			}
			h := r.Header.Get("Authorization")
			if !strings.HasPrefix(h, "Bearer ") || strings.TrimPrefix(h, "Bearer ") != key {
				WriteErr(w, r, quarrycontracts.CodeUnauthorized, "missing or invalid bearer", nil)
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}
