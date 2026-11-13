package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/eventtrigger"
)

// W4-F T6 (plan 3.4 for EVENT_TRIGGER_WAKE_GATE_ENABLED): the dry run is mounted only while the switch is on, answers only
// the trigger's own member in the trigger's org, refuses a ZDR or posture-less caller before reading anything, answers an
// event of another connection as a missing trigger, and returns a verdict with no payload content.

type dryRunTriggerRow struct {
	member, connection string
	filter             json.RawMessage
}

// dryRunDB answers the one trigger read, keyed by (trigger id, org id). It counts reads.
type dryRunDB struct {
	rows  map[[2]string]dryRunTriggerRow
	reads int
}

func (db *dryRunDB) QueryRow(_ context.Context, _ string, args ...any) pgx.Row {
	db.reads++
	row, ok := db.rows[[2]string{args[0].(string), args[1].(string)}]
	return dryRunRow{row: row, ok: ok}
}

type dryRunRow struct {
	row dryRunTriggerRow
	ok  bool
}

func (r dryRunRow) Scan(dest ...any) error {
	if !r.ok {
		return pgx.ErrNoRows
	}
	*dest[0].(*string) = r.row.member
	*dest[1].(*string) = r.row.connection
	*dest[2].(*json.RawMessage) = r.row.filter
	return nil
}

// dryRunSource is the integration-corev2 read. It counts reads and refuses an org it was not given.
type dryRunSource struct {
	event eventtrigger.WebhookEventRecord
	err   error
	reads int
	orgs  []string
}

func (s *dryRunSource) FetchWebhookEvent(_ context.Context, orgID, _ string) (eventtrigger.WebhookEventRecord, error) {
	s.reads++
	s.orgs = append(s.orgs, orgID)
	return s.event, s.err
}

const dryRunSecretValue = "PAYLOAD-SECRET-VALUE"

func dryRunFixture() (*dryRunDB, *dryRunSource) {
	db := &dryRunDB{rows: map[[2]string]dryRunTriggerRow{
		{"trg_1", "org-1"}: {member: "user-1", connection: "conn-1",
			filter: json.RawMessage(`{"version":1,"event_types":["push"],"field_equals":[{"path":"repo","value":"core"}]}`)},
	}}
	source := &dryRunSource{event: eventtrigger.WebhookEventRecord{ProviderKey: "github", EventType: "push",
		ConnectionID: "conn-1", Payload: map[string]any{"repo": "core", "secret": dryRunSecretValue}}}
	return db, source
}

func dryRunPrincipal() authctx.Principal {
	return authctx.Principal{OrganizationID: "org-1", ActorID: "user-1", PrincipalType: "user", RetentionPolicyPresent: true}
}

func serveDryRun(t *testing.T, db *dryRunDB, source *dryRunSource, principal *authctx.Principal, path, body string) *httptest.ResponseRecorder {
	t.Helper()
	mux := http.NewServeMux()
	NewEventTriggerDryRunHandler(db, source).Register(mux)
	request := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
	if principal != nil {
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), *principal))
	}
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, request)
	return recorder
}

func dryRunCode(t *testing.T, recorder *httptest.ResponseRecorder) string {
	t.Helper()
	var envelope struct {
		Error struct {
			Code string `json:"code"`
		} `json:"error"`
	}
	_ = json.Unmarshal(recorder.Body.Bytes(), &envelope)
	return envelope.Error.Code
}

const dryRunPath = "/api/v1/event-triggers/trg_1/dry-run"
const dryRunBody = `{"webhook_event_id":"wh-1"}`

func TestEventTriggerDryRunAnswersTheMembersVerdictWithoutPayload(t *testing.T) {
	db, source := dryRunFixture()
	principal := dryRunPrincipal()
	recorder := serveDryRun(t, db, source, &principal, dryRunPath, dryRunBody)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d body %s", recorder.Code, recorder.Body)
	}
	var envelope struct {
		Data struct {
			WouldWake bool     `json:"would_wake"`
			Reason    string   `json:"reason"`
			Matched   []string `json:"matched"`
		} `json:"data"`
	}
	if err := json.Unmarshal(recorder.Body.Bytes(), &envelope); err != nil {
		t.Fatal(err)
	}
	if !envelope.Data.WouldWake || envelope.Data.Reason != "matched" {
		t.Fatalf("data = %+v", envelope.Data)
	}
	for _, leaked := range []string{dryRunSecretValue, "core", "conn-1", "github"} {
		if strings.Contains(recorder.Body.String(), leaked) {
			t.Fatalf("the dry run answer carries event content %q: %s", leaked, recorder.Body)
		}
	}
	if len(source.orgs) != 1 || source.orgs[0] != "org-1" {
		t.Fatalf("event read for orgs %v; want the caller's org only", source.orgs)
	}
}

func TestEventTriggerDryRunRefusesBeforeReadingAnything(t *testing.T) {
	cases := map[string]struct {
		principal *authctx.Principal
		status    int
		code      string
	}{
		"no principal": {nil, http.StatusForbidden, "not_authorized"},
		"zero data retention": {func() *authctx.Principal {
			p := dryRunPrincipal()
			p.ZeroDataRetention = true
			return &p
		}(), http.StatusForbidden, "event_trigger_dry_run_under_zdr_refused"},
		"no stated posture": {func() *authctx.Principal {
			p := dryRunPrincipal()
			p.RetentionPolicyPresent = false
			return &p
		}(), http.StatusForbidden, "event_trigger_dry_run_under_zdr_refused"},
		"service principal": {func() *authctx.Principal {
			p := dryRunPrincipal()
			p.PrincipalType = "service"
			return &p
		}(), http.StatusForbidden, "event_trigger_member_only"},
		"api key principal": {func() *authctx.Principal {
			p := dryRunPrincipal()
			p.PrincipalType = "api_key"
			p.KeyID = "key-1"
			return &p
		}(), http.StatusForbidden, "event_trigger_member_only"},
	}
	for name, tc := range cases {
		t.Run(name, func(t *testing.T) {
			db, source := dryRunFixture()
			recorder := serveDryRun(t, db, source, tc.principal, dryRunPath, dryRunBody)
			if recorder.Code != tc.status || dryRunCode(t, recorder) != tc.code {
				t.Fatalf("got %d %q; want %d %q", recorder.Code, dryRunCode(t, recorder), tc.status, tc.code)
			}
			if db.reads != 0 || source.reads != 0 {
				t.Fatalf("read trigger %d times and event %d times before refusing", db.reads, source.reads)
			}
		})
	}
}

func TestEventTriggerDryRunAnswersNotFoundForAnotherMembersOrgsOrConnectionsRecord(t *testing.T) {
	t.Run("another member's trigger", func(t *testing.T) {
		db, source := dryRunFixture()
		principal := dryRunPrincipal()
		principal.ActorID = "user-2"
		recorder := serveDryRun(t, db, source, &principal, dryRunPath, dryRunBody)
		if recorder.Code != http.StatusNotFound || dryRunCode(t, recorder) != "event_trigger_not_found" || source.reads != 0 {
			t.Fatalf("got %d %q, event reads %d", recorder.Code, dryRunCode(t, recorder), source.reads)
		}
	})
	t.Run("another org's trigger", func(t *testing.T) {
		db, source := dryRunFixture()
		principal := dryRunPrincipal()
		principal.OrganizationID = "org-2"
		recorder := serveDryRun(t, db, source, &principal, dryRunPath, dryRunBody)
		if recorder.Code != http.StatusNotFound || source.reads != 0 {
			t.Fatalf("got %d, event reads %d", recorder.Code, source.reads)
		}
	})
	for name, connection := range map[string]string{"another connection's event": "conn-2", "an event with no connection": ""} {
		t.Run(name, func(t *testing.T) {
			db, source := dryRunFixture()
			source.event.ConnectionID = connection
			principal := dryRunPrincipal()
			recorder := serveDryRun(t, db, source, &principal, dryRunPath, dryRunBody)
			if recorder.Code != http.StatusNotFound || dryRunCode(t, recorder) != "event_trigger_not_found" {
				t.Fatalf("got %d %q; want the missing-trigger answer", recorder.Code, dryRunCode(t, recorder))
			}
			if strings.Contains(recorder.Body.String(), dryRunSecretValue) {
				t.Fatal("event content leaked")
			}
		})
	}
	t.Run("unreadable event", func(t *testing.T) {
		db, source := dryRunFixture()
		source.err = errors.New("integration-corev2 answered 503")
		principal := dryRunPrincipal()
		recorder := serveDryRun(t, db, source, &principal, dryRunPath, dryRunBody)
		if recorder.Code != http.StatusServiceUnavailable || dryRunCode(t, recorder) != "wake_payload_unavailable" {
			t.Fatalf("got %d %q", recorder.Code, dryRunCode(t, recorder))
		}
	})
}

func TestEventTriggerDryRunRejectsMalformedRequestsAndOtherPaths(t *testing.T) {
	principal := dryRunPrincipal()
	for _, body := range []string{``, `{}`, `{"webhook_event_id":"  "}`, `{"webhook_event_id":"wh-1","extra":1}`,
		`{"webhook_event_id":"wh-1"}{"webhook_event_id":"wh-2"}`, `{"webhook_event_id":"` + strings.Repeat("x", 129) + `"}`} {
		db, source := dryRunFixture()
		recorder := serveDryRun(t, db, source, &principal, dryRunPath, body)
		if recorder.Code != http.StatusBadRequest || db.reads != 0 || source.reads != 0 {
			t.Fatalf("body %q: got %d, trigger reads %d, event reads %d", body, recorder.Code, db.reads, source.reads)
		}
	}
	// ("//dry-run" never reaches the handler: ServeMux cleans the path with a redirect first.)
	for _, path := range []string{"/api/v1/event-triggers/trg_1", "/api/v1/event-triggers/dry-run",
		"/api/v1/event-triggers/a/b/dry-run", "/api/v1/event-triggers/trg_1/dry-run/x"} {
		db, source := dryRunFixture()
		recorder := serveDryRun(t, db, source, &principal, path, dryRunBody)
		if recorder.Code != http.StatusNotFound || db.reads != 0 {
			t.Fatalf("path %q: got %d, trigger reads %d", path, recorder.Code, db.reads)
		}
	}
}

func TestEventTriggerDryRunLimitsEachTriggerPerMinute(t *testing.T) {
	db, source := dryRunFixture()
	principal := dryRunPrincipal()
	mux := http.NewServeMux()
	NewEventTriggerDryRunHandler(db, source).Register(mux)
	var last int
	for i := 0; i < eventTriggerDryRunPerMinute+1; i++ {
		request := httptest.NewRequest(http.MethodPost, dryRunPath, strings.NewReader(dryRunBody))
		request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
		recorder := httptest.NewRecorder()
		mux.ServeHTTP(recorder, request)
		last = recorder.Code
		if i < eventTriggerDryRunPerMinute && last != http.StatusOK {
			t.Fatalf("dry run %d answered %d", i+1, last)
		}
	}
	if last != http.StatusTooManyRequests {
		t.Fatalf("dry run %d answered %d; want 429", eventTriggerDryRunPerMinute+1, last)
	}
	if source.reads != eventTriggerDryRunPerMinute {
		t.Fatalf("event reads %d; the limited dry run must read nothing", source.reads)
	}
}

func TestEventTriggerDryRunIsNotMountedWhileTheSwitchIsOff(t *testing.T) {
	// Off, cmd/main.go never calls Register: the path is the mux's 404.
	mux := http.NewServeMux()
	request := httptest.NewRequest(http.MethodPost, dryRunPath, strings.NewReader(dryRunBody))
	principal := dryRunPrincipal()
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
	recorder := httptest.NewRecorder()
	mux.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusNotFound {
		t.Fatalf("unmounted dry run answered %d", recorder.Code)
	}
}
