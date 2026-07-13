package events

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

type fakeOutboxStore struct {
	rows       []OutboxRow
	delivered  []int64
	retried    []int64
	claimErr   error
	deliverErr error
	retryErr   error
}

func (s *fakeOutboxStore) Claim(context.Context, string, int, time.Duration) ([]OutboxRow, error) {
	return append([]OutboxRow(nil), s.rows...), s.claimErr
}
func (s *fakeOutboxStore) MarkDelivered(_ context.Context, id int64, _ string) error {
	s.delivered = append(s.delivered, id)
	return s.deliverErr
}
func (s *fakeOutboxStore) Retry(_ context.Context, id int64, _ string, _ time.Duration, _ string) error {
	s.retried = append(s.retried, id)
	return s.retryErr
}

func TestOutboxPropagatesStoreFailuresAndBoundsRetryMetadata(t *testing.T) {
	for _, store := range []*fakeOutboxStore{
		{claimErr: errors.New("claim")},
		{rows: []OutboxRow{{ID: 1, EventType: SubjectWikiPublished, Payload: []byte("ok")}}, deliverErr: errors.New("deliver")},
		{rows: []OutboxRow{{ID: 1, EventType: SubjectWikiPublished, Payload: []byte("retry")}}, retryErr: errors.New("retry")},
	} {
		publisher := &fakeAcknowledgedPublisher{failIDs: map[string]bool{"retry": true}}
		if err := newOutboxPublisher(store, publisher, "worker", time.Millisecond).drainOnce(context.Background()); err == nil {
			t.Fatal("outbox swallowed durable store failure")
		}
	}
	if retryDelay(0) != time.Second || retryDelay(99) != 256*time.Second {
		t.Fatalf("unexpected bounded retry delay: %v %v", retryDelay(0), retryDelay(99))
	}
	long := errors.New(strings.Repeat("x", 700))
	if got := sanitizeOutboxError(long); len(got) != 512 {
		t.Fatalf("sanitized error length = %d", len(got))
	}
}

type fakeAcknowledgedPublisher struct {
	failIDs    map[string]bool
	messageIDs []string
}

func (p *fakeAcknowledgedPublisher) PublishAcknowledged(eventType string, payload []byte, messageID string) error {
	if eventType != SubjectWikiPublished {
		return errors.New("wrong subject")
	}
	if p.failIDs[string(payload)] {
		return errors.New("broker unavailable")
	}
	p.messageIDs = append(p.messageIDs, messageID)
	return nil
}

func TestOutboxMarksOnlyJetStreamAcknowledgedRowsDelivered(t *testing.T) {
	store := &fakeOutboxStore{rows: []OutboxRow{
		{ID: 1, EventType: SubjectWikiPublished, Payload: []byte("ok"), Attempts: 1},
		{ID: 2, EventType: SubjectWikiPublished, Payload: []byte("retry"), Attempts: 2},
	}}
	publisher := &fakeAcknowledgedPublisher{failIDs: map[string]bool{"retry": true}}
	outbox := newOutboxPublisher(store, publisher, "worker-test", time.Millisecond)

	if err := outbox.drainOnce(context.Background()); err != nil {
		t.Fatal(err)
	}
	if len(store.delivered) != 1 || store.delivered[0] != 1 {
		t.Fatalf("delivered = %v", store.delivered)
	}
	if len(store.retried) != 1 || store.retried[0] != 2 {
		t.Fatalf("retried = %v", store.retried)
	}
	if len(publisher.messageIDs) != 1 || publisher.messageIDs[0] != "wiki-outbox-1" {
		t.Fatalf("message ids = %v", publisher.messageIDs)
	}
}

type fakeSigner struct{}

func (fakeSigner) Sign(_ string, payload []byte) ([]byte, error) {
	return append([]byte("signed:"), payload...), nil
}

type fakeJetStream struct {
	ack *nats.PubAck
	err error
}

func (j *fakeJetStream) Publish(string, []byte, ...nats.PubOpt) (*nats.PubAck, error) {
	return j.ack, j.err
}

func TestPublisherRequiresAValidJetStreamPubAck(t *testing.T) {
	event := WikiVersionPublishedEvent{PageID: "page", VersionID: "version", OrgID: "org", Content: "content", ZDR: false}
	for _, js := range []*fakeJetStream{
		{err: errors.New("no stream")},
		{ack: nil},
		{ack: &nats.PubAck{}},
	} {
		publisher := &Publisher{js: js, signer: fakeSigner{}}
		if err := publisher.PublishWikiVersionPublished(event); err == nil {
			t.Fatal("publisher accepted a missing/invalid JetStream acknowledgement")
		}
	}
	publisher := &Publisher{js: &fakeJetStream{ack: &nats.PubAck{Stream: WikiStream}}, signer: fakeSigner{}}
	if err := publisher.PublishWikiVersionPublished(event); err != nil {
		t.Fatalf("valid acknowledgement rejected: %v", err)
	}
}

func TestPublishAcknowledgedRejectsWrongSubjectAndMalformedPayload(t *testing.T) {
	publisher := &Publisher{js: &fakeJetStream{ack: &nats.PubAck{Stream: WikiStream}}, signer: fakeSigner{}}
	if err := publisher.PublishAcknowledged("dataplane.documents.created", []byte(`{}`), "wiki-outbox-1"); err == nil {
		t.Fatal("wrong subject accepted")
	}
	if err := publisher.PublishAcknowledged(SubjectWikiPublished, []byte(`not-json`), "wiki-outbox-1"); err == nil {
		t.Fatal("malformed payload accepted")
	}
	if err := publisher.PublishAcknowledged(SubjectWikiPublished, []byte(`{"page_id":"p","version_id":"v","org_id":"o","content":"c","zdr":false}`), "wiki-outbox-1"); err != nil {
		t.Fatalf("valid outbox payload rejected: %v", err)
	}
}
