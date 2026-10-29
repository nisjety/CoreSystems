package datex

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestPullSituationUsesRegisteredBasicAuthAndPreservesXML(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		username, password, ok := r.BasicAuth()
		if !ok || username != "datex-user" || password != "datex-pass" {
			t.Fatalf("basic auth = %q/%q/%v", username, password, ok)
		}
		w.Header().Set("Content-Type", "application/xml")
		_, _ = w.Write([]byte(`<d2LogicalModel><payloadPublication><publicationTime>2026-07-21T10:00:00Z</publicationTime></payloadPublication></d2LogicalModel>`))
	}))
	defer server.Close()

	service := NewService(server.Client(), server.URL, "datex-user", "datex-pass")
	response, err := service.PullSituation(context.Background())
	if err != nil {
		t.Fatalf("PullSituation() error = %v", err)
	}
	if response.Source.Provider != "statens-vegvesen" || response.Source.APIVersion != "3.1" {
		t.Fatalf("source = %+v", response.Source)
	}
	if response.Data == "" || response.ContentType != "application/xml" {
		t.Fatalf("response = %+v", response)
	}
}

func TestPullSituationFailsClosedWithoutRegistration(t *testing.T) {
	service := NewService(http.DefaultClient, "https://example.test/datex", "", "")
	if _, err := service.PullSituation(context.Background()); err != ErrNotConfigured {
		t.Fatalf("error = %v, want ErrNotConfigured", err)
	}
}

func TestPullSituationRejectsInvalidXML(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`<not-xml`))
	}))
	defer server.Close()

	service := NewService(server.Client(), server.URL, "user", "pass")
	if _, err := service.PullSituation(context.Background()); err == nil {
		t.Fatal("PullSituation() error = nil for malformed XML")
	}
}
