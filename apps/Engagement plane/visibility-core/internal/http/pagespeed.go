package http

import (
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"io"
	"mime"
	"net/http"
	"net/url"
)

type PageSpeedSource interface {
	GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
	StartPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
}

func pageSpeedHandler(source PulseGetter) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var command struct {
			Site     string `json:"site"`
			Strategy string `json:"strategy"`
		}
		switch r.Method {
		case http.MethodGet:
			query, err := parsePageSpeedQuery(r)
			if err != nil {
				writePageSpeedError(w, 400, "invalid_query", "Choose one website and device")
				return
			}
			command.Site, command.Strategy = query[0], query[1]
		case http.MethodPost:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				writePageSpeedError(w, 415, "invalid_content_type", "JSON is required")
				return
			}
			decoder := json.NewDecoder(http.MaxBytesReader(w, r.Body, 2048))
			decoder.DisallowUnknownFields()
			if r.URL.RawQuery != "" || decoder.Decode(&command) != nil || decoder.Decode(new(any)) != io.EOF {
				writePageSpeedError(w, 400, "invalid_command", "Choose one website and device")
				return
			}
		default:
			w.Header().Set("Allow", "GET, POST")
			writePageSpeedError(w, 405, "method_not_allowed", "GET or POST is required")
			return
		}
		site, err := pulse.CanonicalSite(command.Site)
		if err != nil || site != command.Site || (command.Strategy != "mobile" && command.Strategy != "desktop") {
			writePageSpeedError(w, 400, "invalid_command", "Choose a public HTTPS website and device")
			return
		}
		projection := pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: command.Strategy, Status: "failed", Reason: pulse.ReasonNotConfigured}
		if service, ok := source.(PageSpeedSource); ok {
			if r.Method == http.MethodPost {
				projection = service.StartPageSpeed(r.Context(), site, command.Strategy)
			} else {
				projection = service.GetPageSpeed(r.Context(), site, command.Strategy)
			}
		}
		writeJSON(w, 200, map[string]any{"data": projection})
	}
}

func parsePageSpeedQuery(r *http.Request) ([2]string, error) {
	query, err := url.ParseQuery(r.URL.RawQuery)
	if err != nil || len(query) != 2 || len(query["site"]) != 1 || len(query["strategy"]) != 1 {
		return [2]string{}, io.ErrUnexpectedEOF
	}
	return [2]string{query.Get("site"), query.Get("strategy")}, nil
}

func writePageSpeedError(w http.ResponseWriter, status int, code, message string) {
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
