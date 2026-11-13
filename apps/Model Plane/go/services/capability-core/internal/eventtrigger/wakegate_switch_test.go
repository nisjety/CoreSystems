package eventtrigger

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
)

// Plan 3.4 for EVENT_TRIGGER_WAKE_GATE_ENABLED (W4-F T5): off (no EnableWakeGate), a stored wake filter is ignored and a
// fire is decided exactly as a fire of a trigger with no filter. On, a filtered fire reads its payload before Control is
// asked: a closed gate refuses it without asking Control, an unreadable payload leaves it pending and then refuses it, and
// a fire with no filter never reads a payload.

// neverMatchingFilter lists an event type no event carries, so the gate always closes for it.
var neverMatchingFilter = json.RawMessage(`{"version":1,"event_types":["never.sent"]}`)

// fakeEventSource is the integration-corev2 read the gate makes. It counts reads and answers event, or err.
type fakeEventSource struct {
	reads int
	event WebhookEventRecord
	err   error
}

func (s *fakeEventSource) FetchWebhookEvent(_ context.Context, orgID, webhookEventID string) (WebhookEventRecord, error) {
	s.reads++
	if orgID != testOrg {
		return WebhookEventRecord{}, errors.New("read for the wrong org")
	}
	return s.event, s.err
}

func filteredRow(filter json.RawMessage) memRow {
	row := pendingRow("trgfire_1", "trg_1", "wh-1")
	row.fire.WakeFilter = filter
	return row
}

func TestWakeGateOffIgnoresAStoredFilterAndDecidesAsBefore(t *testing.T) {
	plain := &memLedger{rows: []memRow{pendingRow("trgfire_1", "trg_1", "wh-1")}}
	plainAuthority := &fakeAuthorizer{}
	if _, err := newTestSweeper(plain, plainAuthority, testLimits).RunOnce(context.Background()); err != nil {
		t.Fatal(err)
	}

	filtered := &memLedger{rows: []memRow{filteredRow(neverMatchingFilter)}}
	filteredAuthority := &fakeAuthorizer{}
	authorized, err := newTestSweeper(filtered, filteredAuthority, testLimits).RunOnce(context.Background())
	if err != nil || authorized != 1 {
		t.Fatalf("RunOnce with the gate off = %d, %v; want 1, nil", authorized, err)
	}
	got, want := filtered.rows[0], plain.rows[0]
	if got.status != want.status || got.outcome != want.outcome || got.attempts != want.attempts || !got.settledAt.Equal(want.settledAt) {
		t.Fatalf("gate off: filtered fire settled %+v; a fire with no filter settled %+v", got, want)
	}
	if len(filteredAuthority.intents) != 1 || filteredAuthority.intents[0] != plainAuthority.intents[0] {
		t.Fatalf("gate off: Control intent %+v differs from the unfiltered fire's %+v", filteredAuthority.intents, plainAuthority.intents)
	}
}

func TestWakeGateOnRefusesANonMatchingFireWithoutAskingControl(t *testing.T) {
	source := &fakeEventSource{event: WebhookEventRecord{ProviderKey: "github", EventType: "push", ConnectionID: "conn-1"}}
	mem := &memLedger{rows: []memRow{filteredRow(neverMatchingFilter)}}
	authority := &fakeAuthorizer{}
	sweeper := newTestSweeper(mem, authority, testLimits)
	sweeper.EnableWakeGate(source)
	authorized, err := sweeper.RunOnce(context.Background())
	if err != nil || authorized != 0 {
		t.Fatalf("RunOnce = %d, %v; want 0, nil", authorized, err)
	}
	row := mem.rows[0]
	if row.status != StatusRefused || row.outcome != OutcomeWakeGateClosed || row.attempts != 0 {
		t.Fatalf("row = %+v; want refused wake_gate_closed with no attempt spent", row)
	}
	if len(authority.intents) != 0 {
		t.Fatal("Control was asked about a fire the wake gate closed")
	}
	if source.reads != 1 {
		t.Fatalf("payload reads = %d; want 1", source.reads)
	}
}

func TestWakeGateOnAuthorisesAMatchingFireThroughControl(t *testing.T) {
	source := &fakeEventSource{event: WebhookEventRecord{ProviderKey: "github", EventType: "push", ConnectionID: "conn-1",
		Payload: map[string]any{"repository": map[string]any{"name": "core"}}}}
	filter := json.RawMessage(`{"version":1,"event_types":["push"],"providers":["github"],"field_equals":[{"path":"repository.name","value":"core"}]}`)
	mem := &memLedger{rows: []memRow{filteredRow(filter)}}
	authority := &fakeAuthorizer{}
	sweeper := newTestSweeper(mem, authority, testLimits)
	sweeper.EnableWakeGate(source)
	authorized, err := sweeper.RunOnce(context.Background())
	if err != nil || authorized != 1 {
		t.Fatalf("RunOnce = %d, %v; want 1, nil", authorized, err)
	}
	if row := mem.rows[0]; row.status != StatusAuthorized || row.attempts != 1 {
		t.Fatalf("row = %+v; want authorized after one Control attempt", row)
	}
	if len(authority.intents) != 1 {
		t.Fatalf("Control asked %d times; want 1 (a passing gate grants nothing by itself)", len(authority.intents))
	}
}

func TestWakeGateOnUnreadablePayloadStaysPendingThenRefuses(t *testing.T) {
	source := &fakeEventSource{err: errors.New("integration-corev2 answered 503")}
	mem := &memLedger{rows: []memRow{filteredRow(neverMatchingFilter)}}
	authority := &fakeAuthorizer{}
	sweeper := newTestSweeper(mem, authority, testLimits)
	sweeper.EnableWakeGate(source)
	for attempt := 1; attempt < testLimits.MaxAttempts; attempt++ {
		if _, err := sweeper.RunOnce(context.Background()); err != nil {
			t.Fatal(err)
		}
		if row := mem.rows[0]; row.status != StatusPending || row.attempts != attempt {
			t.Fatalf("after sweep %d row = %+v; want pending with %d attempts", attempt, row, attempt)
		}
	}
	if _, err := sweeper.RunOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	if row := mem.rows[0]; row.status != StatusRefused || row.outcome != OutcomeWakePayloadUnavailable || row.attempts != testLimits.MaxAttempts {
		t.Fatalf("row = %+v; want refused wake_payload_unavailable once the attempts are spent", row)
	}
	if len(authority.intents) != 0 {
		t.Fatal("Control was asked about a fire whose payload could not be read")
	}
}

func TestWakeGateOnSkipsTheReadForAFireWithNoFilterAndClosesAMalformedFilterWithoutReading(t *testing.T) {
	source := &fakeEventSource{}
	mem := &memLedger{rows: []memRow{
		pendingRow("trgfire_1", "trg_1", "wh-1"),
		filteredRow(json.RawMessage(`{"version":2,"event_types":["push"]}`)),
	}}
	mem.rows[1].fire.ID, mem.rows[1].fire.TriggerID, mem.rows[1].fire.WebhookEventID = "trgfire_2", "trg_2", "wh-2"
	authority := &fakeAuthorizer{}
	sweeper := newTestSweeper(mem, authority, testLimits)
	sweeper.EnableWakeGate(source)
	if _, err := sweeper.RunOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	if source.reads != 0 {
		t.Fatalf("payload reads = %d; want 0 (no filter, and a filter outside the schema, read nothing)", source.reads)
	}
	if mem.rows[0].status != StatusAuthorized {
		t.Fatalf("unfiltered fire = %+v; want authorized", mem.rows[0])
	}
	if row := mem.rows[1]; row.status != StatusRefused || row.outcome != OutcomeWakeGateClosed {
		t.Fatalf("malformed-filter fire = %+v; want refused wake_gate_closed", row)
	}
	if len(authority.intents) != 1 {
		t.Fatalf("Control asked %d times; want 1", len(authority.intents))
	}
}

func TestWakeGateOnWithANilSourceFailsClosed(t *testing.T) {
	mem := &memLedger{rows: []memRow{filteredRow(neverMatchingFilter)}}
	authority := &fakeAuthorizer{}
	sweeper := newTestSweeper(mem, authority, testLimits)
	sweeper.EnableWakeGate(nil)
	if _, err := sweeper.RunOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	if row := mem.rows[0]; row.status != StatusPending || row.attempts != 1 {
		t.Fatalf("row = %+v; want pending (fail closed), one attempt", row)
	}
	if len(authority.intents) != 0 {
		t.Fatal("Control was asked with no payload source")
	}
}

func TestEvaluateTheBodyCannotOverrideTheEventsOwnTypeOrProvider(t *testing.T) {
	filter, err := ParseWakeFilter(json.RawMessage(`{"version":1,"event_types":["push"],"providers":["github"]}`))
	if err != nil {
		t.Fatal(err)
	}
	record := WebhookEventRecord{ProviderKey: "gitlab", EventType: "issue",
		Payload: map[string]any{"eventType": "push", "providerKey": "github"}}
	if open, reason := Evaluate(filter, record.view()); open {
		t.Fatalf("a body naming eventType/providerKey opened the gate (%s)", reason)
	}
}

func TestIntegrationWebhookSourceSendsTheKeyOnlyToItsOwnHost(t *testing.T) {
	var elsewhere atomic.Int32
	other := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		elsewhere.Add(1)
		w.WriteHeader(http.StatusOK)
	}))
	defer other.Close()
	var seenKey, seenPath, seenOrg string
	own := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		seenKey, seenPath, seenOrg = r.Header.Get("X-Internal-API-Key"), r.URL.EscapedPath(), r.URL.Query().Get("organizationId")
		if strings.HasSuffix(r.URL.Path, "/redirect") {
			http.Redirect(w, r, other.URL+"/steal", http.StatusFound)
			return
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"webhookEvent":{"providerKey":"github","eventType":"push","connectionId":"conn-1","payload":{"a":"b"}}}}`))
	}))
	defer own.Close()

	source := NewIntegrationWebhookSource(own.URL+"/", " internal-key ")
	event, err := source.FetchWebhookEvent(context.Background(), "org a", "wh/1")
	if err != nil {
		t.Fatal(err)
	}
	if event.EventType != "push" || event.ConnectionID != "conn-1" {
		t.Fatalf("event = %+v", event)
	}
	if seenKey != "internal-key" || seenPath != "/internal/webhooks/events/wh%2F1" || seenOrg != "org a" {
		t.Fatalf("request key=%q path=%q org=%q", seenKey, seenPath, seenOrg)
	}

	if _, err := source.FetchWebhookEvent(context.Background(), testOrg, "redirect"); err == nil {
		t.Fatal("a redirect answer was accepted as an event")
	}
	if elsewhere.Load() != 0 {
		t.Fatal("the internal key followed a redirect to another host")
	}

	blank := NewIntegrationWebhookSource("", "key")
	if blank.Configured() {
		t.Fatal("a source with no URL reports configured")
	}
	if _, err := blank.FetchWebhookEvent(context.Background(), testOrg, "wh-1"); err == nil {
		t.Fatal("an unconfigured source read an event")
	}
}
