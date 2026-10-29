package pulse

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

const pageSpeedComplete = `{"site":"https://aquatiq.com/","source":"google_pagespeed","strategy":"mobile","status":"complete","requestedAt":"2026-10-21T00:00:00Z","completedAt":"2026-10-21T00:01:00Z","report":{"performance":87,"accessibility":null,"bestPractices":100,"seo":92,"lcpMs":2300,"cls":0,"tbtMs":12,"fcpMs":1000,"speedIndexMs":2000,"fetchTime":"2026-10-21T00:00:45Z","lighthouseVersion":"12.8.2"}}`

func TestPageSpeedUsesOnlyFixedIntegrationRoute(t *testing.T) {
	for _, method := range []string{http.MethodGet, http.MethodPost} {
		t.Run(method, func(t *testing.T) {
			var calls []string
			provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Header.Get("Authorization") != "Bearer scoped-test-token" {
					t.Error("missing org bearer")
				}
				calls = append(calls, r.Method+" "+r.URL.Path)
				if strings.HasSuffix(r.URL.Path, "/actions") {
					_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"}]}}}}`))
					return
				}
				if r.URL.Path != "/api/v1/insights/pagespeed" || r.Method != method {
					t.Error("unexpected integration request")
				}
				if method == http.MethodGet {
					if r.URL.Query().Get("site") != "https://aquatiq.com/" || r.URL.Query().Get("strategy") != "mobile" {
						t.Error("bad query")
					}
				} else {
					var body map[string]string
					_ = json.NewDecoder(r.Body).Decode(&body)
					if body["site"] != "https://aquatiq.com/" || body["strategy"] != "mobile" || len(body) != 2 {
						t.Error("bad command")
					}
				}
				_, _ = w.Write([]byte(`{"data":` + pageSpeedComplete + `}`))
			}))
			defer provider.Close()
			svc := newTestService(t, provider.URL, "")
			var got PageSpeedProjection
			if method == http.MethodPost {
				got = svc.StartPageSpeed(context.Background(), "https://aquatiq.com/", "mobile")
			} else {
				got = svc.GetPageSpeed(context.Background(), "https://aquatiq.com/", "mobile")
			}
			if got.Status != "complete" || got.Report == nil || got.Report.Performance == nil || *got.Report.Performance != 87 || got.Report.Accessibility != nil || got.Report.CLS == nil || *got.Report.CLS != 0 || len(calls) != 1 {
				t.Fatalf("projection=%#v calls=%v", got, calls)
			}
		})
	}
}

func TestPageSpeedRejectsInvalidProviderProjectionAndKeepsFailureHonest(t *testing.T) {
	for _, body := range []string{
		strings.Replace(pageSpeedComplete, `"performance":87`, `"performance":101`, 1),
		strings.Replace(pageSpeedComplete, `"cls":0`, `"cls":-1`, 1),
		strings.Replace(pageSpeedComplete, `"status":"complete"`, `"status":"idle"`, 1),
		strings.Replace(pageSpeedComplete, `"strategy":"mobile"`, `"strategy":"desktop"`, 1),
		strings.Replace(pageSpeedComplete, `"site":"https://aquatiq.com/"`, `"site":"https://other.com/"`, 1),
		strings.Replace(pageSpeedComplete, `"fetchTime":"2026-10-21T00:00:45Z"`, `"fetchTime":"invalid"`, 1),
		`{"status":"running"}`, strings.Repeat("x", 32769),
	} {
		provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if strings.HasSuffix(r.URL.Path, "/actions") {
				_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[{"siteUrl":"sc-domain:aquatiq.com","permissionLevel":"siteOwner"}]}}}}`))
				return
			}
			_, _ = w.Write([]byte(`{"data":` + body + `}`))
		}))
		got := newTestService(t, provider.URL, "").GetPageSpeed(context.Background(), "https://aquatiq.com/", "mobile")
		provider.Close()
		if got.Status != "failed" || got.Reason != ReasonSourceUnavailable || got.Report != nil {
			t.Fatalf("unsafe projection=%#v", got)
		}
	}
	for _, reason := range []string{"quota_limited", "not_configured", "source_unavailable", "audit_failed"} {
		value := PageSpeedProjection{Site: "https://aquatiq.com/", Source: "google_pagespeed", Strategy: "mobile", Status: "failed", Reason: reason}
		if !validPageSpeedProjection(value) {
			t.Errorf("reason rejected: %s", reason)
		}
	}
}

func TestPageSpeedDoesNotStartForUnconfiguredOrInvalidSite(t *testing.T) {
	svc := NewService(Config{}, nil, nil)
	for _, strategy := range []string{"mobile", "desktop", "unexpected"} {
		got := svc.StartPageSpeed(context.Background(), "https://aquatiq.com/", strategy)
		if got.Status != "failed" || got.Report != nil {
			t.Fatalf("projection=%#v", got)
		}
	}
	var calls int
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls++
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"providerKey":"google","operation":"gsc.sites.list","result":{"siteEntry":[]}}}}`))
	}))
	defer provider.Close()
	got := newTestService(t, provider.URL, "").StartPageSpeed(context.Background(), "https://aquatiq.com/", "desktop")
	if calls != 1 || got.Status != "failed" || got.Report != nil {
		t.Fatalf("projection=%#v calls=%d", got, calls)
	}
}

func TestPublicPageSpeedWorksForOtherSiteWithoutSearchConsoleConnection(t *testing.T) {
	provider := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/insights/pagespeed" {
			t.Error("public diagnostics must not call Search Console")
			w.WriteHeader(503)
			return
		}
		_, _ = w.Write([]byte(`{"data":{"site":"https://example.com/","source":"google_pagespeed","strategy":"desktop","status":"running"}}`))
	}))
	defer provider.Close()
	svc := newTestService(t, provider.URL, "")
	svc.cfg.GoogleConnectionID = ""
	got := svc.StartPageSpeed(context.Background(), "https://example.com/", "desktop")
	if got.Status != "running" || got.Site != "https://example.com/" {
		t.Fatalf("projection=%#v", got)
	}
}
