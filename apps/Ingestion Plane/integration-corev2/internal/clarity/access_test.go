package clarity

import (
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func rewriteManifest(t *testing.T, path, old, replacement string) {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(strings.ReplaceAll(string(data), old, replacement)), 0600); err != nil {
		t.Fatal(err)
	}
}

func TestBindingAccessIsFreshScopedAndProviderFree(t *testing.T) {
	path := bindingFixture(t)
	client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		t.Fatal("metadata must not call provider")
		return nil, nil
	})}
	s, err := NewService(path, client, nil)
	if err != nil {
		t.Fatal(err)
	}
	access, err := s.Access("org-1", "https://aquatiq.com/")
	if err != nil || !access.Configured || access.ProjectID != "z1abticqmb" || len(access.Version) != 64 {
		t.Fatalf("access=%+v err=%v", access, err)
	}
	encoded, _ := json.Marshal(access)
	if strings.Contains(string(encoded), "token") || strings.Contains(string(encoded), path) {
		t.Fatal("secret metadata leaked")
	}
	alias, err := s.Access("org-1", "https://www.aquatiq.com/")
	if err != nil || !alias.Configured || alias.ProjectID != access.ProjectID || alias.Version != access.Version {
		t.Fatal("same-organization www alias not resolved")
	}
	for _, scope := range []struct{ org, site string }{{"org-other", "https://aquatiq.com/"}, {"org-1", "https://blog.aquatiq.com/"}} {
		got, err := s.Access(scope.org, scope.site)
		if err != nil || got.Configured || got.Version != "" || got.ProjectID != "" {
			t.Fatal("binding crossed scope")
		}
	}
	rewriteManifest(t, path, "2026-10-21T00:00:00Z", "2026-10-22T00:00:00Z")
	next, err := s.Access("org-1", "https://aquatiq.com/")
	if err != nil || next.Version == access.Version {
		t.Fatal("live manifest changes ignored")
	}
	if err := os.WriteFile(path, []byte(`{"version":1,"bindings":[]}`), 0600); err != nil {
		t.Fatal(err)
	}
	got, err := s.Access("org-1", "https://aquatiq.com/")
	if err != nil || got.Configured {
		t.Fatal("revoked binding still available")
	}
}

func TestBindingAccessFailsClosedForInvalidOrRemovedCredential(t *testing.T) {
	for _, change := range []string{"removed", "unprotected", "badmanifest"} {
		t.Run(change, func(t *testing.T) {
			path := bindingFixture(t)
			s, err := NewService(path, nil, nil)
			if err != nil {
				t.Fatal(err)
			}
			token := filepath.Join(filepath.Dir(path), "aquatiq.token")
			switch change {
			case "removed":
				err = os.Remove(token)
			case "unprotected":
				err = os.Chmod(token, 0644)
			case "badmanifest":
				err = os.WriteFile(path, []byte(`{"version":2}`), 0600)
			}
			if err != nil {
				t.Fatal(err)
			}
			if _, err := s.Access("org-1", "https://aquatiq.com/"); err == nil {
				t.Fatal("invalid live configuration accepted")
			}
			if got := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3); got.Metrics != nil || got.Reason != "source_unavailable" {
				t.Fatal("invalid config did not fail closed")
			}
		})
	}
	s, _ := NewService("", nil, nil)
	if got, err := s.Access("org-1", "https://aquatiq.com/"); err != nil || got.Configured {
		t.Fatal("empty configuration")
	}
	if _, err := s.Access("", "https://aquatiq.com/"); err == nil {
		t.Fatal("empty organization accepted")
	}
	if _, err := s.Access("org-1", "https://127.0.0.1/"); err == nil {
		t.Fatal("private site accepted")
	}
}

func TestLiveBindingChangesInvalidateCachedExports(t *testing.T) {
	path := bindingFixture(t)
	calls := 0
	client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		calls++
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(trafficFixture)), Header: http.Header{}}, nil
	})}
	s, _ := NewService(path, client, nil)
	first := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
	if first.Metrics == nil || first.BindingVersion == "" {
		t.Fatal("configured snapshot lacks authority version")
	}
	rewriteManifest(t, path, "2026-10-21T00:00:00Z", "2026-10-22T00:00:00Z")
	next := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
	if calls != 2 || next.BindingVersion == first.BindingVersion {
		t.Fatal("reconfirmation reused old export")
	}
	if err := os.WriteFile(filepath.Join(filepath.Dir(path), "aquatiq.token"), []byte("rotated.jwt.signature"), 0600); err != nil {
		t.Fatal(err)
	}
	rotated := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
	if calls != 3 || rotated.BindingVersion != next.BindingVersion {
		t.Fatal("credential rotation must refetch without public secret-derived version")
	}
	if err := os.WriteFile(path, []byte(`{"version":1,"bindings":[]}`), 0600); err != nil {
		t.Fatal(err)
	}
	if got := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3); got.Metrics != nil || got.Reason != "not_configured" {
		t.Fatal("revocation returned cached export")
	}
}

func TestBindingRevocationDuringExportCannotReturnMetrics(t *testing.T) {
	path := bindingFixture(t)
	entered, release := make(chan struct{}), make(chan struct{})
	client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		close(entered)
		<-release
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(trafficFixture)), Header: http.Header{}}, nil
	})}
	s, _ := NewService(path, client, nil)
	result := make(chan Snapshot, 1)
	go func() { result <- s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3) }()
	<-entered
	if err := os.WriteFile(path, []byte(`{"version":1,"bindings":[]}`), 0600); err != nil {
		t.Fatal(err)
	}
	close(release)
	if got := <-result; got.Metrics != nil || got.Reason != "not_configured" || got.BindingVersion != "" || got.ProjectID != "" {
		t.Fatal("inflight export survived revoked authority")
	}
}

func TestLiveBindingReassignmentDoesNotCarryOldOrgOrProjectAuthority(t *testing.T) {
	path := bindingFixture(t)
	calls := 0
	client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		calls++
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(trafficFixture)), Header: http.Header{}}, nil
	})}
	s, _ := NewService(path, client, nil)
	first := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3)
	rewriteManifest(t, path, `"organizationId":"org-1"`, `"organizationId":"org-2"`)
	old, err := s.Access("org-1", "https://aquatiq.com/")
	if err != nil || old.Configured {
		t.Fatal("old organization retained authority")
	}
	if got := s.Get(t.Context(), "org-1", "https://aquatiq.com/", 3); got.Metrics != nil || got.Reason != "not_configured" {
		t.Fatal("old organization received cached export")
	}
	access, err := s.Access("org-2", "https://aquatiq.com/")
	if err != nil || !access.Configured || access.Version == first.BindingVersion {
		t.Fatal("new organization lacks distinct authority")
	}
	next := s.Get(t.Context(), "org-2", "https://aquatiq.com/", 3)
	if calls != 2 || next.BindingVersion != access.Version {
		t.Fatal("new organization reused old export")
	}
	rewriteManifest(t, path, `"projectId":"z1abticqmb"`, `"projectId":"replacement-project"`)
	next = s.Get(t.Context(), "org-2", "https://aquatiq.com/", 3)
	if calls != 3 || next.ProjectID != "replacement-project" || next.BindingVersion == access.Version {
		t.Fatal("project rebind reused former project evidence")
	}
}
