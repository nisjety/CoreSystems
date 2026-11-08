package resources

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/egress"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/httpx"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// stubResolver is the DNS the subscription tests see: fixed answers, so the real egress
// guard policy runs without a network. An IP literal answers itself.
type stubResolver map[string][]netip.Addr

func (s stubResolver) LookupNetIP(_ context.Context, _, host string) ([]netip.Addr, error) {
	if a, err := netip.ParseAddr(host); err == nil {
		return []netip.Addr{a}, nil
	}
	if addrs, ok := s[host]; ok {
		return addrs, nil
	}
	return nil, &stubNoHost{host}
}

type stubNoHost struct{ host string }

func (e *stubNoHost) Error() string { return "no such host: " + e.host }

func testGuardVetter() HostVetter {
	g := egress.New(egress.WithResolver(stubResolver{
		"hooks.example.com":  {netip.MustParseAddr("93.184.216.34")},
		"localhost":          {netip.MustParseAddr("127.0.0.1")},
		"other.example.org":  {netip.MustParseAddr("93.184.216.35")},
		"internal.corp.test": {netip.MustParseAddr("10.1.2.3")},
		"rebind.example.com": {netip.MustParseAddr("93.184.216.34"), netip.MustParseAddr("127.0.0.1")},
	}))
	return func(ctx context.Context, host string) error {
		_, err := g.ResolveAndVet(ctx, host)
		return err
	}
}

func newSubServer(t *testing.T, maxPerOrg int) (http.Handler, store.DB) {
	t.Helper()
	db := store.NewMemory()
	r := chi.NewRouter()
	r.Use(httpx.RequestID)
	MountWebhookSubscriptions(r, db, WebhookSubscriptionOptions{Vet: testGuardVetter(), MaxPerOrg: maxPerOrg})
	return r, db
}

type subResp struct {
	Code int
	Raw  string
	Data map[string]any
	Err  map[string]any
}

func subDo(t *testing.T, h http.Handler, method, path string, body any) subResp {
	t.Helper()
	var rd *bytes.Reader
	if body != nil {
		raw, _ := json.Marshal(body)
		rd = bytes.NewReader(raw)
	} else {
		rd = bytes.NewReader(nil)
	}
	req := httptest.NewRequest(method, path, rd)
	req.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	out := subResp{Code: w.Code, Raw: w.Body.String()}
	var env struct {
		Data  map[string]any `json:"data"`
		Error map[string]any `json:"error"`
	}
	if w.Body.Len() > 0 {
		_ = json.Unmarshal(w.Body.Bytes(), &env)
	}
	out.Data, out.Err = env.Data, env.Error
	return out
}

func createSub(t *testing.T, h http.Handler, org, url string, events []string) subResp {
	t.Helper()
	return subDo(t, h, http.MethodPost, "/v1/webhook-subscriptions?org_id="+org,
		map[string]any{"url": url, "events": events, "description": "d", "created_by": "user_1"})
}

func errField(r subResp) (field, reason string) {
	d, _ := r.Err["details"].(map[string]any)
	field, _ = d["field"].(string)
	reason, _ = d["reason"].(string)
	return
}

func TestSubscriptions_CreateShowsSecretOnce(t *testing.T) {
	t.Parallel()
	h, db := newSubServer(t, 0)

	created := createSub(t, h, "org_a", "https://hooks.example.com/q", []string{"change_detected"})
	if created.Code != http.StatusCreated {
		t.Fatalf("create = %d: %s", created.Code, created.Raw)
	}
	secret, _ := created.Data["secret"].(string)
	if !strings.HasPrefix(secret, "whsec_") || len(secret) != len("whsec_")+64 {
		t.Fatalf("secret = %q, want whsec_ + 64 hex", secret)
	}
	hint, _ := created.Data["secret_hint"].(string)
	if hint != "whsec_..."+secret[len(secret)-4:] {
		t.Fatalf("secret_hint = %q", hint)
	}
	id, _ := created.Data["id"].(string)
	if !strings.HasPrefix(id, "whk_") || created.Data["active"] != true || created.Data["url"] != "https://hooks.example.com/q" {
		t.Fatalf("unexpected create body: %s", created.Raw)
	}

	// The stored secret is the one returned, and no other response ever shows it.
	if stored, ok := db.Webhooks().Get(quarrycontracts.ID(id)); !ok || stored.Secret != secret || stored.OrgID != "org_a" || stored.CreatedBy != "user_1" {
		t.Fatalf("stored row = %+v", stored)
	}
	for _, path := range []string{
		"/v1/webhook-subscriptions?org_id=org_a",
		"/v1/webhook-subscriptions/" + id + "?org_id=org_a",
		"/v1/webhook-subscriptions/" + id + "/deliveries?org_id=org_a",
	} {
		got := subDo(t, h, http.MethodGet, path, nil)
		if got.Code != http.StatusOK {
			t.Fatalf("GET %s = %d: %s", path, got.Code, got.Raw)
		}
		if strings.Contains(got.Raw, secret) || strings.Contains(got.Raw, `"secret"`) {
			t.Fatalf("GET %s leaked the secret: %s", path, got.Raw)
		}
	}
	patched := subDo(t, h, http.MethodPatch, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", map[string]any{"active": false})
	if patched.Code != http.StatusOK || strings.Contains(patched.Raw, secret) {
		t.Fatalf("PATCH leaked or failed: %d %s", patched.Code, patched.Raw)
	}
}

func TestSubscriptions_OrgIsolation(t *testing.T) {
	t.Parallel()
	h, db := newSubServer(t, 0)
	a := createSub(t, h, "org_a", "https://hooks.example.com/a", []string{"change_detected"})
	id := a.Data["id"].(string)
	_ = createSub(t, h, "org_b", "https://hooks.example.com/b", []string{"change_detected"})

	// org_b sees only its own row.
	list := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions?org_id=org_b", nil)
	items, _ := list.Data["items"].([]any)
	if len(items) != 1 || items[0].(map[string]any)["url"] != "https://hooks.example.com/b" {
		t.Fatalf("org_b list = %s", list.Raw)
	}
	// Every route answers a foreign id exactly as a missing one: 404.
	for _, tc := range []struct{ method, suffix string }{
		{http.MethodGet, ""},
		{http.MethodPatch, ""},
		{http.MethodDelete, ""},
		{http.MethodPost, "/rotate-secret"},
		{http.MethodGet, "/deliveries"},
		{http.MethodPost, "/deliveries/whkd_x/retry"},
	} {
		got := subDo(t, h, tc.method, "/v1/webhook-subscriptions/"+id+tc.suffix+"?org_id=org_b", map[string]any{})
		if got.Code != http.StatusNotFound {
			t.Errorf("org_b %s %s = %d, want 404: %s", tc.method, tc.suffix, got.Code, got.Raw)
		}
	}
	if _, ok := db.Webhooks().GetByOrg(quarrycontracts.ID(id), "org_a"); !ok {
		t.Fatal("org_b's attempts changed org_a's subscription")
	}
	// A missing id is the same 404 body shape.
	missing := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions/whk_missing?org_id=org_b", nil)
	if missing.Code != http.StatusNotFound {
		t.Fatalf("missing = %d", missing.Code)
	}
	// No org is a 400 on every route, never an unscoped read.
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, "/v1/webhook-subscriptions"},
		{http.MethodPost, "/v1/webhook-subscriptions"},
		{http.MethodGet, "/v1/webhook-subscriptions/" + id},
		{http.MethodDelete, "/v1/webhook-subscriptions/" + id},
	} {
		if got := subDo(t, h, tc.method, tc.path, map[string]any{}); got.Code != http.StatusBadRequest {
			t.Errorf("%s %s without org = %d, want 400", tc.method, tc.path, got.Code)
		}
	}
}

func TestSubscriptions_URLValidation(t *testing.T) {
	t.Parallel()
	h, _ := newSubServer(t, 0)
	cases := []struct {
		name, url, reason string
	}{
		{"http", "http://hooks.example.com/q", "url_not_https"},
		{"userinfo", "https://user:pw@hooks.example.com/q", "url_userinfo"},
		{"fragment", "https://hooks.example.com/q#frag", "url_fragment"},
		{"localhost", "https://localhost/q", "url_private_address"},
		{"loopback v4", "https://127.0.0.1/q", "url_private_address"},
		{"loopback v6", "https://[::1]/q", "url_private_address"},
		{"rfc1918 literal", "https://10.0.0.5/q", "url_private_address"},
		{"metadata literal", "https://169.254.169.254/latest/meta-data", "url_private_address"},
		{"metadata host", "https://metadata.google.internal/q", "url_private_address"},
		{"private by dns", "https://internal.corp.test/q", "url_private_address"},
		{"rebinding mix", "https://rebind.example.com/q", "url_private_address"},
		{"ipv4-mapped private", "https://[::ffff:10.0.0.1]/q", "url_private_address"},
		{"unresolvable", "https://nxdomain.example.invalid/q", "url_unresolvable"},
		{"no scheme", "hooks.example.com/q", "url_invalid"},
		{"empty", "", "url_invalid"},
		{"bad port", "https://hooks.example.com:99999/q", "url_invalid"},
		{"too long", "https://hooks.example.com/" + strings.Repeat("a", 2100), "url_too_long"},
	}
	for _, tc := range cases {
		got := createSub(t, h, "org_a", tc.url, []string{"change_detected"})
		field, reason := errField(got)
		if got.Code != http.StatusBadRequest || field != "url" || reason != tc.reason {
			t.Errorf("%s: %d field=%q reason=%q, want 400 url %s: %s", tc.name, got.Code, field, reason, tc.reason, got.Raw)
		}
	}
	ok := createSub(t, h, "org_a", "https://hooks.example.com:8443/q?x=1", []string{"change_detected"})
	if ok.Code != http.StatusCreated {
		t.Fatalf("valid https url = %d: %s", ok.Code, ok.Raw)
	}
}

func TestSubscriptions_EventsAllowList(t *testing.T) {
	t.Parallel()
	h, _ := newSubServer(t, 0)
	for name, events := range map[string][]string{
		"star":        {"*"},
		"run event":   {"run_completed"},
		"mixed":       {"change_detected", "page_fetched"},
		"dotted name": {"quarry.change.detected"},
	} {
		got := createSub(t, h, "org_a", "https://hooks.example.com/q", events)
		if field, reason := errField(got); got.Code != 400 || field != "events" || reason != "events_unsupported" {
			t.Errorf("%s: %d %s %s", name, got.Code, field, reason)
		}
	}
	empty := createSub(t, h, "org_a", "https://hooks.example.com/q", nil)
	if _, reason := errField(empty); empty.Code != 400 || reason != "events_empty" {
		t.Errorf("empty events: %d %s", empty.Code, empty.Raw)
	}
	// Duplicates collapse; the stored list is canonical.
	dup := createSub(t, h, "org_a", "https://hooks.example.com/q", []string{"change_detected", "change_detected"})
	if evs, _ := dup.Data["events"].([]any); dup.Code != 201 || len(evs) != 1 {
		t.Errorf("duplicate events: %d %s", dup.Code, dup.Raw)
	}
	long := subDo(t, h, http.MethodPost, "/v1/webhook-subscriptions?org_id=org_a",
		map[string]any{"url": "https://hooks.example.com/z", "events": []string{"change_detected"}, "description": strings.Repeat("x", 201)})
	if _, reason := errField(long); long.Code != 400 || reason != "description_too_long" {
		t.Errorf("long description: %d %s", long.Code, long.Raw)
	}
}

func TestSubscriptions_CapAndDuplicate(t *testing.T) {
	t.Parallel()
	h, _ := newSubServer(t, 2)
	if r := createSub(t, h, "org_a", "https://hooks.example.com/1", []string{"change_detected"}); r.Code != 201 {
		t.Fatal(r.Raw)
	}
	dup := createSub(t, h, "org_a", "https://hooks.example.com/1", []string{"change_detected"})
	if dup.Code != http.StatusConflict {
		t.Fatalf("duplicate = %d: %s", dup.Code, dup.Raw)
	}
	if r := createSub(t, h, "org_a", "https://hooks.example.com/2", []string{"change_detected"}); r.Code != 201 {
		t.Fatal(r.Raw)
	}
	full := createSub(t, h, "org_a", "https://hooks.example.com/3", []string{"change_detected"})
	if full.Code != http.StatusTooManyRequests {
		t.Fatalf("over the cap = %d: %s", full.Code, full.Raw)
	}
	if d, _ := full.Err["details"].(map[string]any); d["reason"] != "subscription_limit" {
		t.Fatalf("cap details = %v", full.Err)
	}
	// The cap is per org.
	if r := createSub(t, h, "org_b", "https://hooks.example.com/3", []string{"change_detected"}); r.Code != 201 {
		t.Fatalf("another org at the cap of org_a: %d %s", r.Code, r.Raw)
	}
}

func TestSubscriptions_PatchRotateDelete(t *testing.T) {
	t.Parallel()
	h, db := newSubServer(t, 0)
	created := createSub(t, h, "org_a", "https://hooks.example.com/q", []string{"change_detected"})
	id := created.Data["id"].(string)
	first := created.Data["secret"].(string)

	rot := subDo(t, h, http.MethodPost, "/v1/webhook-subscriptions/"+id+"/rotate-secret?org_id=org_a", nil)
	second, _ := rot.Data["secret"].(string)
	if rot.Code != 200 || second == "" || second == first || !strings.HasPrefix(second, "whsec_") {
		t.Fatalf("rotate = %d %s", rot.Code, rot.Raw)
	}
	if stored, _ := db.Webhooks().Get(quarrycontracts.ID(id)); stored.Secret != second {
		t.Fatal("rotation did not store the new secret")
	}
	got := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", nil)
	if got.Data["secret_hint"] != "whsec_..."+second[len(second)-4:] {
		t.Fatalf("hint after rotate = %v", got.Data["secret_hint"])
	}

	bad := subDo(t, h, http.MethodPatch, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", map[string]any{"events": []string{"nope"}})
	if bad.Code != 400 {
		t.Fatalf("patch bad events = %d", bad.Code)
	}
	off := subDo(t, h, http.MethodPatch, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", map[string]any{"active": false, "description": "paused"})
	if off.Code != 200 || off.Data["active"] != false || off.Data["description"] != "paused" {
		t.Fatalf("patch = %d %s", off.Code, off.Raw)
	}

	if del := subDo(t, h, http.MethodDelete, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", nil); del.Code != http.StatusNoContent {
		t.Fatalf("delete = %d", del.Code)
	}
	if again := subDo(t, h, http.MethodDelete, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", nil); again.Code != http.StatusNotFound {
		t.Fatalf("second delete = %d", again.Code)
	}
}

func TestSubscriptions_ListPaginates(t *testing.T) {
	t.Parallel()
	h, _ := newSubServer(t, 0)
	for _, p := range []string{"1", "2", "3"} {
		if r := createSub(t, h, "org_a", "https://hooks.example.com/"+p, []string{"change_detected"}); r.Code != 201 {
			t.Fatal(r.Raw)
		}
	}
	p1 := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions?org_id=org_a&limit=2", nil)
	items, _ := p1.Data["items"].([]any)
	next, _ := p1.Data["next_cursor"].(string)
	if len(items) != 2 || next == "" {
		t.Fatalf("page 1 = %s", p1.Raw)
	}
	p2 := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions?org_id=org_a&limit=2&cursor="+next, nil)
	rest, _ := p2.Data["items"].([]any)
	if len(rest) != 1 || p2.Data["next_cursor"] != nil {
		t.Fatalf("page 2 = %s", p2.Raw)
	}
}

func TestSubscriptions_DeliveriesAndRetry(t *testing.T) {
	t.Parallel()
	h, db := newSubServer(t, 0)
	id := createSub(t, h, "org_a", "https://hooks.example.com/q", []string{"change_detected"}).Data["id"].(string)
	other := createSub(t, h, "org_b", "https://hooks.example.com/q", []string{"change_detected"}).Data["id"].(string)

	mk := func(org, wh, st, lastErr string, at int64) quarrycontracts.ID {
		d := store.WebhookDelivery{
			ID: quarrycontracts.NewID(quarrycontracts.KindWebhookDelivery), WebhookID: quarrycontracts.ID(wh), OrgID: org,
			EventID: quarrycontracts.NewID(quarrycontracts.KindEvent), EventType: "change_detected",
			Payload: `{"secret":"page content"}`, Status: st, Attempt: 3, LastError: lastErr, NextAttemptAt: at, CreatedAt: at,
		}
		if err := db.WebhookDeliveries().Create(d); err != nil {
			t.Fatal(err)
		}
		return d.ID
	}
	now := time.Now().Unix()
	dlq := mk("org_a", id, "dlq", "http 410", now-30)
	_ = mk("org_a", id, "success", "", now-20)
	pending := mk("org_a", id, "pending", "transport: dial tcp 10.0.0.1:443: connect: refused", now-10)
	foreign := mk("org_b", other, "dlq", "http 500", now-5)

	all := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions/"+id+"/deliveries?org_id=org_a", nil)
	items, _ := all.Data["items"].([]any)
	if len(items) != 3 {
		t.Fatalf("deliveries = %s", all.Raw)
	}
	if strings.Contains(all.Raw, "page content") || strings.Contains(all.Raw, "10.0.0.1") || strings.Contains(all.Raw, "payload") {
		t.Fatalf("delivery list leaks payload or network detail: %s", all.Raw)
	}
	first := items[0].(map[string]any) // newest first: the pending one
	if first["id"] != string(pending) || first["last_error"] != "transport" || first["next_attempt_at"] == nil {
		t.Fatalf("newest = %v", first)
	}
	only := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions/"+id+"/deliveries?org_id=org_a&status=dlq", nil)
	if got, _ := only.Data["items"].([]any); len(got) != 1 || got[0].(map[string]any)["last_error"] != "http_410" {
		t.Fatalf("status filter = %s", only.Raw)
	}
	if bad := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions/"+id+"/deliveries?org_id=org_a&status=weird", nil); bad.Code != 400 {
		t.Fatalf("bad status = %d", bad.Code)
	}

	// The last delivery shows on the subscription.
	sub := subDo(t, h, http.MethodGet, "/v1/webhook-subscriptions/"+id+"?org_id=org_a", nil)
	if ld, _ := sub.Data["last_delivery"].(map[string]any); ld["status"] != "pending" {
		t.Fatalf("last_delivery = %v", sub.Data["last_delivery"])
	}

	// Retry: a dlq delivery goes back to pending; a pending one is a 409; a foreign one is a 404.
	retried := subDo(t, h, http.MethodPost, "/v1/webhook-subscriptions/"+id+"/deliveries/"+string(dlq)+"/retry?org_id=org_a", nil)
	if retried.Code != http.StatusAccepted || retried.Data["status"] != "pending" {
		t.Fatalf("retry dlq = %d %s", retried.Code, retried.Raw)
	}
	if d, _ := db.WebhookDeliveries().Get(dlq); d.Status != "pending" || d.Attempt != 0 || d.LastError != "" {
		t.Fatalf("stored after retry = %+v", d)
	}
	if again := subDo(t, h, http.MethodPost, "/v1/webhook-subscriptions/"+id+"/deliveries/"+string(pending)+"/retry?org_id=org_a", nil); again.Code != http.StatusConflict {
		t.Fatalf("retry pending = %d", again.Code)
	}
	if fr := subDo(t, h, http.MethodPost, "/v1/webhook-subscriptions/"+id+"/deliveries/"+string(foreign)+"/retry?org_id=org_a", nil); fr.Code != http.StatusNotFound {
		t.Fatalf("retry foreign delivery = %d", fr.Code)
	}
	if d, _ := db.WebhookDeliveries().Get(foreign); d.Status != "dlq" {
		t.Fatal("a foreign delivery was re-queued")
	}
}

func TestDeliveryErrorToken(t *testing.T) {
	t.Parallel()
	for in, want := range map[string]string{
		"":         "",
		"http 500": "http_500",
		"http 410": "http_410",
		"http x":   "error",
		"transport: Post \"https://h\": egress: destination blocked by policy: h resolved to 10.0.0.1 (private)": "blocked_target",
		"transport: Post \"https://h\": context deadline exceeded":                                               "timeout",
		"transport: dial tcp 1.2.3.4:443: connect: refused":                                                      "transport",
		"webhook missing or inactive":                                                                            "inactive",
		"webhook org mismatch":                                                                                   "inactive",
		"request build: parse":                                                                                   "error",
	} {
		if got := DeliveryErrorToken(in); got != want {
			t.Errorf("DeliveryErrorToken(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestWebhookSubscriptionsEnabled_DefaultOff(t *testing.T) {
	t.Parallel()
	env := func(v string) func(string) string {
		return func(k string) string {
			if k == "QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS" {
				return v
			}
			return ""
		}
	}
	for v, want := range map[string]bool{"": false, "0": false, "false": false, "off": false, "maybe": false, "1": true, "true": true, "ON": true, " yes ": true} {
		if got := WebhookSubscriptionsEnabled(env(v)); got != want {
			t.Errorf("flag %q = %v, want %v", v, got, want)
		}
	}
}

func TestLegacyWebhookRoutesAreGone(t *testing.T) {
	t.Parallel()
	h, db := newChangeWebhookServerWithGone(t)
	// A legacy row must not be readable through the old route.
	_ = db.Webhooks().Create(store.Webhook{ID: "whk_legacy", URL: "https://x", Secret: "s3cret", Active: true})
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, "/v1/webhooks"},
		{http.MethodPost, "/v1/webhooks"},
		{http.MethodGet, "/v1/webhooks/whk_legacy"},
		{http.MethodDelete, "/v1/webhooks/whk_legacy"},
		{http.MethodGet, "/v1/webhook-deliveries"},
		{http.MethodGet, "/v1/webhook-deliveries/whkd_1"},
		{http.MethodPost, "/v1/webhook-deliveries/whkd_1/retry"},
	} {
		w := httptest.NewRecorder()
		h.ServeHTTP(w, httptest.NewRequest(tc.method, tc.path, nil))
		if w.Code != http.StatusGone || strings.Contains(w.Body.String(), "s3cret") {
			t.Errorf("%s %s = %d (%s), want 410", tc.method, tc.path, w.Code, w.Body.String())
		}
	}
	// The edge's signed receiver still answers on its own handler.
	code, _ := doChange(t, h, signedChangePost(t, "o1", changedBody, "idem-gone", 0, "nonce-gone"))
	if code != http.StatusOK {
		t.Fatalf("POST /v1/webhooks/change after the 410 mount = %d, want 200", code)
	}
}
