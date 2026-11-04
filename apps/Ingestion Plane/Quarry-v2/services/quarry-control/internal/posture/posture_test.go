package posture

import (
	"errors"
	"testing"

	"github.com/nats-io/nats.go"
	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

const (
	orgA = "org-a"
	orgB = "org-b"
)

func seedSchedule(t *testing.T, db store.DB, orgID, kind string, enabled bool) quarrycontracts.ID {
	t.Helper()
	id := quarrycontracts.NewID(quarrycontracts.KindSchedule)
	if err := db.Schedules().Create(store.Schedule{
		ID:         id,
		OrgID:      orgID,
		Cron:       "0 * * * *",
		TargetKind: kind,
		TargetRef:  "https://example.test/" + string(id),
		Enabled:    enabled,
	}); err != nil {
		t.Fatalf("seed schedule: %v", err)
	}
	return id
}

func enabledOf(t *testing.T, db store.DB, id quarrycontracts.ID, orgID string) bool {
	t.Helper()
	s, ok := db.Schedules().GetByOrg(id, orgID)
	if !ok {
		t.Fatalf("schedule %s of %s not found", id, orgID)
	}
	return s.Enabled
}

// recordingEvents wraps the store's event log and keeps every event the
// consumer appended successfully.
type recordingEvents struct {
	store.EventLog
	appended []quarrycontracts.Event
}

func (r *recordingEvents) AppendNext(evt quarrycontracts.Event) (uint64, error) {
	seq, err := r.EventLog.AppendNext(evt)
	if err == nil {
		r.appended = append(r.appended, evt)
	}
	return seq, err
}

// reasons counts the skipped_zdr reason events appended for scheduleID.
func (r *recordingEvents) reasons(scheduleID quarrycontracts.ID) int {
	n := 0
	for _, e := range r.appended {
		if e.Type == quarrycontracts.EvtScheduleSkipped &&
			e.Payload["schedule_id"] == string(scheduleID) &&
			e.Payload["reason"] == ReasonSkippedZDR {
			n++
		}
	}
	return n
}

func newTestConsumer(db store.DB, mode Mode) (*Consumer, *recordingEvents) {
	c := NewConsumer(db, mode, zerolog.Nop())
	rec := &recordingEvents{EventLog: db.Events()}
	c.events = rec
	return c, rec
}

func TestModeFromEnvDefaultsOffAndNeverEnforcesATypo(t *testing.T) {
	cases := []struct {
		value   string
		want    Mode
		invalid bool
	}{
		{"", ModeOff, false},
		{"off", ModeOff, false},
		{"observe", ModeObserve, false},
		{" Observe ", ModeObserve, false},
		{"shadow", ModeObserve, false},
		{"enforce", ModeEnforce, false},
		{"enforced", ModeOff, true},
		{"on", ModeOff, true},
		{"1", ModeOff, true},
	}
	for _, c := range cases {
		got, invalid := ModeFromEnv(func(k string) string {
			if k != FlagEnv {
				t.Fatalf("read %q; want %q", k, FlagEnv)
			}
			return c.value
		})
		if got != c.want || invalid != c.invalid {
			t.Errorf("ModeFromEnv(%q) = %q, %v; want %q, %v", c.value, got, invalid, c.want, c.invalid)
		}
	}
}

func TestEnforcePausesOnlyTheEventOrgAndIsIdempotent(t *testing.T) {
	db := store.NewMemory()
	monitorA := seedSchedule(t, db, orgA, store.TargetKindChangeMonitor, true)
	scrapeA := seedSchedule(t, db, orgA, "scrape", true)
	pausedA := seedSchedule(t, db, orgA, "crawl", false)
	monitorB := seedSchedule(t, db, orgB, store.TargetKindChangeMonitor, true)

	c, rec := newTestConsumer(db, ModeEnforce)
	const key = "interactive-retention:plan:org-a:7"
	res, err := c.Handle([]byte(`{"org_id":"org-a","zdr":true,"cause":"plan_change"}`), key)
	if err != nil {
		t.Fatalf("handle: %v", err)
	}
	if res.Enabled != 2 || res.Monitors != 1 || res.Paused != 2 || res.AlreadyPaused != 1 || res.WouldPause != 0 {
		t.Fatalf("first delivery result=%+v", res)
	}
	if enabledOf(t, db, monitorA, orgA) || enabledOf(t, db, scrapeA, orgA) {
		t.Fatal("org-a schedules still enabled after enforce")
	}
	if enabledOf(t, db, pausedA, orgA) {
		t.Fatal("an already paused org-a schedule was re-enabled")
	}
	if !enabledOf(t, db, monitorB, orgB) {
		t.Fatal("org-b's monitor was paused by org-a's event")
	}
	if rec.reasons(monitorA) != 1 || rec.reasons(scrapeA) != 1 {
		t.Fatal("skipped_zdr reason events missing for the paused schedules")
	}
	if rec.reasons(pausedA) != 0 {
		t.Fatal("a reason event was written for a schedule that was already paused")
	}
	if rec.reasons(monitorB) != 0 {
		t.Fatal("a reason event was written for another org's schedule")
	}
	for _, e := range rec.appended {
		if e.RunID != nil || e.JobID != nil || e.Payload["url"] != nil || e.Payload["org_id"] != nil {
			t.Fatalf("reason event carries more than the schedule: %+v", e)
		}
	}
	written := len(rec.appended)

	// Redelivery of the same event: nothing left to pause, nothing written.
	res, err = c.Handle([]byte(`{"org_id":"org-a","zdr":true,"cause":"plan_change"}`), key)
	if err != nil {
		t.Fatalf("redelivery: %v", err)
	}
	if res.Enabled != 0 || res.Paused != 0 || res.AlreadyPaused != 3 {
		t.Fatalf("redelivery result=%+v; want a no-op", res)
	}
	if len(rec.appended) != written {
		t.Fatalf("redelivery wrote %d more reason events", len(rec.appended)-written)
	}
	if !enabledOf(t, db, monitorB, orgB) {
		t.Fatal("org-b's monitor was paused on redelivery")
	}
}

func TestEnforceRedeliveryAfterACrashBetweenReasonAndPause(t *testing.T) {
	db := store.NewMemory()
	monitor := seedSchedule(t, db, orgA, store.TargetKindChangeMonitor, true)
	c, rec := newTestConsumer(db, ModeEnforce)
	const key = "interactive-retention:41"

	// The first delivery wrote the reason event and died before the pause.
	s, _ := db.Schedules().GetByOrg(monitor, orgA)
	if err := c.writeReason(s, "", key); err != nil {
		t.Fatalf("seed reason: %v", err)
	}
	res, err := c.Handle([]byte(`{"org_id":"org-a","zdr":true}`), key)
	if err != nil {
		t.Fatalf("redelivery: %v (a duplicate reason event must not fail the pause)", err)
	}
	if res.Paused != 1 || enabledOf(t, db, monitor, orgA) {
		t.Fatalf("redelivery result=%+v; want the monitor paused", res)
	}
	if rec.reasons(monitor) != 1 {
		t.Fatalf("reason events=%d; want the first one only", rec.reasons(monitor))
	}
}

func TestObserveLogsAndChangesNothing(t *testing.T) {
	db := store.NewMemory()
	monitor := seedSchedule(t, db, orgA, store.TargetKindChangeMonitor, true)
	scrape := seedSchedule(t, db, orgA, "scrape", true)
	c, rec := newTestConsumer(db, ModeObserve)
	const key = "interactive-retention:9"

	res, err := c.Handle([]byte(`{"org_id":"org-a","zdr":true}`), key)
	if err != nil {
		t.Fatalf("handle: %v", err)
	}
	if res.WouldPause != 2 || res.Paused != 0 || res.Monitors != 1 {
		t.Fatalf("observe result=%+v", res)
	}
	if !enabledOf(t, db, monitor, orgA) || !enabledOf(t, db, scrape, orgA) {
		t.Fatal("observe paused a schedule")
	}
	if len(rec.appended) != 0 {
		t.Fatalf("observe wrote %d events", len(rec.appended))
	}
}

func TestOffAndNoOpEvents(t *testing.T) {
	db := store.NewMemory()
	monitor := seedSchedule(t, db, orgA, store.TargetKindChangeMonitor, true)

	off := NewConsumer(db, ModeOff, zerolog.Nop())
	if res, err := off.Handle([]byte(`{"org_id":"org-a","zdr":true}`), "k"); err != nil || res.Enabled != 0 {
		t.Fatalf("off: result=%+v err=%v", res, err)
	}

	enforce := NewConsumer(db, ModeEnforce, zerolog.Nop())
	for _, payload := range []string{
		`{"org_id":"org-a","zdr":false}`,
		`{"org_id":"org-a"}`,
		`{"zdr":true}`,
		`{"org_id":"  ","zdr":true}`,
	} {
		if res, err := enforce.Handle([]byte(payload), "k"); err != nil || res.Enabled != 0 || res.Paused != 0 {
			t.Fatalf("payload %s: result=%+v err=%v; want a no-op", payload, res, err)
		}
	}
	if !enabledOf(t, db, monitor, orgA) {
		t.Fatal("a no-op event paused a schedule")
	}

	_, err := enforce.Handle([]byte(`{not json`), "k")
	var poison *poisonEventError
	if !errors.As(err, &poison) {
		t.Fatalf("malformed payload error=%v; want a poison error (ack, no retry)", err)
	}
}

func TestStartSubscriberRefusesOffAndMissingInputs(t *testing.T) {
	db := store.NewMemory()
	if _, err := StartSubscriber(nil, NewConsumer(db, ModeEnforce, zerolog.Nop()), zerolog.Nop()); err == nil {
		t.Fatal("nil connection accepted")
	}
	if _, err := StartSubscriber(&nats.Conn{}, nil, zerolog.Nop()); err == nil {
		t.Fatal("nil consumer accepted")
	}
	if _, err := StartSubscriber(&nats.Conn{}, NewConsumer(db, ModeOff, zerolog.Nop()), zerolog.Nop()); err == nil {
		t.Fatal("an off consumer subscribed")
	}
}

func TestDeliveryKeyPrefersTheMessageID(t *testing.T) {
	msg := &nats.Msg{Header: nats.Header{}}
	msg.Header.Set(nats.MsgIdHdr, "interactive-retention:plan:org-a:7")
	if got := deliveryKey(msg); got != "interactive-retention:plan:org-a:7" {
		t.Fatalf("deliveryKey=%q", got)
	}
	if got := deliveryKey(&nats.Msg{}); got != "" {
		t.Fatalf("deliveryKey without id or metadata=%q; want empty", got)
	}
	if got := deliveryKey(nil); got != "" {
		t.Fatalf("deliveryKey(nil)=%q", got)
	}
}

func TestReasonEventIDIsDeterministicAndScoped(t *testing.T) {
	a := reasonEventID(reasonDigest(orgA, "sch_1", "k"))
	if a != reasonEventID(reasonDigest(orgA, "sch_1", "k")) {
		t.Fatal("reason event id is not deterministic")
	}
	if a == reasonEventID(reasonDigest(orgB, "sch_1", "k")) || a == reasonEventID(reasonDigest(orgA, "sch_1", "k2")) {
		t.Fatal("reason event id does not depend on the org and the transition")
	}
	if kind, ok := a.Kind(); !ok || kind != quarrycontracts.KindEvent {
		t.Fatalf("reason event id %q is not an evt_ id", a)
	}
}
