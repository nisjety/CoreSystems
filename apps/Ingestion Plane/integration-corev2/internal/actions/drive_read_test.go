package actions

import (
	"context"
	"crypto/tls"
	"encoding/base64"
	"encoding/json"
	"net"
	"net/http"
	"net/http/httptest"
	neturl "net/url"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"testing"

	"github.com/triodelab/integration-corev2/internal/config"
	"github.com/triodelab/integration-corev2/internal/store"
)

// F4: files and SharePoint for the connected user. These tests pin the
// provider-facing request each parameter combination produces, for Microsoft
// Graph (OneDrive, SharePoint document libraries) and Google Drive (My Drive and
// shared drives). Before F4 there was no direct test of the Microsoft
// drive.files executor at all.

type driveCall struct {
	Method      string
	Path        string
	EscapedPath string
	RawQuery    string
	Query       neturl.Values
	Header      http.Header
}

type driveRecorder struct {
	mu    sync.Mutex
	calls []driveCall
}

func (r *driveRecorder) record(req *http.Request) driveCall {
	call := driveCall{
		Method:      req.Method,
		Path:        req.URL.Path,
		EscapedPath: req.URL.EscapedPath(),
		RawQuery:    req.URL.RawQuery,
		Query:       req.URL.Query(),
		Header:      req.Header.Clone(),
	}
	r.mu.Lock()
	r.calls = append(r.calls, call)
	r.mu.Unlock()
	return call
}

func (r *driveRecorder) all() []driveCall {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]driveCall(nil), r.calls...)
}

func (r *driveRecorder) only(t *testing.T) driveCall {
	t.Helper()
	calls := r.all()
	if len(calls) != 1 {
		t.Fatalf("provider received %d requests, want exactly 1: %+v", len(calls), calls)
	}
	return calls[0]
}

func driveJSON(w http.ResponseWriter, value any) {
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(value)
}

// newDriveGraphService serves Graph over TLS: the page-token validator only
// accepts https links on the configured Graph host.
func newDriveGraphService(t *testing.T, handler http.HandlerFunc) (*Service, *httptest.Server) {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	return NewService(config.Config{MicrosoftGraphBaseURL: server.URL}, server.Client()), server
}

func newDriveGoogleService(t *testing.T, handler http.HandlerFunc) *Service {
	t.Helper()
	server := httptest.NewServer(handler)
	t.Cleanup(server.Close)
	return NewService(config.Config{GoogleAPIBaseURL: server.URL}, server.Client())
}

// newNamedHostsService answers for every host name from one TLS test server:
// the client dials the test server whatever host a URL names. A redirect to a
// real *.sharepoint.com name is then followed (or refused) by the production
// allow-list, not by one a test had to widen first.
func newNamedHostsService(t *testing.T, handler http.HandlerFunc) *Service {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	transport := server.Client().Transport.(*http.Transport).Clone()
	address := server.Listener.Addr().String()
	transport.DialContext = func(ctx context.Context, network, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, network, address)
	}
	// The test certificate names example.com and 127.0.0.1, not the hosts above.
	transport.TLSClientConfig = &tls.Config{InsecureSkipVerify: true}
	return NewService(config.Config{MicrosoftGraphBaseURL: "https://graph.microsoft.com"}, &http.Client{Transport: transport})
}

func runDriveOp(service *Service, provider, operation string, params map[string]any) (map[string]any, error) {
	result, err := service.Execute(context.Background(), ExecuteInput{
		Connection: store.Connection{ProviderKey: provider}, AccessToken: "token-1",
		Operation: operation, Params: params,
	})
	if err != nil {
		return nil, err
	}
	payload, _ := result.Result.(map[string]any)
	return payload, nil
}

func mustRunDriveOp(t *testing.T, service *Service, provider, operation string, params map[string]any) map[string]any {
	t.Helper()
	payload, err := runDriveOp(service, provider, operation, params)
	if err != nil {
		t.Fatalf("%s %s error: %v", provider, operation, err)
	}
	return payload
}

func driveSelectFields(t *testing.T, call driveCall) []string {
	t.Helper()
	return strings.Split(call.Query.Get("$select"), ",")
}

func driveContains(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}

// --- Microsoft: drive.files ----------------------------------------------------

func TestMicrosoftDriveFilesDefaultListsTheOwnDriveRoot(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{map[string]any{"id": "01A", "name": "Budget.xlsx"}}})
	})
	for _, operation := range []string{"drive.files", "microsoft.drive.files"} {
		payload := mustRunDriveOp(t, service, "microsoft", operation, nil)
		if rows, _ := payload["value"].([]any); len(rows) != 1 {
			t.Fatalf("%s value = %#v, want the Graph rows passed through", operation, payload["value"])
		}
		for _, key := range []string{"nextPageToken", "moreAvailable"} {
			if _, present := payload[key]; present {
				t.Fatalf("%s: %s must be absent on a last page: %#v", operation, key, payload)
			}
		}
	}
	for _, call := range rec.all() {
		if call.Method != http.MethodGet || call.Path != "/v1.0/me/drive/root/children" {
			t.Fatalf("request = %s %s, want GET /v1.0/me/drive/root/children", call.Method, call.Path)
		}
		if got := call.Query.Get("$top"); got != "10" {
			t.Fatalf("$top = %q, want the default 10", got)
		}
		fields := driveSelectFields(t, call)
		for _, field := range []string{"id", "name", "webUrl", "file", "folder", "lastModifiedDateTime", "size", "parentReference"} {
			if !driveContains(fields, field) {
				t.Fatalf("$select = %q, missing %s", call.Query.Get("$select"), field)
			}
		}
		if got := call.Header.Get("Authorization"); got != "Bearer token-1" {
			t.Fatalf("Authorization = %q, want the connection's bearer", got)
		}
	}
}

func TestMicrosoftDriveFilesMaxResultsIsClampedToTheCap(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"maxResults": float64(500)})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"maxResults": "3"})
	calls := rec.all()
	if len(calls) != 2 || calls[0].Query.Get("$top") != "100" || calls[1].Query.Get("$top") != "3" {
		t.Fatalf("$top values = %+v, want 100 then 3", calls)
	}
}

func TestMicrosoftDriveFilesQueryUsesTheDriveSearchEndpoint(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"query": "quarterly report", "maxResults": float64(25)})
	call := rec.only(t)
	if call.Path != "/v1.0/me/drive/root/search(q='quarterly report')" {
		t.Fatalf("path = %q, want the drive search endpoint", call.Path)
	}
	if call.Query.Get("$top") != "25" {
		t.Fatalf("$top = %q, want 25", call.Query.Get("$top"))
	}
}

func TestMicrosoftDriveFilesSearchDoublesSingleQuotes(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"query": "Ola's plan"})
	if call := rec.only(t); call.Path != "/v1.0/me/drive/root/search(q='Ola''s plan')" {
		t.Fatalf("path = %q, want the single quote doubled inside the OData literal", call.Path)
	}
}

func TestMicrosoftDriveFilesSearchTextCannotLeaveTheSearchSegment(t *testing.T) {
	// The text sits inside the URL path. A question mark, a hash or a percent
	// sign there used to cut the request short (or fail to parse), and a slash
	// started another path segment.
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"query": "50% off? #1 a/b+c"})
	call := rec.only(t)
	if call.Path != "/v1.0/me/drive/root/search(q='50% off? #1 a/b+c')" {
		t.Fatalf("decoded path = %q, want the whole text inside the search literal", call.Path)
	}
	for _, escaped := range []string{"%25", "%3F", "%23", "%2F"} {
		if !strings.Contains(call.EscapedPath, escaped) {
			t.Fatalf("escaped path = %q, want %s so the text cannot end the segment", call.EscapedPath, escaped)
		}
	}
	var keys []string
	for key := range call.Query {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	if strings.Join(keys, ",") != "$select,$top" {
		t.Fatalf("query parameters = %v, want only $select and $top (the text must not leak into the query string)", keys)
	}
}

func TestMicrosoftDriveFilesFolderIdListsThatFolder(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"folderId": "01FOLDER"})
	if call := rec.only(t); call.Path != "/v1.0/me/drive/items/01FOLDER/children" {
		t.Fatalf("path = %q, want the folder's children", call.Path)
	}
}

func TestMicrosoftDriveFilesQueryTakesPriorityOverFolderId(t *testing.T) {
	// Graph's search endpoint already searches the whole drive; v1.0 has no way
	// to combine it with a folder-scoped listing, so the search wins.
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"query": "plan", "folderId": "01FOLDER"})
	if call := rec.only(t); call.Path != "/v1.0/me/drive/root/search(q='plan')" {
		t.Fatalf("path = %q, want the search endpoint", call.Path)
	}
}

func TestMicrosoftDriveFilesFolderIdIsPathEscaped(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"folderId": "a/b c?x"})
	call := rec.only(t)
	if call.EscapedPath != "/v1.0/me/drive/items/a%2Fb%20c%3Fx/children" {
		t.Fatalf("escaped path = %q, want the id kept inside one segment", call.EscapedPath)
	}
}

func TestMicrosoftDriveFilesDriveIdReadsASharePointLibrary(t *testing.T) {
	tests := []struct {
		name        string
		params      map[string]any
		wantPath    string
		wantEscaped string
	}{
		{
			name:        "library root",
			params:      map[string]any{"driveId": "b!abc-DEF_123"},
			wantPath:    "/v1.0/drives/b!abc-DEF_123/root/children",
			wantEscaped: "/v1.0/drives/b%21abc-DEF_123/root/children",
		},
		{
			name:        "folder in the library",
			params:      map[string]any{"driveId": "b!abc-DEF_123", "folderId": "01FOLDER"},
			wantPath:    "/v1.0/drives/b!abc-DEF_123/items/01FOLDER/children",
			wantEscaped: "/v1.0/drives/b%21abc-DEF_123/items/01FOLDER/children",
		},
		{
			name:        "search inside the library",
			params:      map[string]any{"driveId": "b!abc-DEF_123", "query": "offer"},
			wantPath:    "/v1.0/drives/b!abc-DEF_123/root/search(q='offer')",
			wantEscaped: "/v1.0/drives/b%21abc-DEF_123/root/search(q='offer')",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var rec driveRecorder
			service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				driveJSON(w, map[string]any{"value": []any{}})
			})
			mustRunDriveOp(t, service, "microsoft", "drive.files", tt.params)
			call := rec.only(t)
			if call.Path != tt.wantPath || call.EscapedPath != tt.wantEscaped {
				t.Fatalf("path = %q (escaped %q), want %q (escaped %q)", call.Path, call.EscapedPath, tt.wantPath, tt.wantEscaped)
			}
			if call.Query.Get("$top") != "10" {
				t.Fatalf("$top = %q, want 10", call.Query.Get("$top"))
			}
		})
	}
}

// --- Microsoft: drive.files paging ----------------------------------------------

func TestMicrosoftDriveFilesNextLinkIsReturnedAsPageTokenAndRoundTripsUnchanged(t *testing.T) {
	var rec driveRecorder
	var serverURL string
	service, server := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Query().Get("$skiptoken") == "" {
			driveJSON(w, map[string]any{
				"value":           []any{map[string]any{"id": "f1"}},
				"@odata.nextLink": serverURL + "/v1.0/drives/b!abc/root/children?$top=2&$skiptoken=Page%202",
			})
			return
		}
		driveJSON(w, map[string]any{"value": []any{map[string]any{"id": "f2"}}})
	})
	serverURL = server.URL

	first := mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{"driveId": "b!abc", "maxResults": float64(2)})
	token, _ := first["nextPageToken"].(string)
	wantToken := server.URL + "/v1.0/drives/b!abc/root/children?$top=2&$skiptoken=Page%202"
	if token != wantToken {
		t.Fatalf("nextPageToken = %q, want the nextLink unchanged %q", token, wantToken)
	}
	if _, present := first["moreAvailable"]; present {
		t.Fatalf("moreAvailable must not be set when a token continues the list: %#v", first)
	}

	// The page request ignores every other param and replays the link as is.
	second := mustRunDriveOp(t, service, "microsoft", "drive.files", map[string]any{
		"pageToken": token, "query": "ignored", "driveId": "other", "maxResults": float64(50),
	})
	calls := rec.all()
	if len(calls) != 2 {
		t.Fatalf("requests = %d, want 2", len(calls))
	}
	if calls[1].Path != "/v1.0/drives/b!abc/root/children" || calls[1].RawQuery != "$top=2&$skiptoken=Page%202" {
		t.Fatalf("page request = %s?%s, want the nextLink replayed", calls[1].Path, calls[1].RawQuery)
	}
	if got := calls[1].Header.Get("Authorization"); got != "Bearer token-1" {
		t.Fatalf("page request Authorization = %q", got)
	}
	if rows, _ := second["value"].([]any); len(rows) != 1 {
		t.Fatalf("second page value = %#v", second["value"])
	}
	if _, present := second["nextPageToken"]; present {
		t.Fatalf("the last page must not carry a nextPageToken: %#v", second)
	}
}

func TestMicrosoftDriveFilesAcceptsTheLinkShapesGraphHandsOut(t *testing.T) {
	var rec driveRecorder
	service, server := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	accepted := []string{
		"/v1.0/me/drive/root/children?$skiptoken=x",
		"/v1.0/me/drive/items/01ABC/children?$skiptoken=x",
		"/v1.0/me/drive/root/search(q='quarterly%20report')?$top=10&$skiptoken=x",
		"/v1.0/me/drive/root/search(q='a%2Fb')?$skiptoken=x",
		"/v1.0/me/drive/root/microsoft.graph.search(q='plan')?$skiptoken=x",
		"/v1.0/drives/b!abc/root/children?$skiptoken=x",
		"/v1.0/drives/b%21abc/items/01ABC/children?$skiptoken=x",
		"/v1.0/drives('b!abc')/items('01ABC')/children?$skiptoken=x",
		"/v1.0/drives/b!abc/root/search(q='offer')?$skiptoken=x",
	}
	for _, path := range accepted {
		if _, err := runDriveOp(service, "microsoft", "drive.files", map[string]any{"pageToken": server.URL + path}); err != nil {
			t.Fatalf("pageToken %q was refused: %v", path, err)
		}
	}
	calls := rec.all()
	if len(calls) != len(accepted) {
		t.Fatalf("provider requests = %d, want %d", len(calls), len(accepted))
	}
	for index, path := range accepted {
		// Each link is replayed as it came, not rebuilt from other parameters.
		if got := calls[index].EscapedPath + "?" + calls[index].RawQuery; got != path {
			t.Fatalf("request %d = %q, want the link %q replayed", index, got, path)
		}
	}
}

func TestMicrosoftDriveFilesRejectsPageTokensThatCouldCarryTheBearerElsewhere(t *testing.T) {
	var hits atomic.Int32
	var foreignHits atomic.Int32
	foreign := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		foreignHits.Add(1)
		driveJSON(w, map[string]any{"value": []any{}})
	}))
	t.Cleanup(foreign.Close)
	service, server := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	host := strings.TrimPrefix(server.URL, "https://")

	rejected := map[string]string{
		"plain http":                     "http://" + host + "/v1.0/me/drive/root/children?$skiptoken=x",
		"user info before the host":      "https://evil@" + host + "/v1.0/me/drive/root/children?$skiptoken=x",
		"another host entirely":          foreign.URL + "/v1.0/me/drive/root/children?$skiptoken=x",
		"a lookalike host":               "https://graph.microsoft.com.evil.example/v1.0/me/drive/root/children?$skiptoken=x",
		"scheme-relative":                "//" + host + "/v1.0/me/drive/root/children?$skiptoken=x",
		"no scheme":                      "/v1.0/me/drive/root/children?$skiptoken=x",
		"beta endpoint":                  server.URL + "/beta/me/drive/root/children?$skiptoken=x",
		"another user's drive":           server.URL + "/v1.0/users/victim@contoso.com/drive/root/children?$skiptoken=x",
		"the mailbox":                    server.URL + "/v1.0/me/messages?$skiptoken=x",
		"a file's content":               server.URL + "/v1.0/me/drive/items/01A/content",
		"a sharing link":                 server.URL + "/v1.0/me/drive/items/01A/createLink",
		"permissions":                    server.URL + "/v1.0/me/drive/items/01A/permissions",
		"a special folder":               server.URL + "/v1.0/me/drive/special/documents/children",
		"the drive itself":               server.URL + "/v1.0/me/drive",
		"the drive root itself":          server.URL + "/v1.0/me/drive/root",
		"a single item":                  server.URL + "/v1.0/me/drive/items/01A",
		"children of one child":          server.URL + "/v1.0/me/drive/root/children/01A",
		"the sites list":                 server.URL + "/v1.0/sites?search=*",
		"a site's libraries":             server.URL + "/v1.0/sites/contoso.sharepoint.com,1,2/drives",
		"dot segments":                   server.URL + "/v1.0/me/drive/root/children/../../../messages",
		"encoded dot segments":           server.URL + "/v1.0/me/drive/items/%2e%2e/%2e%2e/messages/children",
		"dot segments in a library id":   server.URL + "/v1.0/drives/../me/messages/children",
		"empty segment":                  server.URL + "/v1.0/me/drive//root/children",
		"a fragment":                     server.URL + "/v1.0/me/drive/root/children?$skiptoken=x#frag",
		"a search function that runs on": server.URL + "/v1.0/me/drive/root/search(q='a')/children",
		"a function that is not search":  server.URL + "/v1.0/me/drive/root/delta()",
	}
	for name, token := range rejected {
		t.Run(name, func(t *testing.T) {
			_, err := runDriveOp(service, "microsoft", "drive.files", map[string]any{"pageToken": token})
			if err == nil {
				t.Fatalf("pageToken %q was accepted", token)
			}
			if !strings.Contains(err.Error(), "pageToken") {
				t.Fatalf("error = %q, want it to name pageToken", err)
			}
		})
	}
	if hits.Load() != 0 || foreignHits.Load() != 0 {
		t.Fatalf("a rejected page token still caused provider traffic: graph=%d foreign=%d", hits.Load(), foreignHits.Load())
	}
}

func TestMicrosoftDriveFilesDoesNotHandOutANextLinkItWouldRefuse(t *testing.T) {
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		driveJSON(w, map[string]any{
			"value":           []any{},
			"@odata.nextLink": "https://evil.example/v1.0/me/drive/root/children?$skiptoken=x",
		})
	})
	payload := mustRunDriveOp(t, service, "microsoft", "drive.files", nil)
	if token, present := payload["nextPageToken"]; present {
		t.Fatalf("nextPageToken = %#v for a link on a foreign host", token)
	}
	if payload["moreAvailable"] != true {
		t.Fatalf("moreAvailable = %v, want true so the caller can tell the list is cut short", payload["moreAvailable"])
	}
}

func TestMicrosoftDriveReadsRefuseUnsafeParameters(t *testing.T) {
	var hits atomic.Int32
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	tests := []struct {
		name      string
		operation string
		params    map[string]any
		want      string
	}{
		{"driveId is a dot segment", "drive.files", map[string]any{"driveId": ".."}, "driveId"},
		{"folderId is a dot segment", "drive.files", map[string]any{"folderId": ".."}, "folderId"},
		{"folderId is one dot", "drive.files", map[string]any{"folderId": "."}, "folderId"},
		{"driveId is not text", "drive.files", map[string]any{"driveId": float64(7)}, "driveId"},
		{"query is not text", "drive.files", map[string]any{"query": true}, "query"},
		{"query is too long", "drive.files", map[string]any{"query": strings.Repeat("a", 501)}, "query"},
		{"pageToken has a control character", "drive.files", map[string]any{"pageToken": "a\nb"}, "pageToken"},
		{"content without an item", "drive.file.content", map[string]any{}, "itemId is required"},
		{"content item is a dot segment", "drive.file.content", map[string]any{"itemId": ".."}, "itemId"},
		{"content drive is a dot segment", "drive.file.content", map[string]any{"itemId": "01A", "driveId": ".."}, "driveId"},
		{"sites query is not text", "sharepoint.sites", map[string]any{"query": float64(5)}, "query"},
		{"libraries without a site", "sharepoint.drives", map[string]any{}, "siteId is required"},
		{"libraries site is a dot segment", "sharepoint.drives", map[string]any{"siteId": ".."}, "siteId"},
		{"libraries site is not text", "sharepoint.drives", map[string]any{"siteId": []any{"a"}}, "siteId"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := runDriveOp(service, "microsoft", tt.operation, tt.params)
			if err == nil {
				t.Fatalf("%s %v was accepted", tt.operation, tt.params)
			}
			if !strings.Contains(err.Error(), tt.want) {
				t.Fatalf("error = %q, want it to mention %q", err, tt.want)
			}
		})
	}
	if hits.Load() != 0 {
		t.Fatalf("a refused request still reached the provider %d times", hits.Load())
	}
}

// --- Microsoft: drive.file.content -----------------------------------------------

func TestMicrosoftDriveFileContentWithDriveIdReadsFromThatLibrary(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		w.Header().Set("Content-Type", "application/pdf; charset=binary")
		_, _ = w.Write([]byte("PDF-BYTES"))
	})
	payload := mustRunDriveOp(t, service, "microsoft", "microsoft.drive.file.content", map[string]any{"driveId": "b!lib", "itemId": "01ABC"})
	call := rec.only(t)
	if call.EscapedPath != "/v1.0/drives/b%21lib/items/01ABC/content" {
		t.Fatalf("escaped path = %q, want the library's item content endpoint", call.EscapedPath)
	}
	if call.Header.Get("Authorization") != "Bearer token-1" {
		t.Fatalf("Authorization = %q", call.Header.Get("Authorization"))
	}
	decoded, err := base64.StdEncoding.DecodeString(payload["content_base64"].(string))
	if err != nil || string(decoded) != "PDF-BYTES" {
		t.Fatalf("content_base64 decodes to %q (err %v)", decoded, err)
	}
	if payload["mime_type"] != "application/pdf" {
		t.Fatalf("mime_type = %#v, want the bare type", payload["mime_type"])
	}
}

func TestMicrosoftDriveFileContentWithoutDriveIdStillReadsTheOwnDrive(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		_, _ = w.Write([]byte("x"))
	})
	mustRunDriveOp(t, service, "microsoft", "drive.file.content", map[string]any{"itemId": "01 ABC"})
	if call := rec.only(t); call.EscapedPath != "/v1.0/me/drive/items/01%20ABC/content" {
		t.Fatalf("escaped path = %q, want the own-drive content endpoint", call.EscapedPath)
	}
}

func TestMicrosoftDriveFileContentFollowsAGraphRedirectToSharePointWithoutTheBearer(t *testing.T) {
	// A SharePoint library's /content answers 302 to a pre-authenticated
	// download URL on the tenant's *.sharepoint.com host. The production
	// allow-list admits that host, and the bearer must not travel with the hop.
	var sawBearerAtSharePoint atomic.Bool
	var sharePointHits atomic.Int32
	service := newNamedHostsService(t, func(w http.ResponseWriter, r *http.Request) {
		switch strings.ToLower(r.Host) {
		case "graph.microsoft.com":
			if got := r.URL.EscapedPath(); got != "/v1.0/drives/b%21lib/items/01ABC/content" {
				t.Errorf("graph path = %q", got)
			}
			if got := r.Header.Get("Authorization"); got != "Bearer token-1" {
				t.Errorf("graph Authorization = %q", got)
			}
			http.Redirect(w, r, "https://contoso-my.sharepoint.com/personal/ola/_layouts/15/download.aspx?id=1", http.StatusFound)
		case "contoso-my.sharepoint.com":
			sharePointHits.Add(1)
			if r.Header.Get("Authorization") != "" {
				sawBearerAtSharePoint.Store(true)
			}
			w.Header().Set("Content-Type", "application/pdf")
			_, _ = w.Write([]byte("SHAREPOINT-PDF"))
		default:
			t.Errorf("unexpected host %q", r.Host)
		}
	})
	payload := mustRunDriveOp(t, service, "microsoft", "drive.file.content", map[string]any{"driveId": "b!lib", "itemId": "01ABC"})
	decoded, err := base64.StdEncoding.DecodeString(payload["content_base64"].(string))
	if err != nil || string(decoded) != "SHAREPOINT-PDF" {
		t.Fatalf("content_base64 decodes to %q (err %v), want the sharepoint.com target's bytes", decoded, err)
	}
	if sharePointHits.Load() != 1 {
		t.Fatalf("sharepoint.com requests = %d, want 1", sharePointHits.Load())
	}
	if sawBearerAtSharePoint.Load() {
		t.Fatal("the bearer token was forwarded to the pre-authenticated download host")
	}
}

func TestMicrosoftDriveFileContentRefusesARedirectToAnUndocumentedHost(t *testing.T) {
	for _, target := range []string{
		"https://evil.example/steal",
		"https://sharepoint.com.evil.example/steal",
		"https://evil-sharepoint.com/steal",
		"http://contoso-my.sharepoint.com/downgraded",
	} {
		t.Run(target, func(t *testing.T) {
			var targetHits atomic.Int32
			service := newNamedHostsService(t, func(w http.ResponseWriter, r *http.Request) {
				if strings.EqualFold(r.Host, "graph.microsoft.com") {
					http.Redirect(w, r, target, http.StatusFound)
					return
				}
				targetHits.Add(1)
				_, _ = w.Write([]byte("must never be read"))
			})
			_, err := runDriveOp(service, "microsoft", "drive.file.content", map[string]any{"driveId": "b!lib", "itemId": "01ABC"})
			if err == nil {
				t.Fatal("want an error when the redirect target is not on the documented download-host allow-list")
			}
			if targetHits.Load() != 0 {
				t.Fatalf("the refused redirect target was still requested %d times", targetHits.Load())
			}
		})
	}
}

func TestMicrosoftDriveFileContentWithDriveIdKeepsTheHardSizeReject(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		w.Header().Set("Content-Length", "1000001")
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("short"))
	})
	_, err := runDriveOp(service, "microsoft", "drive.file.content", map[string]any{"driveId": "b!lib", "itemId": "01ABC"})
	if err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("error = %v, want an 'exceeds' rejection for a file one byte over the 1,000,000 byte cap", err)
	}
	if call := rec.only(t); call.Path != "/v1.0/drives/b!lib/items/01ABC/content" {
		t.Fatalf("path = %q, want the library's item content endpoint", call.Path)
	}
}

// --- Microsoft: sharepoint.sites and sharepoint.drives -----------------------------

func TestSharePointSitesDefaultSearchesEverySite(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{map[string]any{"id": "contoso.sharepoint.com,1,2", "displayName": "Team"}}})
	})
	for _, operation := range []string{"sharepoint.sites", "microsoft.sharepoint.sites"} {
		payload := mustRunDriveOp(t, service, "microsoft", operation, nil)
		if rows, _ := payload["value"].([]any); len(rows) != 1 {
			t.Fatalf("%s value = %#v", operation, payload["value"])
		}
	}
	for _, call := range rec.all() {
		if call.Method != http.MethodGet || call.Path != "/v1.0/sites" {
			t.Fatalf("request = %s %s, want GET /v1.0/sites", call.Method, call.Path)
		}
		if got := call.Query.Get("search"); got != "*" {
			t.Fatalf("search = %q, want * so every site is listed", got)
		}
		if got := call.Query.Get("$select"); got != "id,displayName,webUrl" {
			t.Fatalf("$select = %q", got)
		}
		if _, present := call.Query["$top"]; present {
			t.Fatalf("$top is not part of the sites search: %s", call.RawQuery)
		}
		if got := call.Header.Get("Authorization"); got != "Bearer token-1" {
			t.Fatalf("Authorization = %q", got)
		}
	}
}

func TestSharePointSitesQueryIsSentAsTheSearchTerm(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	mustRunDriveOp(t, service, "microsoft", "sharepoint.sites", map[string]any{"query": "contoso team+docs"})
	call := rec.only(t)
	if got := call.Query.Get("search"); got != "contoso team+docs" {
		t.Fatalf("search = %q, want the text unchanged (a literal plus stays a plus)", got)
	}
	if strings.Contains(call.RawQuery, "+") && !strings.Contains(call.RawQuery, "%2B") {
		t.Fatalf("raw query %q carries an ambiguous plus", call.RawQuery)
	}
}

func TestSharePointSitesPagesThroughAValidatedNextLink(t *testing.T) {
	var rec driveRecorder
	var serverURL string
	service, server := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		if r.URL.Query().Get("$skiptoken") == "" {
			driveJSON(w, map[string]any{"value": []any{}, "@odata.nextLink": serverURL + "/v1.0/sites?search=*&$skiptoken=S2"})
			return
		}
		driveJSON(w, map[string]any{"value": []any{map[string]any{"id": "s2"}}})
	})
	serverURL = server.URL
	first := mustRunDriveOp(t, service, "microsoft", "sharepoint.sites", nil)
	token, _ := first["nextPageToken"].(string)
	if token != server.URL+"/v1.0/sites?search=*&$skiptoken=S2" {
		t.Fatalf("nextPageToken = %q", token)
	}
	second := mustRunDriveOp(t, service, "microsoft", "sharepoint.sites", map[string]any{"pageToken": token, "query": "ignored"})
	calls := rec.all()
	if len(calls) != 2 || calls[1].RawQuery != "search=*&$skiptoken=S2" {
		t.Fatalf("page request = %+v, want the nextLink replayed", calls)
	}
	if _, present := second["nextPageToken"]; present {
		t.Fatalf("the last page must not carry a nextPageToken: %#v", second)
	}
}

func TestSharePointDrivesListsTheSitesLibraries(t *testing.T) {
	var rec driveRecorder
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"value": []any{map[string]any{"id": "b!lib", "name": "Documents", "driveType": "documentLibrary"}}})
	})
	for _, operation := range []string{"sharepoint.drives", "microsoft.sharepoint.drives"} {
		payload := mustRunDriveOp(t, service, "microsoft", operation, map[string]any{"siteId": "contoso.sharepoint.com,11111111-aaaa,22222222-bbbb"})
		if rows, _ := payload["value"].([]any); len(rows) != 1 {
			t.Fatalf("%s value = %#v", operation, payload["value"])
		}
	}
	for _, call := range rec.all() {
		if call.Path != "/v1.0/sites/contoso.sharepoint.com,11111111-aaaa,22222222-bbbb/drives" {
			t.Fatalf("path = %q, want the site's drives endpoint", call.Path)
		}
		if call.EscapedPath != "/v1.0/sites/contoso.sharepoint.com%2C11111111-aaaa%2C22222222-bbbb/drives" {
			t.Fatalf("escaped path = %q, want the site id path-escaped", call.EscapedPath)
		}
		if got := call.Query.Get("$select"); got != "id,name,driveType,webUrl" {
			t.Fatalf("$select = %q", got)
		}
	}
}

func TestSharePointDrivesDoesNotReachTheProviderWithoutASiteId(t *testing.T) {
	var hits atomic.Int32
	service, _ := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	_, err := runDriveOp(service, "microsoft", "sharepoint.drives", nil)
	if err == nil || !strings.Contains(err.Error(), "siteId") {
		t.Fatalf("error = %v, want siteId is required", err)
	}
	if hits.Load() != 0 {
		t.Fatalf("provider requests = %d, want 0", hits.Load())
	}
}

func TestSharePointPageTokensAreBoundToTheirOwnOperation(t *testing.T) {
	var hits atomic.Int32
	service, server := newDriveGraphService(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		driveJSON(w, map[string]any{"value": []any{}})
	})
	sitesLink := server.URL + "/v1.0/sites?search=*&$skiptoken=x"
	librariesLink := server.URL + "/v1.0/sites/contoso.sharepoint.com,1,2/drives?$skiptoken=x"
	filesLink := server.URL + "/v1.0/drives/b!lib/root/children?$skiptoken=x"
	mismatched := []struct {
		operation string
		token     string
		params    map[string]any
	}{
		{"sharepoint.sites", librariesLink, nil},
		{"sharepoint.sites", filesLink, nil},
		{"sharepoint.drives", sitesLink, map[string]any{"siteId": "s"}},
		{"sharepoint.drives", filesLink, map[string]any{"siteId": "s"}},
		{"drive.files", sitesLink, nil},
		{"drive.files", librariesLink, nil},
	}
	for _, tt := range mismatched {
		params := map[string]any{"pageToken": tt.token}
		for key, value := range tt.params {
			params[key] = value
		}
		if _, err := runDriveOp(service, "microsoft", tt.operation, params); err == nil || !strings.Contains(err.Error(), "pageToken") {
			t.Fatalf("%s accepted a page token from another operation: %q (err %v)", tt.operation, tt.token, err)
		}
	}
	if hits.Load() != 0 {
		t.Fatalf("a mismatched page token still caused %d provider requests", hits.Load())
	}
	// Each operation does take its own links.
	for _, tt := range []struct {
		operation string
		token     string
		params    map[string]any
	}{
		{"sharepoint.sites", sitesLink, nil},
		{"sharepoint.drives", librariesLink, map[string]any{"siteId": "s"}},
		{"drive.files", filesLink, nil},
	} {
		params := map[string]any{"pageToken": tt.token}
		for key, value := range tt.params {
			params[key] = value
		}
		if _, err := runDriveOp(service, "microsoft", tt.operation, params); err != nil {
			t.Fatalf("%s refused its own link %q: %v", tt.operation, tt.token, err)
		}
	}
}

func TestSharePointOperationsAreMicrosoftOnly(t *testing.T) {
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		t.Errorf("provider was called for %s", r.URL.Path)
	})
	for _, operation := range []string{"sharepoint.sites", "sharepoint.drives"} {
		if _, err := runDriveOp(service, "google", operation, map[string]any{"siteId": "s"}); err == nil || !strings.Contains(err.Error(), "unsupported Google operation") {
			t.Fatalf("%s on Google: error = %v, want unsupported", operation, err)
		}
	}
}

// --- Google Drive: drive.files -------------------------------------------------------

func TestGoogleDriveFilesDefaultIncludesSharedDriveItemsAndPaging(t *testing.T) {
	var rec driveRecorder
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"files": []any{map[string]any{"id": "f1"}}})
	})
	for _, operation := range []string{"drive.files", "google.drive.files"} {
		mustRunDriveOp(t, service, "google", operation, nil)
	}
	for _, call := range rec.all() {
		if call.Path != "/drive/v3/files" {
			t.Fatalf("path = %q", call.Path)
		}
		for key, want := range map[string]string{
			"supportsAllDrives":         "true",
			"includeItemsFromAllDrives": "true",
			"pageSize":                  "10",
			"fields":                    "nextPageToken,files(id,name,mimeType,webViewLink,modifiedTime,size,parents,driveId)",
		} {
			if got := call.Query.Get(key); got != want {
				t.Fatalf("%s = %q, want %q", key, got, want)
			}
		}
		for _, key := range []string{"q", "corpora", "driveId", "pageToken"} {
			if _, present := call.Query[key]; present {
				t.Fatalf("%s must be absent on the default listing: %s", key, call.RawQuery)
			}
		}
		if got := call.Header.Get("Authorization"); got != "Bearer token-1" {
			t.Fatalf("Authorization = %q", got)
		}
	}
}

func TestGoogleDriveFilesDriveIdScopesTheListingToOneSharedDrive(t *testing.T) {
	var rec driveRecorder
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"files": []any{}})
	})
	mustRunDriveOp(t, service, "google", "drive.files", map[string]any{"driveId": "0AAbc_DEF-1"})
	call := rec.only(t)
	if call.Query.Get("driveId") != "0AAbc_DEF-1" || call.Query.Get("corpora") != "drive" {
		t.Fatalf("driveId/corpora = %q/%q, want the shared drive scoped with corpora=drive", call.Query.Get("driveId"), call.Query.Get("corpora"))
	}
	if call.Query.Get("supportsAllDrives") != "true" || call.Query.Get("includeItemsFromAllDrives") != "true" {
		t.Fatalf("shared-drive flags missing: %s", call.RawQuery)
	}
	if got := call.Query.Get("q"); got != "trashed=false" {
		t.Fatalf("q = %q, want trashed items left out of a shared-drive listing", got)
	}
}

func TestGoogleDriveFilesMatchSelectsNameOrFullText(t *testing.T) {
	tests := []struct {
		name   string
		params map[string]any
		wantQ  string
	}{
		{"default stays a name match", map[string]any{"query": "report"}, "name contains 'report' and trashed=false"},
		{"explicit name", map[string]any{"query": "report", "match": "name"}, "name contains 'report' and trashed=false"},
		{"text", map[string]any{"query": "Q3 budget", "match": "text"}, "fullText contains 'Q3 budget' and trashed=false"},
		{"text is case-insensitive", map[string]any{"query": "budget", "match": "TEXT"}, "fullText contains 'budget' and trashed=false"},
		{"text with a quote and a folder", map[string]any{"query": "Ola's", "match": "text", "folderId": "folder-1"}, `fullText contains 'Ola\'s' and trashed=false and 'folder-1' in parents`},
		{"name with a folder (the picker's query)", map[string]any{"query": "report", "folderId": "folder-1"}, "name contains 'report' and trashed=false and 'folder-1' in parents"},
		{"folder only leaves trashed files out", map[string]any{"folderId": "folder-1"}, "trashed=false and 'folder-1' in parents"},
		{"match without a query changes nothing", map[string]any{"match": "text"}, ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var rec driveRecorder
			service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				driveJSON(w, map[string]any{"files": []any{}})
			})
			mustRunDriveOp(t, service, "google", "drive.files", tt.params)
			call := rec.only(t)
			got, present := call.Query["q"]
			if tt.wantQ == "" {
				if present {
					t.Fatalf("q = %q, want no q at all", got)
				}
				return
			}
			if !present || got[0] != tt.wantQ {
				t.Fatalf("q = %q, want %q", got, tt.wantQ)
			}
		})
	}
}

func TestGoogleDriveFilesPageTokenGoesToGoogleAndNextPageTokenComesBack(t *testing.T) {
	var rec driveRecorder
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		driveJSON(w, map[string]any{"files": []any{}, "nextPageToken": "NEXT/1+=="})
	})
	payload := mustRunDriveOp(t, service, "google", "drive.files", map[string]any{"pageToken": "tok/en+1==", "maxResults": float64(5)})
	call := rec.only(t)
	if call.Query.Get("pageToken") != "tok/en+1==" || call.Query.Get("pageSize") != "5" {
		t.Fatalf("pageToken/pageSize = %q/%q", call.Query.Get("pageToken"), call.Query.Get("pageSize"))
	}
	if payload["nextPageToken"] != "NEXT/1+==" {
		t.Fatalf("nextPageToken = %#v, want Google's token passed back", payload["nextPageToken"])
	}
}

func TestGoogleDriveReadsRefuseUnsafeParameters(t *testing.T) {
	var hits atomic.Int32
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		driveJSON(w, map[string]any{"files": []any{}})
	})
	tests := []struct {
		name      string
		operation string
		params    map[string]any
		want      string
	}{
		{"unknown match", "drive.files", map[string]any{"query": "a", "match": "semantic"}, "match"},
		{"match is not text", "drive.files", map[string]any{"match": float64(1)}, "match"},
		{"query is not text", "drive.files", map[string]any{"query": []any{"a"}}, "query"},
		{"query is too long", "drive.files", map[string]any{"query": strings.Repeat("a", 501)}, "query"},
		{"driveId with a control character", "drive.files", map[string]any{"driveId": "a\nb"}, "driveId"},
		{"driveId is not text", "drive.files", map[string]any{"driveId": true}, "driveId"},
		{"pageToken with a control character", "drive.files", map[string]any{"pageToken": "a\nb"}, "pageToken"},
		{"content without a file", "drive.file.content", map[string]any{}, "fileId is required"},
		{"content file is a dot segment", "drive.file.content", map[string]any{"fileId": ".."}, "fileId"},
		{"unknown exportAs", "drive.file.content", map[string]any{"fileId": "f1", "exportAs": "pdf"}, "exportAs"},
		{"exportAs is not text", "drive.file.content", map[string]any{"fileId": "f1", "exportAs": float64(1)}, "exportAs"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := runDriveOp(service, "google", tt.operation, tt.params)
			if err == nil {
				t.Fatalf("%s %v was accepted", tt.operation, tt.params)
			}
			if !strings.Contains(err.Error(), tt.want) {
				t.Fatalf("error = %q, want it to mention %q", err, tt.want)
			}
		})
	}
	if hits.Load() != 0 {
		t.Fatalf("a refused request still reached the provider %d times", hits.Load())
	}
}

// --- Google Drive: drive.file.content ---------------------------------------------------

func TestGoogleDriveFileContentReadsABinaryFileFromAnyDrive(t *testing.T) {
	var rec driveRecorder
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		w.Header().Set("Content-Type", "application/pdf")
		_, _ = w.Write([]byte("PDF-BYTES"))
	})
	payload := mustRunDriveOp(t, service, "google", "google.drive.file.content", map[string]any{"fileId": "f-1", "mimeType": "application/pdf"})
	call := rec.only(t)
	if call.Path != "/drive/v3/files/f-1" {
		t.Fatalf("path = %q", call.Path)
	}
	if call.Query.Get("alt") != "media" || call.Query.Get("supportsAllDrives") != "true" {
		t.Fatalf("query = %s, want alt=media and supportsAllDrives=true so a shared-drive file is found", call.RawQuery)
	}
	decoded, err := base64.StdEncoding.DecodeString(payload["content_base64"].(string))
	if err != nil || string(decoded) != "PDF-BYTES" {
		t.Fatalf("content_base64 decodes to %q (err %v)", decoded, err)
	}
	if payload["mime_type"] != "application/pdf" {
		t.Fatalf("mime_type = %#v", payload["mime_type"])
	}
	if _, present := payload["export_extension"]; present {
		t.Fatalf("export_extension must be absent for a binary file: %#v", payload)
	}
}

func TestGoogleDriveFileContentExportAsTextMapsEachNativeType(t *testing.T) {
	tests := []struct {
		name          string
		nativeMIME    string
		wantExport    string
		wantExtension string
	}{
		{"docs to plain text", "application/vnd.google-apps.document", "text/plain", ".txt"},
		{"sheets to csv", "application/vnd.google-apps.spreadsheet", "text/csv", ".csv"},
		{"slides to plain text", "application/vnd.google-apps.presentation", "text/plain", ".txt"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var rec driveRecorder
			service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				w.Header().Set("Content-Type", tt.wantExport+"; charset=UTF-8")
				_, _ = w.Write([]byte("hello,world"))
			})
			payload := mustRunDriveOp(t, service, "google", "drive.file.content", map[string]any{
				"fileId": "doc-1", "mimeType": tt.nativeMIME, "exportAs": "text",
			})
			call := rec.only(t)
			if call.Path != "/drive/v3/files/doc-1/export" {
				t.Fatalf("path = %q, want the export endpoint", call.Path)
			}
			if got := call.Query.Get("mimeType"); got != tt.wantExport {
				t.Fatalf("export mimeType = %q, want %q", got, tt.wantExport)
			}
			if _, present := call.Query["supportsAllDrives"]; present {
				t.Fatalf("files.export takes no supportsAllDrives parameter: %s", call.RawQuery)
			}
			if payload["export_extension"] != tt.wantExtension {
				t.Fatalf("export_extension = %#v, want %q", payload["export_extension"], tt.wantExtension)
			}
			if payload["mime_type"] != tt.wantExport {
				t.Fatalf("mime_type = %#v, want %q", payload["mime_type"], tt.wantExport)
			}
			decoded, err := base64.StdEncoding.DecodeString(payload["content_base64"].(string))
			if err != nil || string(decoded) != "hello,world" {
				t.Fatalf("content_base64 decodes to %q (err %v)", decoded, err)
			}
		})
	}
}

func TestGoogleDriveFileContentExportAsFallsBackToTheTextMimeWhenContentTypeIsMissing(t *testing.T) {
	// A raw response with no Content-Type header: mime_type must be the text
	// export type, not the native application/vnd.google-apps.* type.
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		hijacker, ok := w.(http.Hijacker)
		if !ok {
			t.Error("test response writer cannot hijack")
			return
		}
		conn, buf, err := hijacker.Hijack()
		if err != nil {
			t.Errorf("hijack: %v", err)
			return
		}
		defer conn.Close()
		_, _ = buf.WriteString("HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
		_ = buf.Flush()
	})
	payload := mustRunDriveOp(t, service, "google", "drive.file.content", map[string]any{
		"fileId": "doc-1", "mimeType": "application/vnd.google-apps.spreadsheet", "exportAs": "text",
	})
	if payload["mime_type"] != "text/csv" {
		t.Fatalf("mime_type = %#v, want text/csv", payload["mime_type"])
	}
}

func TestGoogleDriveFileContentExportDefaultsStayOfficeFormats(t *testing.T) {
	wantByNative := map[string]string{
		"application/vnd.google-apps.document":     "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
		"application/vnd.google-apps.spreadsheet":  "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
		"application/vnd.google-apps.presentation": "application/vnd.openxmlformats-officedocument.presentationml.presentation",
	}
	for native, want := range wantByNative {
		var rec driveRecorder
		service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
			rec.record(r)
			_, _ = w.Write([]byte("OFFICE"))
		})
		mustRunDriveOp(t, service, "google", "drive.file.content", map[string]any{"fileId": "f", "mimeType": native})
		if got := rec.only(t).Query.Get("mimeType"); got != want {
			t.Fatalf("%s exports as %q by default, want %q (the picker is unchanged)", native, got, want)
		}
	}
}

func TestGoogleDriveFileContentExportAsIsIgnoredForABinaryFile(t *testing.T) {
	var rec driveRecorder
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		rec.record(r)
		w.Header().Set("Content-Type", "application/pdf")
		_, _ = w.Write([]byte("PDF"))
	})
	mustRunDriveOp(t, service, "google", "drive.file.content", map[string]any{"fileId": "f", "mimeType": "application/pdf", "exportAs": "text"})
	call := rec.only(t)
	if call.Path != "/drive/v3/files/f" || call.Query.Get("alt") != "media" {
		t.Fatalf("request = %s?%s, want the plain download (only native files are exported)", call.Path, call.RawQuery)
	}
}

func TestGoogleDriveFileContentLooksUpTheMimeTypeOnlyWhenTheCallerDoesNotKnowIt(t *testing.T) {
	// The picker always sends mimeType (from its own listing) and must cost
	// exactly one request. A caller that has lost it would otherwise ask for
	// alt=media on a Docs Editors file, which Google answers 403
	// fileNotDownloadable, and that reads as a credential rejection.
	tests := []struct {
		name         string
		params       map[string]any
		lookupMIME   string
		lookupPath   string
		wantRequests int
		wantPath     string
		wantQuery    map[string]string
	}{
		{
			name:         "mimeType given: one request",
			params:       map[string]any{"fileId": "doc-1", "mimeType": "application/pdf"},
			wantRequests: 1,
			wantPath:     "/drive/v3/files/doc-1",
			wantQuery:    map[string]string{"alt": "media"},
		},
		{
			name:         "native doc, mimeType unknown: looked up, then exported as docx",
			params:       map[string]any{"fileId": "doc-1"},
			lookupMIME:   "application/vnd.google-apps.document",
			lookupPath:   "/drive/v3/files/doc-1",
			wantRequests: 2,
			wantPath:     "/drive/v3/files/doc-1/export",
			wantQuery:    map[string]string{"mimeType": "application/vnd.openxmlformats-officedocument.wordprocessingml.document"},
		},
		{
			name:         "native sheet, mimeType unknown, exportAs text: looked up, then exported as csv",
			params:       map[string]any{"fileId": "sheet-1", "exportAs": "text"},
			lookupMIME:   "application/vnd.google-apps.spreadsheet",
			lookupPath:   "/drive/v3/files/sheet-1",
			wantRequests: 2,
			wantPath:     "/drive/v3/files/sheet-1/export",
			wantQuery:    map[string]string{"mimeType": "text/csv"},
		},
		{
			name:         "binary file, mimeType unknown: looked up, then downloaded",
			params:       map[string]any{"fileId": "pdf-1"},
			lookupMIME:   "application/pdf",
			lookupPath:   "/drive/v3/files/pdf-1",
			wantRequests: 2,
			wantPath:     "/drive/v3/files/pdf-1",
			wantQuery:    map[string]string{"alt": "media", "supportsAllDrives": "true"},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			var rec driveRecorder
			service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
				rec.record(r)
				if r.URL.Query().Get("fields") == "mimeType" {
					driveJSON(w, map[string]any{"mimeType": tt.lookupMIME})
					return
				}
				w.Header().Set("Content-Type", "application/octet-stream")
				_, _ = w.Write([]byte("BYTES"))
			})
			mustRunDriveOp(t, service, "google", "drive.file.content", tt.params)
			calls := rec.all()
			if len(calls) != tt.wantRequests {
				t.Fatalf("requests = %d, want %d: %+v", len(calls), tt.wantRequests, calls)
			}
			if tt.wantRequests == 2 {
				lookup := calls[0]
				if lookup.Path != tt.lookupPath {
					t.Fatalf("lookup path = %q, want the file's metadata endpoint %q", lookup.Path, tt.lookupPath)
				}
				if lookup.Query.Get("supportsAllDrives") != "true" || lookup.Query.Get("fields") != "mimeType" {
					t.Fatalf("lookup query = %s, want fields=mimeType and supportsAllDrives=true", lookup.RawQuery)
				}
				if _, present := lookup.Query["alt"]; present {
					t.Fatalf("the lookup must not ask for the content: %s", lookup.RawQuery)
				}
			}
			final := calls[len(calls)-1]
			if final.Path != tt.wantPath {
				t.Fatalf("content path = %q, want %q", final.Path, tt.wantPath)
			}
			for key, want := range tt.wantQuery {
				if got := final.Query.Get(key); got != want {
					t.Fatalf("content %s = %q, want %q", key, got, want)
				}
			}
		})
	}
}

func TestGoogleDriveFileContentMimeTypeLookupFailureIsReportedAsIs(t *testing.T) {
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"error":{"code":404,"message":"File not found: x.","errors":[{"reason":"notFound"}]}}`))
	})
	_, err := runDriveOp(service, "google", "drive.file.content", map[string]any{"fileId": "gone"})
	if err == nil || !strings.Contains(err.Error(), "404") {
		t.Fatalf("error = %v, want the provider's 404", err)
	}
	if _, isAuth := AsProviderAuthError(err); isAuth {
		t.Fatalf("a missing file must not read as a reconnect: %v", err)
	}
}

func TestGoogleDriveFileContentAGenuineForbiddenStaysAReconnect(t *testing.T) {
	service := newDriveGoogleService(t, func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusForbidden)
		_, _ = w.Write([]byte(`{"error":{"code":403,"message":"Request had insufficient authentication scopes.","errors":[{"reason":"insufficientPermissions"}],"status":"PERMISSION_DENIED"}}`))
	})
	_, err := runDriveOp(service, "google", "drive.file.content", map[string]any{"fileId": "f", "mimeType": "application/pdf"})
	if _, isAuth := AsProviderAuthError(err); !isAuth {
		t.Fatalf("error = %v, want the credential rejection kept for a real scope failure", err)
	}
}
