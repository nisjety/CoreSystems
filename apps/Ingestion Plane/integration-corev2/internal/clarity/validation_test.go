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

func TestClarityTrafficRejectsAmbiguousOrInvalidCountRepresentations(t *testing.T) {
	for _, raw := range []string{`"1.1"`, `"9007199254740990.9"`, `"9007199254740992"`, `"-1"`, `"NaN"`, `true`, `"1e100"`, `"1e-999999999"`} {
		if _, err := parseTraffic([]byte(`[{"metricName":"Traffic","information":[{"totalSessionCount":` + raw + `}]}]`)); err == nil {
			t.Errorf("invalidcountaccepted %s", raw)
		}
	}
	for _, data := range []string{`[]`, `[{"metricName":"Traffic","information":[]}]`, `[{"metricName":"Traffic","information":[{},{}]}]`, `[{"metricName":"Traffic","information":[{"distinctUserCount":"2","distantUserCount":"3"}]}]`, `[{"metricName":"Traffic","information":[{}]}]`} {
		if _, err := parseTraffic([]byte(data)); err == nil {
			t.Fatal("ambiguousTrafficaccepted")
		}
	}
	got, err := parseTraffic([]byte(`[{"metricName":"Traffic","information":[{"totalSessionCount":"0","distantUserCount":"2","PagesPerSessionPercentage":1.5}]}]`))
	if err != nil || got.Sessions == nil || *got.Sessions != 0 || *got.Users != 2 {
		t.Fatal("validdocumentedTrafficaliasesrejected")
	}
	if _, err := integer(map[string]json.RawMessage{"count": json.RawMessage(`"2.0"`)}, "count"); err != nil {
		t.Fatal(err)
	}
}

func TestClarityDoesNotFollowRedirectsOrReadOversizedProviderResponses(t *testing.T) {
	for _, tc := range []struct {
		status int
		body   string
	}{{302, ""}, {200, strings.Repeat(" ", maxResponseBytes+1)}} {
		client := &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: tc.status, Body: io.NopCloser(strings.NewReader(tc.body)), Header: http.Header{"Location": []string{"https://evil.example"}}}, nil
		})}
		service, err := NewService(bindingFixture(t), client, nil)
		if err != nil {
			t.Fatal(err)
		}
		if got := service.Get(t.Context(), "org-1", "https://aquatiq.com/", 3); got.Reason != "source_unavailable" {
			t.Fatal("unsafeprovidersourceaccepted")
		}
	}
}

func TestClarityTokenFilesCannotBeSymlinksOrWorldReadable(t *testing.T) {
	for _, mode := range []os.FileMode{0644, 0666} {
		path := bindingFixture(t)
		token := filepath.Join(filepath.Dir(path), "aquatiq.token")
		if err := os.Chmod(token, mode); err != nil {
			t.Fatal(err)
		}
		if _, err := NewService(path, nil, nil); err == nil {
			t.Fatal("insecurecredentialmodeaccepted")
		}
	}
	path := bindingFixture(t)
	token := filepath.Join(filepath.Dir(path), "aquatiq.token")
	moved := token + ".original"
	if err := os.Rename(token, moved); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(moved, token); err != nil {
		t.Fatal(err)
	}
	if _, err := NewService(path, nil, nil); err == nil {
		t.Fatal("symlinkcredentialaccepted")
	}
}
