package browsersites

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"
)

// auditFakeStore is a Store with no database. It records the writer each write carries, and
// it holds an in-memory outbox, so the service and the publish loop run without Postgres.
type auditFakeStore struct {
	writers   []writer
	pending   []AuditOutboxRow
	published []string
	failed    []string
}

func (f *auditFakeStore) Get(context.Context, string) (*PolicySet, error) {
	return &PolicySet{Mode: ModeOpen, Rules: []Rule{}}, nil
}

func (f *auditFakeStore) Put(ctx context.Context, orgID, _ string, expected int64, req PutRequest) (*PolicySet, error) {
	w, _ := writerFrom(ctx)
	f.writers = append(f.writers, w)
	return &PolicySet{OrganizationID: orgID, Mode: req.Mode, Revision: expected + 1, Rules: []Rule{}}, nil
}

func (f *auditFakeStore) Snapshot(context.Context, string, string, []string) (*PolicySet, map[string]Pref, error) {
	return &PolicySet{Mode: ModeOpen, Rules: []Rule{}}, map[string]Pref{}, nil
}

func (f *auditFakeStore) ListPrefs(context.Context, string, string) (PrefSet, error) {
	return PrefSet{Prefs: []Pref{}}, nil
}

func (f *auditFakeStore) PutPref(ctx context.Context, _, _ string, _ int64, pref Pref) (Pref, error) {
	w, _ := writerFrom(ctx)
	f.writers = append(f.writers, w)
	return pref, nil
}

func (f *auditFakeStore) DeletePref(ctx context.Context, _, _, _ string) error {
	w, _ := writerFrom(ctx)
	f.writers = append(f.writers, w)
	return nil
}

func (f *auditFakeStore) DeleteUserData(context.Context, string, string) (int64, int64, error) {
	return 0, 0, nil
}

func (f *auditFakeStore) DeleteRemovedMemberData(context.Context, string, string) (int64, int64, error) {
	return 0, 0, nil
}

func (f *auditFakeStore) claimAuditOutbox(context.Context, int) ([]AuditOutboxRow, error) {
	rows := f.pending
	f.pending = nil
	return rows, nil
}

func (f *auditFakeStore) markAuditPublished(_ context.Context, eventID string) error {
	f.published = append(f.published, eventID)
	return nil
}

func (f *auditFakeStore) markAuditPublishFailed(_ context.Context, eventID string, _ error) (bool, error) {
	f.failed = append(f.failed, eventID)
	return false, nil
}

// auditRecordingPublisher records every publish and fails the one event id it is told to.
type auditRecordingPublisher struct {
	failOn   string
	subjects []string
	eventIDs []string
}

func (p *auditRecordingPublisher) PublishAudit(_ context.Context, subject, eventID string, _ map[string]any) error {
	p.subjects = append(p.subjects, subject)
	p.eventIDs = append(p.eventIDs, eventID)
	if eventID == p.failOn {
		return errors.New("jetstream unavailable")
	}
	return nil
}

// auditAssertKeys fails unless m holds exactly the keys in want.
func auditAssertKeys(t *testing.T, m map[string]any, want ...string) {
	t.Helper()
	got := make([]string, 0, len(m))
	for key := range m {
		got = append(got, key)
	}
	sort.Strings(got)
	sort.Strings(want)
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("payload keys = %v, want exactly %v", got, want)
	}
}

// F-f06-11: the switch is read strictly. Only the literal "true" turns the outbox on.
func TestAuditEnabledIsTheStrictLiteralTrue(t *testing.T) {
	if EnvAuditEnabled != "ORG_BROWSER_SITE_AUDIT_ENABLED" {
		t.Fatalf("switch name = %q", EnvAuditEnabled)
	}
	for raw, want := range map[string]bool{
		"true": true, "false": false, "1": false, "TRUE": false, " true": false, "yes": false, "": false,
	} {
		got := AuditEnabled(func(key string) string {
			if key != EnvAuditEnabled {
				return ""
			}
			return raw
		})
		if got != want {
			t.Fatalf("AuditEnabled(%q) = %v, want %v", raw, got, want)
		}
	}
}

// F-f06-11: the payload carries the listed keys and the listed details, and nothing else. A
// choice carries its host and level, a policy its mode and rule count.
func TestAuditPayloadCarriesOnlyTheListedKeys(t *testing.T) {
	occurred := time.Date(2026, 11, 20, 9, 0, 0, 0, time.UTC)
	admin := writer{id: "u-admin", role: "owner", audit: true}

	_, choice := auditPayload(auditChange{
		orgID: "org-1", event: BrowserSitePrefChanged, revision: 4, host: "news.example.com", historyID: 7,
		details: map[string]any{"host": "news.example.com", "level": LevelAct, "revision": int64(4)},
	}, writer{id: "u-member", role: "member", audit: true}, occurred)
	auditAssertKeys(t, choice, "event_id", "occurred_at", "org_id", "user_id", "actor_role", "plane",
		"producer", "event", "subject", "resource_id", "outcome", "details")
	auditAssertKeys(t, choice["details"].(map[string]any), "host", "level", "revision")
	if choice["event"] != BrowserSitePrefChanged || choice["plane"] != "control" || choice["producer"] != "org-core" ||
		choice["outcome"] != "ok" || choice["org_id"] != "org-1" || choice["resource_id"] != "org-1" ||
		choice["user_id"] != "u-member" || choice["actor_role"] != "member" {
		t.Fatalf("choice payload identity is wrong: %v", choice)
	}

	_, policy := auditPayload(auditChange{
		orgID: "org-1", event: BrowserSitePolicyChanged, revision: 5, historyID: 8,
		details: map[string]any{"mode": ModeAllowlist, "rule_count": 2, "revision": int64(5)},
	}, admin, occurred)
	auditAssertKeys(t, policy, "event_id", "occurred_at", "org_id", "user_id", "actor_role", "plane",
		"producer", "event", "subject", "resource_id", "outcome", "details")
	auditAssertKeys(t, policy["details"].(map[string]any), "mode", "rule_count", "revision")
}

// F-f06-11: the intent's event id is stable for one write and differs between two writes that
// share a host and a revision. Two members, or one member twice, are two events.
func TestAuditEventIDIsStableAndDistinctPerWrite(t *testing.T) {
	change := auditChange{orgID: "org-1", event: BrowserSitePrefChanged, revision: 3, host: "news.example.com", historyID: 10}
	member := writer{id: "u-1", role: "member", audit: true}
	first, _ := auditPayload(change, member, time.Now())
	again, _ := auditPayload(change, member, time.Now())
	if first != again {
		t.Fatalf("the same write has two event ids: %q and %q", first, again)
	}
	if !strings.HasPrefix(first, "browsersite:org-core:") || len(first) > 128 {
		t.Fatalf("event id %q is not a bounded org-core id", first)
	}
	otherMember, _ := auditPayload(change, writer{id: "u-2", role: "member", audit: true}, time.Now())
	change.historyID = 11
	nextWrite, _ := auditPayload(change, member, time.Now())
	if first == otherMember || first == nextWrite {
		t.Fatal("two writes at one policy revision share an event id")
	}
}

// F-f06-11: an intent is queued only for a writer the Service put on the context with the
// switch on. Without one, or with the switch off, the transaction is not touched.
func TestEnqueueAuditQueuesNothingUnlessTheSwitchIsOn(t *testing.T) {
	change := auditChange{orgID: "org-1", event: BrowserSitePolicyChanged, revision: 1, historyID: 1,
		details: map[string]any{"mode": ModeOpen, "rule_count": 0, "revision": int64(1)}}
	if err := enqueueAudit(context.Background(), nil, change); err != nil {
		t.Fatalf("a write without a writer must queue nothing: %v", err)
	}
	off := withWriter(context.Background(), writer{id: "u-owner", role: "owner", audit: false})
	if err := enqueueAudit(off, nil, change); err != nil {
		t.Fatalf("a write with the switch off must queue nothing: %v", err)
	}
	blank := withWriter(context.Background(), writer{id: "u-owner", role: " ", audit: true})
	if err := enqueueAudit(blank, nil, change); !errors.Is(err, ErrAuditActorRequired) {
		t.Fatalf("an audited write with no role must be refused, got %v", err)
	}
}

// F-f06-11: with the switch on, a write without a verified actor and role is refused before
// it reaches the store, so no change is stored without its intent.
func TestServiceRefusesAnAuditedWriteWithoutItsActorAndRole(t *testing.T) {
	store := &auditFakeStore{}
	service := NewService(store, time.Now)
	service.SetAuditEnabled(true)
	ctx := context.Background()
	expected := int64(0)

	if _, err := service.Put(ctx, "org-1", "u-owner", "", PutRequest{ExpectedRevision: &expected, Mode: ModeOpen}); !errors.Is(err, ErrAuditActorRequired) {
		t.Fatalf("policy write without a role: got %v", err)
	}
	if _, err := service.SetPref(ctx, "org-1", "u-member", " ", "news.example.com", PrefRequest{ExpectedRevision: &expected, Level: LevelRead}); !errors.Is(err, ErrAuditActorRequired) {
		t.Fatalf("choice write without a role: got %v", err)
	}
	if err := service.DeletePref(ctx, "org-1", "u-member", "", "news.example.com"); !errors.Is(err, ErrAuditActorRequired) {
		t.Fatalf("choice delete without a role: got %v", err)
	}
	if len(store.writers) != 0 {
		t.Fatalf("%d refused writes reached the store", len(store.writers))
	}
}

// F-f06-11: with the switch off a write is unchanged. It needs no role, carries the switch as
// off, and never touches the publisher.
func TestServiceWithTheSwitchOffWritesUnchangedAndQueuesNothing(t *testing.T) {
	store := &auditFakeStore{}
	service := NewService(store, time.Now)
	publisher := &auditRecordingPublisher{}
	service.SetAuditPublisher(publisher)
	ctx := context.Background()
	expected := int64(0)

	if _, err := service.Put(ctx, "org-1", "u-owner", "", PutRequest{ExpectedRevision: &expected, Mode: ModeOpen}); err != nil {
		t.Fatalf("policy write with the switch off: %v", err)
	}
	if _, err := service.SetPref(ctx, "org-1", "u-member", "member", "news.example.com", PrefRequest{ExpectedRevision: &expected, Level: LevelRead}); err != nil {
		t.Fatalf("choice write with the switch off: %v", err)
	}
	if err := service.DeletePref(ctx, "org-1", "u-member", "member", "news.example.com"); err != nil {
		t.Fatalf("choice delete with the switch off: %v", err)
	}
	if len(store.writers) != 3 {
		t.Fatalf("store saw %d writes, want 3", len(store.writers))
	}
	for _, w := range store.writers {
		if w.audit {
			t.Fatal("a write with the switch off carries the audit switch on")
		}
	}
	if len(publisher.eventIDs) != 0 {
		t.Fatalf("a write with the switch off published %d intents", len(publisher.eventIDs))
	}
}

// F-f06-11: an audited write hands the Repository the verified writer, with the switch on.
func TestServiceCarriesTheVerifiedWriterWhenTheSwitchIsOn(t *testing.T) {
	store := &auditFakeStore{}
	service := NewService(store, time.Now)
	service.SetAuditEnabled(true)
	expected := int64(2)

	if _, err := service.SetPref(context.Background(), "org-1", "u-member", "member", "news.example.com", PrefRequest{ExpectedRevision: &expected, Level: LevelAct}); err != nil {
		t.Fatalf("choice write: %v", err)
	}
	if len(store.writers) != 1 {
		t.Fatalf("store saw %d writes, want 1", len(store.writers))
	}
	got := store.writers[0]
	if got.id != "u-member" || got.role != "member" || !got.audit {
		t.Fatalf("writer on the write = %+v, want the verified member with the switch on", got)
	}
}

// F-f06-11: the flush publishes each claimed intent and marks it by its outcome. A failed
// publish stays pending and is reported, and it never stops the others.
func TestFlushPublishesEachIntentAndMarksItsOutcome(t *testing.T) {
	store := &auditFakeStore{pending: []AuditOutboxRow{
		{EventID: "browsersite:org-core:a", OrgID: "org-1", Subject: browserSiteAuditSubjectPrefix + BrowserSitePrefChanged, Payload: map[string]any{}},
		{EventID: "browsersite:org-core:b", OrgID: "org-1", Subject: browserSiteAuditSubjectPrefix + BrowserSitePolicyChanged, Payload: map[string]any{}},
	}}
	publisher := &auditRecordingPublisher{failOn: "browsersite:org-core:b"}
	service := NewService(store, time.Now)
	service.SetAuditPublisher(publisher)

	result, err := service.FlushBrowserSiteAuditOutbox(context.Background(), 100)
	if err == nil {
		t.Fatal("a failed publish must be reported to the caller")
	}
	if result.Published != 1 || result.DeadLettered != 0 {
		t.Fatalf("flush result = %+v, want one published and none dead-lettered", result)
	}
	if !reflect.DeepEqual(store.published, []string{"browsersite:org-core:a"}) {
		t.Fatalf("marked published = %v", store.published)
	}
	if !reflect.DeepEqual(store.failed, []string{"browsersite:org-core:b"}) {
		t.Fatalf("marked failed = %v", store.failed)
	}
	wantSubjects := []string{
		"verevon.audit.v2.control.org-core.browser_site_pref_changed",
		"verevon.audit.v2.control.org-core.browser_site_policy_changed",
	}
	if !reflect.DeepEqual(publisher.subjects, wantSubjects) {
		t.Fatalf("published subjects = %v, want %v", publisher.subjects, wantSubjects)
	}
}

// F-f06-11: a flush that cannot publish says so instead of claiming rows it cannot send.
func TestFlushRefusesWhenItCannotPublish(t *testing.T) {
	ctx := context.Background()
	store := &auditFakeStore{}
	service := NewService(store, time.Now)
	if _, err := service.FlushBrowserSiteAuditOutbox(ctx, 100); err == nil {
		t.Fatal("a flush without a publisher must refuse")
	}
	service.SetAuditPublisher(&auditRecordingPublisher{})
	for _, limit := range []int{0, 1001} {
		if _, err := service.FlushBrowserSiteAuditOutbox(ctx, limit); err == nil {
			t.Fatalf("a flush with limit %d must refuse", limit)
		}
	}
	noOutbox := NewService(nil, time.Now)
	noOutbox.SetAuditPublisher(&auditRecordingPublisher{})
	if _, err := noOutbox.FlushBrowserSiteAuditOutbox(ctx, 100); err == nil {
		t.Fatal("a store without an outbox must refuse the flush")
	}
}

// F-f06-11: the writer is carried only when the Service sets it. A bare context has none.
func TestWriterIsCarriedOnlyWhenTheServiceSetsIt(t *testing.T) {
	if _, ok := writerFrom(context.Background()); ok {
		t.Fatal("a bare context carries a writer")
	}
	carried := withWriter(context.Background(), writer{id: "u-1", role: "owner", audit: true})
	if w, ok := writerFrom(carried); !ok || w.id != "u-1" || !w.audit {
		t.Fatalf("writerFrom = %+v, %v", w, ok)
	}
}

// F-f06-11: the payload names no rule host and no note. The policy details are the mode and the
// rule count, which is what the repository builds from the request.
func TestAuditPolicyPayloadNamesNoRuleHostOrNote(t *testing.T) {
	change := auditChange{orgID: "org-1", event: BrowserSitePolicyChanged, revision: 2, historyID: 3,
		details: map[string]any{"mode": ModeAllowlist, "rule_count": 1, "revision": int64(2)}}
	_, payload := auditPayload(change, writer{id: "u-owner", role: "owner", audit: true}, time.Now())
	body, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	for _, banned := range []string{"private.example.org", "checkout note"} {
		if strings.Contains(string(body), banned) {
			t.Fatalf("payload carries %q", banned)
		}
	}
}
