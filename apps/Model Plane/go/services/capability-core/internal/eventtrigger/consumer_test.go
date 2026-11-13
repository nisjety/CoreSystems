package eventtrigger

import (
	"context"
	"errors"
	"testing"
	"time"
)

// consumedAt is the consumer's clock in these tests.
var consumedAt = time.Date(2026, 11, 8, 12, 0, 0, 0, time.UTC)

// fakeRecorder stands in for the fire table's record step. It keeps each event and the clock it was recorded at.
type fakeRecorder struct {
	events   []WebhookReceived
	nows     []time.Time
	recorded int
	err      error
}

func (r *fakeRecorder) record(_ context.Context, event WebhookReceived, now time.Time) (int, error) {
	r.events = append(r.events, event)
	r.nows = append(r.nows, now)
	if r.err != nil {
		return 0, r.err
	}
	return r.recorded, nil
}

// consumerAt is a consumer over rec whose clock reads now.
func consumerAt(rec recorder, now time.Time) *Consumer {
	c := newConsumer(rec)
	c.now = func() time.Time { return now }
	return c
}

func TestProcessRecordsAWellFormedEventAndAcknowledgesIt(t *testing.T) {
	rec := &fakeRecorder{recorded: 2}
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(validEnvelope), consumedAt); got != deliveryAck {
		t.Fatalf("delivery = %v; want ack", got)
	}
	if len(rec.events) != 1 || rec.events[0].WebhookEventID != "wh-1" || rec.events[0].OrgID != "org-1" {
		t.Fatalf("recorded events = %+v", rec.events)
	}
}

func TestProcessDropsAMalformedEventWithoutRecordingIt(t *testing.T) {
	rec := &fakeRecorder{}
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(`{"organizationId":"org-1"}`), consumedAt); got != deliveryAck {
		t.Fatalf("delivery = %v; a malformed event is acknowledged and dropped", got)
	}
	if len(rec.events) != 0 {
		t.Fatal("a malformed event reached the fire table")
	}
}

func TestProcessRetriesOnlyAFailedWrite(t *testing.T) {
	rec := &fakeRecorder{err: errors.New("database unavailable")}
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(validEnvelope), consumedAt); got != deliveryRetry {
		t.Fatalf("delivery = %v; a failed write is redelivered", got)
	}
}

// The fire carries the event's own creation time, not the broker's: the envelope's createdAt is what the age checks read.
func TestProcessStampsTheFireWithTheEventsOwnCreationTime(t *testing.T) {
	rec := &fakeRecorder{recorded: 1}
	brokerAt := consumedAt.Add(-time.Hour)
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(timedEnvelope), brokerAt); got != deliveryAck {
		t.Fatalf("delivery = %v; want ack", got)
	}
	if len(rec.events) != 1 || !rec.events[0].EventAt.Equal(eventCreatedAt) {
		t.Fatalf("recorded events = %+v; want the envelope's createdAt %v", rec.events, eventCreatedAt)
	}
}

// An envelope with no createdAt takes the broker's time for the message, the time the event was published.
func TestProcessFallsBackToTheBrokerTimeWhenTheEventHasNoCreationTime(t *testing.T) {
	rec := &fakeRecorder{recorded: 1}
	brokerAt := consumedAt.Add(-5 * time.Minute)
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(validEnvelope), brokerAt); got != deliveryAck {
		t.Fatalf("delivery = %v; want ack", got)
	}
	if len(rec.events) != 1 || !rec.events[0].EventAt.Equal(brokerAt) {
		t.Fatalf("recorded events = %+v; want the broker time %v", rec.events, brokerAt)
	}
}

// An event whose time is unknown in both the envelope and the broker has no age that can be checked, so no fire is
// recorded for it and it is acknowledged, never redelivered.
func TestProcessDropsAnEventWithNoTimeAtAll(t *testing.T) {
	rec := &fakeRecorder{recorded: 1}
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(validEnvelope), time.Time{}); got != deliveryAck {
		t.Fatalf("delivery = %v; an event with no time is acknowledged and dropped", got)
	}
	if len(rec.events) != 0 {
		t.Fatalf("an event with no time reached the fire table: %+v", rec.events)
	}
}

// The recorder is handed the consumer's clock, which decides whether the event is still inside the window.
func TestProcessHandsTheRecorderTheConsumeTime(t *testing.T) {
	rec := &fakeRecorder{recorded: 1}
	if got := consumerAt(rec, consumedAt).process(context.Background(), []byte(timedEnvelope), time.Time{}); got != deliveryAck {
		t.Fatalf("delivery = %v; want ack", got)
	}
	if len(rec.nows) != 1 || !rec.nows[0].Equal(consumedAt) {
		t.Fatalf("recorder clock = %v; want the consume time %v", rec.nows, consumedAt)
	}
}

func TestNewConsumerNeedsADatabase(t *testing.T) {
	if _, err := NewConsumer(nil); err == nil {
		t.Fatal("a consumer without a database was built")
	}
}

func TestTheIngestionSubjectAndStreamAreThePublishersNames(t *testing.T) {
	if WebhookReceivedSubject != "verevon.ingestion.integration.webhook_received" || WebhookStream != "VEREVON_INGESTION" {
		t.Fatalf("subject %q on stream %q", WebhookReceivedSubject, WebhookStream)
	}
}
