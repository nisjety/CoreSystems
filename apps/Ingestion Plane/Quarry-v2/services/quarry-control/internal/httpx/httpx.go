package httpx

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"net/http"
	"os"
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

// Health is a liveness probe — process is up and serving requests.
// It does NOT check downstreams; that's what Ready is for.
func Health(w http.ResponseWriter, _ *http.Request) { _, _ = w.Write([]byte("ok")) }

// Version reports what is actually running in this container: the git
// revision and build timestamp baked into the image (see the service's
// Dockerfile `ARG SOURCE_REVISION` / `ARG BUILD_DATE`, re-exposed as
// runtime ENV so no Docker/registry access is needed to answer "what SHA
// is deployed here"). Falls back to the same unverified/unknown defaults
// the image LABELs use when unset. `service` names the binary answering
// (e.g. "quarry-control").
func Version(service string) http.HandlerFunc {
	return func(w http.ResponseWriter, _ *http.Request) {
		revision := os.Getenv("SOURCE_REVISION")
		if revision == "" {
			revision = "unverified"
		}
		buildDate := os.Getenv("BUILD_DATE")
		if buildDate == "" {
			buildDate = "unknown"
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{
			"service":    service,
			"revision":   revision,
			"build_date": buildDate,
		})
	}
}

// Pinger is the slice of pgxpool.Pool the ready handler actually uses.
// Defining it here (where it's consumed) avoids pulling pgx into this
// package; main wires the real pool via ReadyWithPing.
type Pinger interface {
	Ping(ctx context.Context) error
}

// Ready returns 503 when the database is unreachable. Without this
// the load balancer keeps routing traffic to a control plane whose
// pool is dead — every request 500s with no signal at the LB layer.
// `ping` may be nil for tests / dev-without-DB; falls back to "ready".
func ReadyWithPing(ping Pinger) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if ping == nil {
			_, _ = w.Write([]byte("ready"))
			return
		}
		ctx, cancel := context.WithTimeout(r.Context(), 2*time.Second)
		defer cancel()
		if err := ping.Ping(ctx); err != nil {
			log.Warn().Err(err).Msg("/ready: db ping failed")
			w.WriteHeader(http.StatusServiceUnavailable)
			_, _ = w.Write([]byte("db unreachable"))
			return
		}
		_, _ = w.Write([]byte("ready"))
	}
}

// Ready is the dev/no-DB fallback the router can use when there's no
// pool to ping (e.g. memory-store mode).
func Ready(w http.ResponseWriter, _ *http.Request) { _, _ = w.Write([]byte("ready")) }

func WriteJSON[T any](w http.ResponseWriter, r *http.Request, status int, data T) {
	env := quarrycontracts.OK(RequestIDOf(r.Context()), data)
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(env)
}

// WriteRawJSON emits `data` WITHOUT the {request_id,data} envelope. Only
// for endpoints whose Rust caller parses the body as a bare JSON object
// (quarry-edge's forward_one::<T> on /v1/team/* aggregates) — list
// endpoints keep WriteJSON/forward_list semantics.
func WriteRawJSON[T any](w http.ResponseWriter, r *http.Request, status int, data T) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(data)
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
// If key is empty, the middleware is a no-op (useful for local/dev). The
// token comparison uses `crypto/subtle.ConstantTimeCompare` so an
// attacker can't recover the key one byte at a time by measuring
// response latency — the difference between a 1-byte-wrong header and
// a 32-byte-wrong header would otherwise be observable.
func BearerAuth(key string) func(http.Handler) http.Handler {
	keyBytes := []byte(key)
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if len(keyBytes) == 0 {
				next.ServeHTTP(w, r)
				return
			}
			h := r.Header.Get("Authorization")
			if !strings.HasPrefix(h, "Bearer ") {
				WriteErr(w, r, quarrycontracts.CodeUnauthorized, "missing or invalid bearer", nil)
				return
			}
			provided := []byte(strings.TrimPrefix(h, "Bearer "))
			if subtle.ConstantTimeCompare(provided, keyBytes) != 1 {
				WriteErr(w, r, quarrycontracts.CodeUnauthorized, "missing or invalid bearer", nil)
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}
