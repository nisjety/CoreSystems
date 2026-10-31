package orgcore

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

const testToken = "0123456789abcdef0123456789abcdef"

// org returns a client bound to org-1, reading from server.
func org(t *testing.T, server *httptest.Server) *Client {
	t.Helper()
	client, err := NewClient(Config{URL: server.URL, Token: testToken, OrganizationID: "org-1"}, server.Client())
	if err != nil || client == nil {
		t.Fatalf("NewClient: %v", err)
	}
	return client
}

func TestPrimarySiteReadsTheWebsiteAsTheServiceIdentity(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/organizations/org-1/website" || r.Method != http.MethodGet {
			t.Errorf("request = %s %s", r.Method, r.URL.Path)
		}
		if r.Header.Get("X-Service-Id") != "visibility-core" || r.Header.Get("X-Service-Token") != testToken {
			t.Errorf("service headers = %q / %q", r.Header.Get("X-Service-Id"), r.Header.Get("X-Service-Token"))
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"website_url":"https://triodelab.no/","website_updated_at":null}`))
	}))
	defer server.Close()

	site, err := org(t, server).PrimarySite(context.Background(), "org-1")
	if err != nil || site != "https://triodelab.no/" {
		t.Fatalf("PrimarySite = %q, %v", site, err)
	}
}

func TestAnOrganisationWithNoWebsiteHasNoPrimarySite(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"website_url":null}`))
	}))
	defer server.Close()

	if site, err := org(t, server).PrimarySite(context.Background(), "org-1"); err != nil || site != "" {
		t.Errorf("PrimarySite = %q, %v; want no site and no error", site, err)
	}
}

func TestOrgCoreStatusesBecomeTheRightError(t *testing.T) {
	t.Parallel()

	cases := map[int]error{
		http.StatusForbidden:           ErrRefused,
		http.StatusUnauthorized:        ErrRefused,
		http.StatusInternalServerError: ErrUnavailable,
	}
	for status, want := range cases {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
			w.WriteHeader(status)
		}))
		_, err := org(t, server).PrimarySite(context.Background(), "org-1")
		server.Close()
		if !errors.Is(err, want) {
			t.Errorf("status %d: err = %v, want %v", status, err, want)
		}
	}
}

func TestTheClientOnlyReadsItsOwnOrganisation(t *testing.T) {
	t.Parallel()

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		t.Error("a request left for another organisation")
	}))
	defer server.Close()

	if _, err := org(t, server).PrimarySite(context.Background(), "org-2"); !errors.Is(err, ErrWrongOrganization) {
		t.Errorf("err = %v, want ErrWrongOrganization", err)
	}
}

func TestNewClientRefusesAPartialOrUnsafeConfiguration(t *testing.T) {
	t.Parallel()

	refused := []Config{
		{URL: "http://firma.example.com", Token: testToken, OrganizationID: "org-1"},
		{URL: "https://org-core.example.com/extra", Token: testToken, OrganizationID: "org-1"},
		{URL: "https://org-core.example.com", Token: "short", OrganizationID: "org-1"},
		{URL: "https://org-core.example.com", Token: testToken},
		{Token: testToken, OrganizationID: "org-1"},
	}
	for _, cfg := range refused {
		if client, err := NewClient(cfg, nil); err == nil || client != nil {
			t.Errorf("NewClient(%+v) accepted an unsafe configuration", cfg)
		}
	}
	if client, err := NewClient(Config{}, nil); err != nil || client != nil {
		t.Errorf("an unconfigured org-core = %v, %v; want nil and no error", client, err)
	}
}
