package codexsubscription

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/rs/zerolog"
)

// sentinel returns a unique marker that must never appear in a log line or
// an error string.
func sentinel(kind string) string {
	raw := make([]byte, 6)
	_, _ = rand.Read(raw)
	return "SENTINEL-" + kind + "-" + hex.EncodeToString(raw)
}

// fakeJWT is an unsigned header.payload.sig token. The signature slot holds
// sig verbatim, so a sentinel there stays visible if the raw token leaks.
func fakeJWT(claims map[string]any, sig string) string {
	header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"none","typ":"JWT"}`))
	payload, _ := json.Marshal(claims)
	return header + "." + base64.RawURLEncoding.EncodeToString(payload) + "." + sig
}

type authOptions struct {
	accessExp    time.Time
	accessClaims map[string]any
	idClaims     map[string]any
	// accountID nil omits tokens.account_id.
	accountID   *string
	lastRefresh string
	// extra is raw top-level members appended after last_refresh.
	extra    string
	authMode string
	mode     os.FileMode
}

type authFixture struct {
	access, refresh, id string
	path                string
}

func strPtr(value string) *string { return &value }

// writeAuthJSON writes the Codex auth.json layout with sentinel tokens.
func writeAuthJSON(t *testing.T, dir string, opts authOptions) authFixture {
	t.Helper()
	accessClaims := map[string]any{"client_id": "app_TESTCLIENT12345"}
	if !opts.accessExp.IsZero() {
		accessClaims["exp"] = opts.accessExp.Unix()
	}
	for key, value := range opts.accessClaims {
		accessClaims[key] = value
	}
	idClaims := map[string]any{}
	for key, value := range opts.idClaims {
		idClaims[key] = value
	}
	fixture := authFixture{
		access:  fakeJWT(accessClaims, sentinel("access")),
		refresh: sentinel("refresh"),
		id:      fakeJWT(idClaims, sentinel("id")),
		path:    filepath.Join(dir, "auth.json"),
	}
	mode := opts.authMode
	if mode == "" {
		mode = "chatgpt"
	}
	var doc strings.Builder
	doc.WriteString("{\n  \"auth_mode\": " + quoteJSON(mode) + ",\n  \"OPENAI_API_KEY\": null,\n  \"tokens\": {\n")
	doc.WriteString("    \"id_token\": " + quoteJSON(fixture.id) + ",\n")
	doc.WriteString("    \"access_token\": " + quoteJSON(fixture.access) + ",\n")
	doc.WriteString("    \"refresh_token\": " + quoteJSON(fixture.refresh))
	if opts.accountID != nil {
		doc.WriteString(",\n    \"account_id\": " + quoteJSON(*opts.accountID))
	}
	doc.WriteString("\n  }")
	if opts.lastRefresh != "" {
		doc.WriteString(",\n  \"last_refresh\": " + quoteJSON(opts.lastRefresh))
	}
	if opts.extra != "" {
		doc.WriteString(",\n  " + opts.extra)
	}
	doc.WriteString("\n}")
	perm := opts.mode
	if perm == 0 {
		perm = 0o600
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(fixture.path, []byte(doc.String()), perm); err != nil {
		t.Fatal(err)
	}
	return fixture
}

func quoteJSON(value string) string {
	encoded, _ := json.Marshal(value)
	return string(encoded)
}

// freshAuth writes an auth.json whose access token is valid for an hour.
func freshAuth(t *testing.T, dir string, now time.Time) authFixture {
	t.Helper()
	return writeAuthJSON(t, dir, authOptions{accessExp: now.Add(time.Hour), accountID: strPtr("acct-file"), lastRefresh: now.Format(time.RFC3339)})
}

// sse encodes one server-sent event.
func sse(value any) string {
	encoded, _ := json.Marshal(value)
	return "data: " + string(encoded) + "\n\n"
}

type anyMap = map[string]any

func createdFrame(model string) string {
	return sse(anyMap{"type": "response.created", "response": anyMap{"id": "resp_1", "model": model}})
}

func deltaFrame(delta string) string {
	return sse(anyMap{"type": "response.output_text.delta", "delta": delta})
}

func completedFrame(model string, usage any) string {
	response := anyMap{"id": "resp_1", "model": model, "service_tier": "default"}
	if usage != nil {
		response["usage"] = usage
	}
	return sse(anyMap{"type": "response.completed", "response": response})
}

var goldenUsage = anyMap{
	"input_tokens":          1200,
	"input_tokens_details":  anyMap{"cached_tokens": 1024},
	"output_tokens":         42,
	"output_tokens_details": anyMap{"reasoning_tokens": 30},
	"total_tokens":          1242,
}

func successFrames(model string, deltas ...string) []string {
	frames := []string{createdFrame(model)}
	for _, delta := range deltas {
		frames = append(frames, deltaFrame(delta))
	}
	return append(frames, completedFrame(model, goldenUsage))
}

// fakeTurn scripts one upstream response.
type fakeTurn struct {
	status  int
	headers map[string]string
	frames  []string
	body    string
	// closeBeforeHeaders hijacks and closes the connection without a response.
	closeBeforeHeaders bool
	// killAfterFrames closes the connection after that many frames (0: never).
	killAfterFrames int
	// waitForCancel blocks after the frames until the client goes away.
	waitForCancel bool
	before        func(r *http.Request)
}

type recordedRequest struct {
	method, path string
	header       http.Header
	body         []byte
}

type fakeChatGPT struct {
	*httptest.Server
	mu       sync.Mutex
	requests []recordedRequest
	script   []fakeTurn
	canceled chan struct{}
	// handler, when set, answers every request instead of the script.
	handler http.HandlerFunc
}

func (f *fakeChatGPT) setScript(script ...fakeTurn) {
	f.mu.Lock()
	f.script = script
	f.mu.Unlock()
}

func (f *fakeChatGPT) setHandler(handler http.HandlerFunc) {
	f.mu.Lock()
	f.handler = handler
	f.mu.Unlock()
}

func newFakeChatGPT(t *testing.T, script ...fakeTurn) *fakeChatGPT {
	t.Helper()
	fake := &fakeChatGPT{script: script, canceled: make(chan struct{}, 16)}
	fake.Server = httptest.NewTLSServer(http.HandlerFunc(fake.serve))
	t.Cleanup(fake.Close)
	return fake
}

func (f *fakeChatGPT) serve(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	handler := f.handler
	f.mu.Unlock()
	if handler != nil {
		handler(w, r)
		return
	}
	body, _ := io.ReadAll(r.Body)
	f.mu.Lock()
	index := len(f.requests)
	f.requests = append(f.requests, recordedRequest{method: r.Method, path: r.URL.Path, header: r.Header.Clone(), body: body})
	turn := fakeTurn{status: http.StatusOK, frames: successFrames("gpt-5.6-terra", "ok")}
	if len(f.script) > 0 {
		turn = f.script[min(index, len(f.script)-1)]
	}
	f.mu.Unlock()
	if turn.before != nil {
		turn.before(r)
	}
	if turn.closeBeforeHeaders {
		if hijacker, ok := w.(http.Hijacker); ok {
			if conn, _, err := hijacker.Hijack(); err == nil {
				_ = conn.Close()
			}
		}
		return
	}
	for key, value := range turn.headers {
		w.Header().Set(key, value)
	}
	if turn.body == "" && w.Header().Get("Content-Type") == "" {
		w.Header().Set("Content-Type", "text/event-stream")
	}
	status := turn.status
	if status == 0 {
		status = http.StatusOK
	}
	w.WriteHeader(status)
	flusher, _ := w.(http.Flusher)
	if turn.body != "" {
		_, _ = io.WriteString(w, turn.body)
		return
	}
	for index, frame := range turn.frames {
		_, _ = io.WriteString(w, frame)
		if flusher != nil {
			flusher.Flush()
		}
		if turn.killAfterFrames > 0 && index+1 >= turn.killAfterFrames {
			if hijacker, ok := w.(http.Hijacker); ok {
				if conn, _, err := hijacker.Hijack(); err == nil {
					_ = conn.Close()
				}
			}
			return
		}
	}
	if turn.waitForCancel {
		select {
		case <-r.Context().Done():
			f.canceled <- struct{}{}
		case <-time.After(5 * time.Second):
		}
	}
}

func (f *fakeChatGPT) recorded() []recordedRequest {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]recordedRequest(nil), f.requests...)
}

func (f *fakeChatGPT) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.requests)
}

func (f *fakeChatGPT) lastBody(t *testing.T) map[string]any {
	t.Helper()
	requests := f.recorded()
	if len(requests) == 0 {
		t.Fatal("upstream received no request")
	}
	var body map[string]any
	if err := json.Unmarshal(requests[len(requests)-1].body, &body); err != nil {
		t.Fatalf("decode upstream body: %v", err)
	}
	return body
}

// fakeAuth is a fake OAuth token endpoint.
type fakeAuth struct {
	*httptest.Server
	mu      sync.Mutex
	calls   int
	forms   []url.Values
	headers []http.Header
	respond func(call int, form url.Values) (int, string)
}

func newFakeAuth(t *testing.T, respond func(call int, form url.Values) (int, string)) *fakeAuth {
	t.Helper()
	fake := &fakeAuth{respond: respond}
	fake.Server = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = r.ParseForm()
		fake.mu.Lock()
		fake.calls++
		call := fake.calls
		fake.forms = append(fake.forms, r.PostForm)
		fake.headers = append(fake.headers, r.Header.Clone())
		respond := fake.respond
		fake.mu.Unlock()
		status, body := http.StatusOK, `{"access_token":"unused"}`
		if respond != nil {
			status, body = respond(call, r.PostForm)
		}
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = io.WriteString(w, body)
	}))
	t.Cleanup(fake.Close)
	return fake
}

func (f *fakeAuth) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls
}

// grantBody is a token endpoint 200 body; empty values are omitted.
func grantBody(access, refresh, id string) string {
	body := anyMap{"access_token": access, "token_type": "Bearer", "expires_in": 3600}
	if refresh != "" {
		body["refresh_token"] = refresh
	}
	if id != "" {
		body["id_token"] = id
	}
	encoded, _ := json.Marshal(body)
	return string(encoded)
}

// scriptedClock returns the given offsets from base in order, then keeps
// returning the last one.
type scriptedClock struct {
	mu      sync.Mutex
	base    time.Time
	offsets []time.Duration
	calls   int
}

func newScriptedClock(offsets ...time.Duration) *scriptedClock {
	return &scriptedClock{base: time.Unix(1_800_000_000, 0).UTC(), offsets: offsets}
}

func (c *scriptedClock) now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	offset := c.offsets[min(c.calls, len(c.offsets)-1)]
	c.calls++
	return c.base.Add(offset)
}

// directHarness wires a DirectTransport and credential store to fakes.
type directHarness struct {
	t         *testing.T
	home      string
	now       time.Time
	upstream  *fakeChatGPT
	auth      *fakeAuth
	store     *credentialStore
	transport *DirectTransport
	logs      *syncBuffer
	sleepsMu  sync.Mutex
	sleeps    []time.Duration
}

type syncBuffer struct {
	mu  sync.Mutex
	buf bytes.Buffer
}

func (b *syncBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.Write(p)
}

func (b *syncBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.buf.String()
}

type harnessOptions struct {
	direct  DirectConfig
	respond func(call int, form url.Values) (int, string)
	noAuth  bool
}

func newDirectHarness(t *testing.T, script []fakeTurn, configure ...func(*harnessOptions)) *directHarness {
	t.Helper()
	options := harnessOptions{direct: DirectConfig{MaxRetries: 2}}
	for _, apply := range configure {
		apply(&options)
	}
	h := &directHarness{t: t, home: filepath.Join(t.TempDir(), "conn-test"), now: time.Now().UTC().Truncate(time.Second), logs: &syncBuffer{}}
	if err := os.MkdirAll(h.home, 0o700); err != nil {
		t.Fatal(err)
	}
	h.upstream = newFakeChatGPT(t, script...)
	h.auth = newFakeAuth(t, options.respond)
	logger := zerolog.New(h.logs)
	h.store = newCredentialStore(credentialStoreConfig{
		TokenURL:   h.auth.URL + "/oauth/token",
		HTTPClient: h.upstream.Client(),
		Now:        func() time.Time { return h.now },
		Logger:     logger,
	})
	direct := options.direct
	direct.BaseURL = h.upstream.URL + "/backend-api/codex"
	direct.HTTPClient = h.upstream.Client()
	direct.Logger = logger
	transport, err := NewDirectTransport(direct, h.store)
	if err != nil {
		t.Fatalf("NewDirectTransport: %v", err)
	}
	transport.sleep = func(ctx context.Context, delay time.Duration) error {
		h.sleepsMu.Lock()
		h.sleeps = append(h.sleeps, delay)
		h.sleepsMu.Unlock()
		return ctx.Err()
	}
	transport.jitter = func(base time.Duration) time.Duration { return base }
	h.transport = transport
	return h
}

func (h *directHarness) recordedSleeps() []time.Duration {
	h.sleepsMu.Lock()
	defer h.sleepsMu.Unlock()
	return append([]time.Duration(nil), h.sleeps...)
}

func testRequest() InvokeRequest {
	return InvokeRequest{
		ConnectionID:    "conn-test",
		RequestID:       "req-test",
		Model:           "gpt-5.6-terra",
		Messages:        []ChatMessage{{Role: "user", Content: "hello"}},
		ReasoningEffort: "low",
	}
}
