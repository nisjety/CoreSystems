package http

import (
	"context"
	"net/http"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type GA4Getter interface {
	GetGA4(context.Context, string, string) pulse.GA4Projection
}
type ClarityGetter interface {
	GetClarity(context.Context, string) pulse.ClarityProjection
}

func registerAnalytics(mux *http.ServeMux, key string, source PulseGetter) {
	inner := bindWebsite(source, analyticsProjection(source))
	handler := requireInternalKey(key, websiteLimiter(analyticsReadBoundary(inner)))
	mux.HandleFunc("/api/v1/marketing/ga4", handler)
	mux.HandleFunc("/api/v1/marketing/clarity", handler)
}
func analyticsReadBoundary(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", http.MethodGet)
			websiteError(w, 405)
			return
		}
		query, err := strictQuery(r)
		if err != nil || len(query["websiteId"]) != 1 || !websiteIDPattern.MatchString(query.Get("websiteId")) || len(query["site"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
			websiteError(w, 400)
			return
		}
		ga4 := r.URL.Path == "/api/v1/marketing/ga4"
		for key := range query {
			if key != "websiteId" && key != "site" && !(ga4 && key == "quarter") {
				websiteError(w, 400)
				return
			}
		}
		canonical, err := pulse.CanonicalSite(query.Get("site"))
		if err != nil || canonical != query.Get("site") {
			websiteError(w, 400)
			return
		}
		if ga4 {
			if len(query["quarter"]) > 1 {
				websiteError(w, 400)
				return
			}
			if _, err := pulse.QuarterPeriod(query.Get("quarter"), time.Now()); err != nil {
				websiteError(w, 400)
				return
			}
		}
		next(w, r)
	}
}
func analyticsProjection(source PulseGetter) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		query := r.URL.Query()
		site := query.Get("site")
		if r.URL.Path == "/api/v1/marketing/ga4" {
			period, _ := pulse.QuarterPeriod(query.Get("quarter"), time.Now())
			value := pulse.UnavailableGA4(site, period, "not_configured")
			if provider, ok := source.(GA4Getter); ok {
				value = provider.GetGA4(r.Context(), query.Get("quarter"), site)
			}
			writeJSON(w, 200, map[string]any{"data": value})
			return
		}
		value := pulse.UnavailableClarity(site, "not_configured")
		if provider, ok := source.(ClarityGetter); ok {
			value = provider.GetClarity(r.Context(), site)
		}
		writeJSON(w, 200, map[string]any{"data": value})
	}
}
