package events

import (
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

type fakeSourceStreamManager struct {
	info      *nats.StreamInfo
	infoErr   error
	added     *nats.StreamConfig
	addErr    error
	addResult *nats.StreamInfo
}

func (f *fakeSourceStreamManager) StreamInfo(_ string, _ ...nats.JSOpt) (*nats.StreamInfo, error) {
	return f.info, f.infoErr
}

func (f *fakeSourceStreamManager) AddStream(cfg *nats.StreamConfig, _ ...nats.JSOpt) (*nats.StreamInfo, error) {
	copy := *cfg
	copy.Subjects = append([]string(nil), cfg.Subjects...)
	f.added = &copy
	return f.addResult, f.addErr
}

type fakeJetStreamPublisher struct {
	ack   *nats.PubAck
	err   error
	calls int
	opts  int
}

func (f *fakeJetStreamPublisher) Publish(_ string, _ []byte, opts ...nats.PubOpt) (*nats.PubAck, error) {
	f.calls++
	f.opts = len(opts)
	return f.ack, f.err
}

type fakeEventSigner struct{ err error }

func (f fakeEventSigner) Sign(_ string, payload []byte) ([]byte, error) {
	if f.err != nil {
		return nil, f.err
	}
	return append([]byte("signed:"), payload...), nil
}

func TestOutboxRequiresJetStreamAcknowledgementBeforeDelivery(t *testing.T) {
	if publisher, err := NewOutboxPublisher(nil, nil, nil); err == nil || publisher != nil {
		t.Fatal("outbox constructor accepted unavailable production dependencies")
	}
	t.Run("acknowledged", func(t *testing.T) {
		js := &fakeJetStreamPublisher{ack: &nats.PubAck{Stream: "DATAPLANE_DOCUMENTS", Sequence: 7}}
		publisher := &OutboxPublisher{js: js, signer: fakeEventSigner{}}
		if err := publisher.publishAcknowledged("dataplane.documents.deleted", []byte(`{"org_id":"org-a","zdr":false}`), "documents-outbox-7"); err != nil {
			t.Fatalf("acknowledged publish failed: %v", err)
		}
		if js.calls != 1 {
			t.Fatalf("publish calls=%d, want 1", js.calls)
		}
		if js.opts != 1 {
			t.Fatalf("publish options=%d, want stable message id", js.opts)
		}
	})

	for _, tc := range []struct {
		name   string
		js     *fakeJetStreamPublisher
		signer fakeEventSigner
	}{
		{name: "signing fails", js: &fakeJetStreamPublisher{}, signer: fakeEventSigner{err: errors.New("sign")}},
		{name: "broker rejects", js: &fakeJetStreamPublisher{err: errors.New("broker")}, signer: fakeEventSigner{}},
		{name: "missing ack", js: &fakeJetStreamPublisher{}, signer: fakeEventSigner{}},
		{name: "empty stream ack", js: &fakeJetStreamPublisher{ack: &nats.PubAck{}}, signer: fakeEventSigner{}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			publisher := &OutboxPublisher{js: tc.js, signer: tc.signer}
			if err := publisher.publishAcknowledged("dataplane.documents.deleted", []byte(`{}`), "documents-outbox-8"); err == nil {
				t.Fatal("unacknowledged publish was accepted")
			}
		})
	}
}

func TestSourceObjectStreamIsBoundedAndFailClosed(t *testing.T) {
	wantSubjects := []string{SubjectSourceObjectChanged, SubjectSourceObjectDeleted}
	cfg := sourceObjectStreamConfig()
	if cfg.Name != sourceObjectStreamName || !reflect.DeepEqual(cfg.Subjects, wantSubjects) {
		t.Fatalf("unexpected source stream identity: %#v", cfg)
	}
	if cfg.Retention != nats.LimitsPolicy || cfg.Storage != nats.FileStorage || cfg.Discard != nats.DiscardOld ||
		cfg.MaxAge != 7*24*time.Hour || cfg.MaxMsgs != 100_000 || cfg.MaxBytes != 256*1024*1024 ||
		cfg.MaxMsgSize != 1024*1024 || cfg.Duplicates != 10*time.Minute || !cfg.DenyDelete || !cfg.DenyPurge {
		t.Fatalf("source stream is not bounded and protected: %#v", cfg)
	}

	t.Run("creates absent owned stream", func(t *testing.T) {
		manager := &fakeSourceStreamManager{
			infoErr:   nats.ErrStreamNotFound,
			addResult: &nats.StreamInfo{Config: cfg},
		}
		if err := ensureSourceObjectStream(manager); err != nil {
			t.Fatalf("ensure source stream: %v", err)
		}
		if manager.added == nil || manager.added.Name != sourceObjectStreamName {
			t.Fatal("missing owned source stream creation")
		}
	})

	t.Run("accepts matching existing stream", func(t *testing.T) {
		manager := &fakeSourceStreamManager{info: &nats.StreamInfo{Config: cfg}}
		if err := ensureSourceObjectStream(manager); err != nil {
			t.Fatalf("matching source stream rejected: %v", err)
		}
		if manager.added != nil {
			t.Fatal("matching source stream was recreated")
		}
	})

	t.Run("rejects unsafe existing stream", func(t *testing.T) {
		unsafe := cfg
		unsafe.MaxAge = 0
		manager := &fakeSourceStreamManager{info: &nats.StreamInfo{Config: unsafe}}
		if err := ensureSourceObjectStream(manager); err == nil {
			t.Fatal("unbounded existing source stream was accepted")
		}
	})

	t.Run("fails closed when broker inspection fails", func(t *testing.T) {
		manager := &fakeSourceStreamManager{infoErr: errors.New("unavailable")}
		if err := ensureSourceObjectStream(manager); err == nil {
			t.Fatal("broker inspection failure was accepted")
		}
	})

	t.Run("fails closed without a manager", func(t *testing.T) {
		if err := ensureSourceObjectStream(nil); err == nil {
			t.Fatal("missing stream manager was accepted")
		}
	})

	t.Run("rejects a create response without stream configuration", func(t *testing.T) {
		manager := &fakeSourceStreamManager{infoErr: nats.ErrStreamNotFound}
		if err := ensureSourceObjectStream(manager); err == nil {
			t.Fatal("empty stream creation response was accepted")
		}
	})
}
