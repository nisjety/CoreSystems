// Package http owns Visibility Core's intentionally small HTTP boundary.
package http

import (
	"context"
	"crypto/subtle"
	"encoding/json"
	"github.com/triodelab/studio-plane/planeclient"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

const serviceName = "visibility-core"

type Server struct {
	httpServer *http.Server
}

type PulseGetter interface {
	Get(context.Context, string, string) pulse.Projection
}

func NewServer(port int, internalAPIKey string, pulseSource ...PulseGetter) *Server {
	return NewServerWithPlanes(port, internalAPIKey, nil, pulseSource...)
}

func NewServerWithPlanes(port int, internalAPIKey string, planes *planeclient.Client, pulseSource ...PulseGetter) *Server {
	return &Server{httpServer: &http.Server{
		Addr:              ":" + strconv.Itoa(port),
		Handler:           NewHandlerWithPlanes(internalAPIKey, planes, pulseSource...),
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       15 * time.Second,
		WriteTimeout:      20 * time.Second,
		IdleTimeout:       30 * time.Second,
	}}
}

func (s *Server) Start() error {
	return s.httpServer.ListenAndServe()
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.httpServer.Shutdown(ctx)
}

func NewHandler(internalAPIKey string, pulseSource ...PulseGetter) http.Handler {
	return NewHandlerWithPlanes(internalAPIKey, nil, pulseSource...)
}

func NewHandlerWithPlanes(internalAPIKey string, planes *planeclient.Client, pulseSource ...PulseGetter) http.Handler {
	key := strings.TrimSpace(internalAPIKey)
	source := PulseGetter(pulse.NewService(pulse.Config{}, nil, nil))
	if len(pulseSource) > 0 && pulseSource[0] != nil {
		source = pulseSource[0]
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/health", health)
	mux.HandleFunc("/ready", health)
	mux.HandleFunc("/api/v1/marketing/status", requireInternalKey(key, status))
	if planes != nil {
		mux.HandleFunc("/api/v1/marketing/plane-connections", requireInternalKey(key, planeclient.ConnectionsHandler(planes).ServeHTTP))
		mux.HandleFunc("/api/v1/marketing/assistant/drafts", requireInternalKey(key, planeclient.DraftHandler(planes).ServeHTTP))
	}
	mux.HandleFunc("/api/v1/marketing/pulse", requireInternalKey(key, bindWebsite(source, pulseHandler(source))))
	mux.HandleFunc("/api/v1/marketing/search", requireInternalKey(key, bindWebsite(source, searchHandler(source))))
	mux.HandleFunc("/api/v1/marketing/search/compare", requireInternalKey(key, bindWebsite(source, searchComparisonHandler(source))))
	mux.HandleFunc("/api/v1/marketing/pagespeed", requireInternalKey(key, bindWebsite(source, pageSpeedHandler(source))))
	registerWebsites(mux, key, source)
	registerAnalytics(mux, key, source)
	registerOutcomes(mux, key, source)
	registerWork(mux, key, source)
	registerDiscovery(mux, key, source)
	registerAudit(mux, key, source)
	registerVisibilityAnalysis(mux, key, source)
	registerCollection(mux, key, source)
	registerSearchResearch(mux, key, source)
	registerCompetitors(mux, key, source)
	registerMonitoring(mux, key, source)
	registerInsights(mux, key, source)
	registerSources(mux, key, source)
	registerPreparation(mux, key, source)
	registerAnalyticsSync(mux, key, source)
	registerPublicSignals(mux, key, source)
	registerTrends(mux, key, source)
	registerWebVitals(mux, key, source)
	registerSiteSources(mux, key, source)
	registerAlgoliaMapping(mux, key, source)
	mux.HandleFunc("/", landingPage)
	// The owner namespace is Visibility. Legacy Marketing routes remain aliases
	// for existing tools and saved client contracts; authentication is identical.
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/v1/visibility/") {
			if r.URL.Path == "/api/v1/visibility/outcomes" {
				http.NotFound(w, r)
				return
			}
			request := r.Clone(r.Context())
			request.URL.Path = "/api/v1/marketing/" + strings.TrimPrefix(r.URL.Path, "/api/v1/visibility/")
			request.URL.RawPath = ""
			mux.ServeHTTP(w, request)
			return
		}
		mux.ServeHTTP(w, r)
	})
}

func pulseHandler(source PulseGetter) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", http.MethodGet)
			writeJSON(w, http.StatusMethodNotAllowed, map[string]any{"error": map[string]string{"code": "method_not_allowed", "message": "GET is required"}})
			return
		}
		query := r.URL.Query()
		if len(query) > 2 || len(query["quarter"]) > 1 || len(query["site"]) != 1 {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "one site and at most one quarter parameter are required"}})
			return
		}
		for key := range query {
			if key != "site" && key != "quarter" {
				writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_query", "message": "unknown query parameter"}})
				return
			}
		}
		site, err := pulse.CanonicalSite(query.Get("site"))
		if err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_site", "message": "site must be a public HTTPS website origin"}})
			return
		}
		quarter := query.Get("quarter")
		if _, err := pulse.QuarterPeriod(quarter, time.Now()); err != nil {
			writeJSON(w, http.StatusBadRequest, map[string]any{"error": map[string]string{"code": "invalid_quarter", "message": "quarter must be a current or past quarter in this year"}})
			return
		}
		writeJSON(w, http.StatusOK, map[string]any{"data": source.Get(r.Context(), quarter, site)})
	}
}

func health(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok", "service": serviceName})
}

func status(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]any{"data": map[string]string{"status": "ready", "service": serviceName}})
}

func landingPage(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path != "/" {
		http.NotFound(w, r)
		return
	}
	if r.Method != http.MethodGet && r.Method != http.MethodHead {
		w.Header().Set("Allow", http.MethodGet+", "+http.MethodHead)
		w.WriteHeader(http.StatusMethodNotAllowed)
		return
	}
	setHTMLSafetyHeaders(w)
	w.Header().Set("Content-Type", "text/html; charset=utf-8")
	_, _ = w.Write([]byte(`<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Visibility Core</title>
  <style>body{font:16px system-ui,sans-serif;max-width:42rem;margin:4rem auto;padding:0 1.25rem;color:#172033}code{background:#edf2f7;padding:.15rem .3rem;border-radius:.25rem}</style>
</head>
<body>
  <main>
    <h1>Visibility Core</h1>
    <p>Marketing workspace projections and approved measurement workflows begin here.</p>
    <p>Service health: <code>/health</code> · readiness: <code>/ready</code></p>
  </main>
</body>
</html>`))
}

func requireInternalKey(expected string, next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		provided := strings.TrimSpace(r.Header.Get("X-Internal-API-Key"))
		if expected == "" || provided == "" || subtle.ConstantTimeCompare([]byte(expected), []byte(provided)) != 1 {
			writeJSON(w, http.StatusUnauthorized, map[string]any{"error": map[string]string{"code": "unauthorized", "message": "authentication required"}})
			return
		}
		next(w, r)
	}
}

func setHTMLSafetyHeaders(w http.ResponseWriter) {
	w.Header().Set("Content-Security-Policy", "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("X-Frame-Options", "DENY")
	w.Header().Set("Referrer-Policy", "no-referrer")
}

func writeJSON(w http.ResponseWriter, status int, body any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(body)
}
