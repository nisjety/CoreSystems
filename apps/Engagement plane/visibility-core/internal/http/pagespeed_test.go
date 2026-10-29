package http

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"net/http/httptest"
	"strings"
	"testing"
)

type fakePageSpeed struct {
	fakePulse
	method, site, strategy string
}

func (f *fakePageSpeed) GetPageSpeed(_ context.Context, site, strategy string) pulse.PageSpeedProjection {
	f.method = "GET"
	f.site = site
	f.strategy = strategy
	return pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "idle"}
}
func (f *fakePageSpeed) StartPageSpeed(_ context.Context, site, strategy string) pulse.PageSpeedProjection {
	f.method = "POST"
	f.site = site
	f.strategy = strategy
	return pulse.PageSpeedProjection{Site: site, Source: "google_pagespeed", Strategy: strategy, Status: "running"}
}
func TestPageSpeedProtectedStrictCommandAndRead(t *testing.T) {
	source := &fakePageSpeed{}
	handler := NewHandler("test", source)
	for _, tc := range []struct {
		method, path, body, content, key string
		status                           int
	}{
		{"GET", "?site=https://aquatiq.com/&strategy=mobile", "", "", "", 401},
		{"GET", "?site=https://aquatiq.com/&strategy=mobile", "", "", "test", 200},
		{"POST", "", `{"site":"https://aquatiq.com/","strategy":"desktop"}`, "application/json", "test", 200},
		{"POST", "", `{"site":"https://aquatiq.com/","strategy":"desktop","extra":true}`, "application/json", "test", 400},
		{"POST", "", `{"site":"https://aquatiq.com/","strategy":"mobile"} {}`, "application/json", "test", 400},
		{"POST", "", `{"site":"https://aquatiq.com/","strategy":"mobile"}`, "text/plain", "test", 415},
		{"GET", "?site=https://localhost/&strategy=mobile", "", "", "test", 400},
		{"GET", "?site=https://aquatiq.com/&strategy=bad", "", "", "test", 400},
		{"GET", "?site=https://aquatiq.com/&strategy=mobile&strategy=desktop", "", "", "test", 400},
		{"GET", "?site=https://aquatiq.com/&strategy=mobile&operation=custom", "", "", "test", 400},
		{"POST", "?site=https://aquatiq.com/", `{}`, "application/json", "test", 400},
		{"DELETE", "", "", "", "test", 405},
	} {
		req := httptest.NewRequest(tc.method, "/api/v1/marketing/pagespeed"+tc.path, strings.NewReader(tc.body))
		req.Header.Set("X-Internal-API-Key", tc.key)
		req.Header.Set("Content-Type", tc.content)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != tc.status {
			t.Errorf("%s %s=%d %s", tc.method, tc.path, response.Code, response.Body.String())
		}
	}
	if source.method != "POST" || source.strategy != "desktop" || source.site != "https://aquatiq.com/" {
		t.Fatalf("source=%#v", source)
	}
}
