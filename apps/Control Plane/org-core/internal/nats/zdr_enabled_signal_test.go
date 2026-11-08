package nats

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	gonats "github.com/nats-io/nats.go"
)

type postureReaderStub struct {
	postures []orgcore.RetentionPosture // returned in order; the last one repeats
	err      error
	calls    int
}

func (r *postureReaderStub) GetRetentionPosture(_ context.Context, _ string) (*orgcore.RetentionPosture, error) {
	r.calls++
	if r.err != nil {
		return nil, r.err
	}
	i := r.calls - 1
	if i >= len(r.postures) {
		i = len(r.postures) - 1
	}
	p := r.postures[i]
	return &p, nil
}

type publishedZDR struct {
	subject string
	msgID   string
	payload map[string]any
}

type zdrPublishStub struct {
	published []publishedZDR
	err       error
}

func (p *zdrPublishStub) publish(_ context.Context, subject, msgID string, payload map[string]any) error {
	if p.err != nil {
		return p.err
	}
	p.published = append(p.published, publishedZDR{subject: subject, msgID: msgID, payload: payload})
	return nil
}

func TestEmitZDREnabledOnPlanChangeOnlyOnEffectiveTransition(t *testing.T) {
	intentOn := orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}
	intentOff := orgcore.RetentionPosture{ZDRIntent: false, Entitled: true}
	tests := []struct {
		name        string
		prev, next  string
		posture     orgcore.RetentionPosture
		wantPublish bool
		wantRead    bool
	}{
		{name: "upgrade with stored intent turns ZDR on", prev: "standard", next: "pro", posture: intentOn, wantPublish: true, wantRead: true},
		{name: "trial to enterprise with stored intent", prev: "trial", next: "enterprise", posture: intentOn, wantPublish: true, wantRead: true},
		{name: "upgrade without intent", prev: "standard", next: "pro", posture: intentOff, wantRead: true},
		{name: "entitled to entitled keeps ZDR where it was", prev: "pro", next: "enterprise", posture: intentOn},
		{name: "downgrade can only turn ZDR off", prev: "pro", next: "standard", posture: intentOn},
		{name: "unentitled to unentitled", prev: "free", next: "hobby", posture: intentOn},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			reader := &postureReaderStub{postures: []orgcore.RetentionPosture{tt.posture}}
			pub := &zdrPublishStub{}
			got, err := emitZDREnabledOnPlanChange(context.Background(), reader, pub.publish, "org-a", tt.prev, tt.next, 7)
			if err != nil {
				t.Fatalf("emit: %v", err)
			}
			if got != tt.wantPublish || (len(pub.published) == 1) != tt.wantPublish {
				t.Fatalf("published=%v (%d messages); want %v", got, len(pub.published), tt.wantPublish)
			}
			if (reader.calls > 0) != tt.wantRead {
				t.Fatalf("posture reads=%d; want read=%v", reader.calls, tt.wantRead)
			}
			if !tt.wantPublish {
				return
			}
			msg := pub.published[0]
			if msg.subject != "controlplane.org.interactive_retention.enabled" {
				t.Fatalf("subject=%q", msg.subject)
			}
			if msg.msgID != "interactive-retention:plan:org-a:7" {
				t.Fatalf("msgID=%q; want stable per org and revision", msg.msgID)
			}
			if msg.payload["org_id"] != "org-a" || msg.payload["zdr"] != true || msg.payload["cause"] != ZDRCausePlanChange {
				t.Fatalf("payload=%v", msg.payload)
			}
			if len(msg.payload) != 3 {
				t.Fatalf("payload carries more than org_id, zdr and cause: %v", msg.payload)
			}
		})
	}
}

func TestEmitZDREnabledOnPlanChangeIsInertWithoutReader(t *testing.T) {
	pub := &zdrPublishStub{}
	got, err := emitZDREnabledOnPlanChange(context.Background(), nil, pub.publish, "org-a", "standard", "pro", 1)
	if err != nil || got || len(pub.published) != 0 {
		t.Fatalf("nil reader: published=%v err=%v messages=%d", got, err, len(pub.published))
	}
	reader := &postureReaderStub{postures: []orgcore.RetentionPosture{{ZDRIntent: true, Entitled: true}}}
	if got, err := emitZDREnabledOnPlanChange(context.Background(), reader, pub.publish, "", "standard", "pro", 1); err != nil || got {
		t.Fatalf("empty org id: published=%v err=%v", got, err)
	}
	if reader.calls != 0 {
		t.Fatalf("empty org id read the posture %d times", reader.calls)
	}
}

func TestEmitZDREnabledOnPlanChangeErrors(t *testing.T) {
	pub := &zdrPublishStub{}
	gone := &postureReaderStub{err: orgcore.ErrNotFound}
	if got, err := emitZDREnabledOnPlanChange(context.Background(), gone, pub.publish, "org-a", "standard", "pro", 2); err != nil || got {
		t.Fatalf("deleted org: published=%v err=%v; want a skip", got, err)
	}

	broken := &postureReaderStub{err: errors.New("db down")}
	if _, err := emitZDREnabledOnPlanChange(context.Background(), broken, pub.publish, "org-a", "standard", "pro", 2); err == nil || !strings.Contains(err.Error(), "db down") {
		t.Fatalf("posture read error=%v; want it returned so the outbox retries", err)
	}

	reader := &postureReaderStub{postures: []orgcore.RetentionPosture{{ZDRIntent: true, Entitled: true}}}
	failing := &zdrPublishStub{err: errors.New("broker unavailable")}
	if _, err := emitZDREnabledOnPlanChange(context.Background(), reader, failing.publish, "org-a", "standard", "pro", 2); err == nil || !strings.Contains(err.Error(), "broker unavailable") {
		t.Fatalf("publish error=%v; want it returned so the outbox retries", err)
	}
	if len(pub.published) != 0 {
		t.Fatalf("error paths published %d messages", len(pub.published))
	}
}

func TestZDRMessageIDs(t *testing.T) {
	if got, want := planChangeZDRMessageID("org-1", 42), "interactive-retention:plan:org-1:42"; got != want {
		t.Fatalf("planChangeZDRMessageID()=%q; want %q", got, want)
	}
	if got := planChangeZDRMessageID("org-1", 42); got == interactiveRetentionMessageID(42) {
		t.Fatalf("plan-change id %q collides with the toggle's outbox id", got)
	}
	at := time.Unix(1700000000, 5)
	if got, want := billingSyncZDRMessageID("org-1", "pro", at), "interactive-retention:billing-sync:org-1:pro:1700000000000000005"; got != want {
		t.Fatalf("billingSyncZDRMessageID()=%q; want %q", got, want)
	}
}

func TestZDRTurnedOn(t *testing.T) {
	on := orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}
	lapsed := orgcore.RetentionPosture{ZDRIntent: true, Entitled: false}
	none := orgcore.RetentionPosture{}
	if !zdrTurnedOn(lapsed, on) || !zdrTurnedOn(none, on) {
		t.Fatal("false-to-true transition not detected")
	}
	if zdrTurnedOn(on, on) || zdrTurnedOn(on, lapsed) || zdrTurnedOn(lapsed, lapsed) {
		t.Fatal("non-transition reported as ZDR turned on")
	}
}

type billingOrgServiceStub struct {
	postureReaderStub
	skippedOverride bool
	applyErr        error
	applied         []string
}

func (s *billingOrgServiceStub) ApplyBillingPlanSync(_ context.Context, orgID, plan string) (bool, error) {
	s.applied = append(s.applied, orgID+":"+plan)
	return s.skippedOverride, s.applyErr
}

type billingSignalStub struct {
	calls []string
}

func (s *billingSignalStub) PublishZDREnabledForBillingSync(_ context.Context, orgID, plan string) error {
	s.calls = append(s.calls, orgID+":"+plan)
	return nil
}

func TestBillingSyncEmitsZDREnabledOnlyOnTransition(t *testing.T) {
	lapsed := orgcore.RetentionPosture{ZDRIntent: true, Entitled: false}
	on := orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}
	off := orgcore.RetentionPosture{ZDRIntent: false, Entitled: true}
	msg := &gonats.Msg{Data: []byte(`{"data":{"org_id":"org-a","plan":"pro"}}`)}

	tests := []struct {
		name     string
		postures []orgcore.RetentionPosture
		override bool
		applyErr error
		want     int
	}{
		{name: "lapsed intent becomes effective", postures: []orgcore.RetentionPosture{lapsed, on}, want: 1},
		{name: "already effective", postures: []orgcore.RetentionPosture{on, on}},
		{name: "no intent", postures: []orgcore.RetentionPosture{off, off}},
		{name: "override pin skips the sync", postures: []orgcore.RetentionPosture{lapsed, on}, override: true},
		{name: "failed sync", postures: []orgcore.RetentionPosture{lapsed, on}, applyErr: errors.New("db down")},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			svc := &billingOrgServiceStub{
				postureReaderStub: postureReaderStub{postures: tt.postures},
				skippedOverride:   tt.override,
				applyErr:          tt.applyErr,
			}
			signal := &billingSignalStub{}
			sub := &BillingSyncSubscriber{orgService: svc}
			sub.SetZDRSignal(signal)
			sub.handle(context.Background())(msg)
			if len(svc.applied) != 1 || svc.applied[0] != "org-a:pro" {
				t.Fatalf("applied=%v", svc.applied)
			}
			if len(signal.calls) != tt.want {
				t.Fatalf("ZDR signal calls=%v; want %d", signal.calls, tt.want)
			}
			if tt.want == 1 && signal.calls[0] != "org-a:pro" {
				t.Fatalf("ZDR signal call=%q", signal.calls[0])
			}
		})
	}
}

func TestBillingSyncWithoutZDRSignalReadsNoPosture(t *testing.T) {
	svc := &billingOrgServiceStub{postureReaderStub: postureReaderStub{postures: []orgcore.RetentionPosture{{}}}}
	sub := &BillingSyncSubscriber{orgService: svc}
	var nilPublisher *SharedPublisher
	sub.SetZDRSignal(nilPublisher) // shared NATS not configured: stays off
	if sub.zdrSignal != nil {
		t.Fatal("a typed-nil shared publisher switched the ZDR signal on")
	}
	sub.handle(context.Background())(&gonats.Msg{Data: []byte(`{"org_id":"org-a","plan":"pro"}`)})
	if svc.calls != 0 {
		t.Fatalf("posture reads=%d with the signal off; want 0", svc.calls)
	}
	if len(svc.applied) != 1 {
		t.Fatalf("applied=%v", svc.applied)
	}
}

func TestSetRetentionPostureReaderOnNilPublisherIsSafe(t *testing.T) {
	var sp *SharedPublisher
	sp.SetRetentionPostureReader(&postureReaderStub{}) // must not panic
}
