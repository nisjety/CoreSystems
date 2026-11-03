package resources

import (
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"
	"testing"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/egress"
)

// W3-E3.5-3: per-job webhooks.

// jobHookVetter resolves nothing: hosts named "blocked.example" are refused as a private
// address, "nxdomain.example" as unresolvable, every other host passes.
func jobHookVetter(_ context.Context, host string) error {
	switch host {
	case "blocked.example":
		return egress.ErrBlocked
	case "nxdomain.example":
		return fmt.Errorf("no such host")
	}
	return nil
}

func decodeJSON(t *testing.T, raw string) any {
	t.Helper()
	var v any
	if err := json.Unmarshal([]byte(raw), &v); err != nil {
		t.Fatalf("decode %s: %v", raw, err)
	}
	return v
}

func TestNormalizeJobWebhookAcceptsAnObjectAndABareURL(t *testing.T) {
	ctx := context.Background()
	hook, field, reason, _ := NormalizeJobWebhook(ctx,
		decodeJSON(t, `{"url":" https://hooks.example.com/q ","events":["run_completed","page_fetched","run_completed"]}`),
		false, jobHookVetter)
	if reason != "" {
		t.Fatalf("object refused: %s %s", field, reason)
	}
	if hook.URL != "https://hooks.example.com/q" {
		t.Fatalf("url = %q", hook.URL)
	}
	if !reflect.DeepEqual(hook.Events, []string{"page_fetched", "run_completed"}) {
		t.Fatalf("events = %v", hook.Events)
	}

	bare, _, reason, _ := NormalizeJobWebhook(ctx, "https://hooks.example.com/q", false, jobHookVetter)
	if reason != "" {
		t.Fatalf("bare url refused: %s", reason)
	}
	if !reflect.DeepEqual(bare.Events, DefaultJobWebhookEvents) {
		t.Fatalf("default events = %v", bare.Events)
	}
	// The default slice is copied, not shared.
	bare.Events[0] = "mutated"
	if DefaultJobWebhookEvents[0] == "mutated" {
		t.Fatal("DefaultJobWebhookEvents was aliased")
	}
}

func TestNormalizeJobWebhookRefusals(t *testing.T) {
	ctx := context.Background()
	cases := []struct {
		name   string
		raw    string
		zdr    bool
		field  string
		reason string
	}{
		{"zdr", `"https://hooks.example.com/q"`, true, "webhook", "zdr_refused"},
		{"wrong type", `7`, false, "webhook", "invalid_type"},
		{"http", `{"url":"http://hooks.example.com/q"}`, false, "webhook.url", "url_not_https"},
		{"bare http", `"http://hooks.example.com/q"`, false, "webhook", "url_not_https"},
		{"userinfo", `{"url":"https://u:p@hooks.example.com/q"}`, false, "webhook.url", "url_userinfo"},
		{"fragment", `{"url":"https://hooks.example.com/q#x"}`, false, "webhook.url", "url_fragment"},
		{"private", `{"url":"https://blocked.example/q"}`, false, "webhook.url", "url_private_address"},
		{"unresolvable", `{"url":"https://nxdomain.example/q"}`, false, "webhook.url", "url_unresolvable"},
		{"no url", `{"events":["run_completed"]}`, false, "webhook.url", "url_invalid"},
		{"url not string", `{"url":5}`, false, "webhook.url", "invalid_type"},
		{"headers", `{"url":"https://hooks.example.com/q","headers":{"a":"b"}}`, false, "webhook.headers", "reserved_field"},
		{"unknown key", `{"url":"https://hooks.example.com/q","secret":"s"}`, false, "webhook.secret", "unknown_field"},
		{"empty events", `{"url":"https://hooks.example.com/q","events":[]}`, false, "webhook.events", "events_empty"},
		{"unknown event", `{"url":"https://hooks.example.com/q","events":["change_detected"]}`, false, "webhook.events", "events_unsupported"},
		{"events not list", `{"url":"https://hooks.example.com/q","events":"run_completed"}`, false, "webhook.events", "invalid_type"},
		{"event not string", `{"url":"https://hooks.example.com/q","events":[1]}`, false, "webhook.events", "invalid_type"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			_, field, reason, msg := NormalizeJobWebhook(ctx, decodeJSON(t, tc.raw), tc.zdr, jobHookVetter)
			if field != tc.field || reason != tc.reason {
				t.Fatalf("got (%s, %s), want (%s, %s)", field, reason, tc.field, tc.reason)
			}
			if msg == "" {
				t.Fatal("a refusal carries a message")
			}
		})
	}
}

func TestJobWebhookParamsRoundTrip(t *testing.T) {
	hook := JobWebhook{URL: "https://hooks.example.com/q", Events: []string{"run_completed", "run_failed"}}
	// Through JSON, as the job params are stored.
	raw, err := json.Marshal(map[string]any{JobWebhookParamKey: hook.ParamsValue(), "url": "https://e.test/"})
	if err != nil {
		t.Fatal(err)
	}
	var params map[string]any
	if err := json.Unmarshal(raw, &params); err != nil {
		t.Fatal(err)
	}
	got, ok := JobWebhookFromParams(params)
	if !ok || !reflect.DeepEqual(got, hook) {
		t.Fatalf("round trip = %+v, %v", got, ok)
	}
	if !got.Wants("run_failed") || got.Wants("run_started") {
		t.Fatal("Wants does not follow the events")
	}
	// In memory, before any JSON round trip.
	if got, ok := JobWebhookFromParams(map[string]any{JobWebhookParamKey: map[string]any{"url": "https://h.example/", "events": []string{"run_completed"}}}); !ok || got.Events[0] != "run_completed" {
		t.Fatalf("[]string events = %+v, %v", got, ok)
	}
	for _, params := range []map[string]any{
		nil,
		{},
		{JobWebhookParamKey: "https://h.example/"},
		{JobWebhookParamKey: map[string]any{"events": []any{"run_completed"}}},
		{JobWebhookParamKey: map[string]any{"url": "https://h.example/", "events": []any{"change_detected"}}},
	} {
		if _, ok := JobWebhookFromParams(params); ok {
			t.Fatalf("malformed params accepted: %v", params)
		}
	}
}

func TestJobWebhooksEnabledDefaultsOff(t *testing.T) {
	for value, want := range map[string]bool{"": false, "off": false, "nope": false, "on": true, " TRUE ": true, "1": true, "yes": true} {
		got := JobWebhooksEnabled(func(name string) string {
			if name == "QUARRY_CONTROL__JOB_WEBHOOK" {
				return value
			}
			return ""
		})
		if got != want {
			t.Fatalf("%q: got %v", value, got)
		}
	}
}

func TestJobWebhookKeyAndSecret(t *testing.T) {
	for _, raw := range []string{"", "zz", strings.Repeat("ab", 31)} {
		if _, err := JobWebhookKey(func(string) string { return raw }); err == nil {
			t.Fatalf("key of %d chars accepted", len(raw))
		}
	}
	key, err := JobWebhookKey(func(string) string { return strings.Repeat("ab", 32) })
	if err != nil {
		t.Fatal(err)
	}
	a, err := JobWebhookSecret(key, "org_a", "job_1")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(a, "whjob_") || len(a) != len("whjob_")+64 {
		t.Fatalf("secret shape %q", a[:6])
	}
	again, _ := JobWebhookSecret(key, "org_a", "job_1")
	if a != again {
		t.Fatal("the derivation is not deterministic")
	}
	for _, other := range [][2]string{{"org_b", "job_1"}, {"org_a", "job_2"}, {"org_aj", "ob_1"}} {
		b, err := JobWebhookSecret(key, other[0], other[1])
		if err != nil || b == a {
			t.Fatalf("secret not bound to %v", other)
		}
	}
	if _, err := JobWebhookSecret(key, "", "job_1"); err == nil {
		t.Fatal("empty org accepted")
	}
	if _, err := JobWebhookSecret(key[:16], "org_a", "job_1"); err == nil {
		t.Fatal("short key accepted")
	}
}

func TestSignWebhookBodyMatchesTheDispatcherFormat(t *testing.T) {
	body := []byte(`{"type":"run_completed"}`)
	got := SignWebhookBody("whjob_secret", 1700000000, body)
	mac := hmac.New(sha256.New, []byte("whjob_secret"))
	mac.Write([]byte("1700000000."))
	mac.Write(body)
	want := "t=1700000000,v1=" + hex.EncodeToString(mac.Sum(nil))
	if got != want {
		t.Fatalf("got %s, want %s", got, want)
	}
}
