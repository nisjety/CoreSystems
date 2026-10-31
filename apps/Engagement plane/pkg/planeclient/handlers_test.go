package planeclient

import (
	"net/http/httptest"
	"strings"
	"testing"
)

// The owner core must mount these handlers behind its existing authentication.
func TestPlaneHandlersAreDraftOnlyStrictAndUnavailableWithoutCredentials(t *testing.T) {
	c, err := New(Config{Core: "marketing-core"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	read := ConnectionsHandler(c)
	w := httptest.NewRecorder()
	read.ServeHTTP(w, httptest.NewRequest("GET", "/connections", nil))
	if w.Code != 200 || strings.Contains(w.Body.String(), "token") {
		t.Fatalf("read %d %s", w.Code, w.Body.String())
	}
	draft := DraftHandler(c)
	for _, tc := range []struct {
		method, body, media string
		status              int
	}{{"GET", "", "", 405}, {"POST", `{"prompt":"Draft a title"}`, "text/plain", 415}, {"POST", `{"prompt":"Draft a title","orgId":"foreign"}`, "application/json", 400}, {"POST", `{"prompt":"a","prompt":"b"}`, "application/json", 400}, {"POST", `{"prompt":""}`, "application/json", 400}, {"POST", `{"prompt":"Draft a title","tools":["publish"]}`, "application/json", 400}, {"POST", `{"prompt":"Draft a title"}`, "application/json", 503}} {
		r := httptest.NewRequest(tc.method, "/drafts", strings.NewReader(tc.body))
		r.Header.Set("Content-Type", tc.media)
		w := httptest.NewRecorder()
		draft.ServeHTTP(w, r)
		if w.Code != tc.status {
			t.Fatalf("%s %s=%d expected%d %s", tc.method, tc.body, w.Code, tc.status, w.Body.String())
		}
	}
}
