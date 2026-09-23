package main

import (
	"os"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
)

func TestTopologySpecsCoverRuntimeBindings(t *testing.T) {
	streams := streamConfigs()
	if len(streams) != 3 {
		t.Fatalf("stream count = %d, want 3", len(streams))
	}
	for _, stream := range streams {
		if stream.Name == "" || len(stream.Subjects) != 1 || stream.MaxAge == 0 {
			t.Fatalf("invalid stream spec: %+v", stream)
		}
		if stream.Storage != nats.FileStorage || stream.Replicas != 1 {
			t.Fatalf("runtime streams must be durable single-replica contracts: %+v", stream)
		}
	}
	bindings := consumerBindings()
	if len(bindings) != 4 {
		t.Fatalf("consumer count = %d, want 4", len(bindings))
	}
	if bindings[0].config.AckPolicy != nats.AckExplicitPolicy || bindings[0].config.AckWait != 30*time.Second {
		t.Fatalf("tool completion consumer must retry explicit work: %+v", bindings[0].config)
	}
	if bindings[1].config.AckPolicy != nats.AckNonePolicy {
		t.Fatalf("orchestration bridge must use at-most-once fan-out: %+v", bindings[1].config)
	}
	// AUTO-2 run-watch notify consumer: its side effect is an outbound
	// cross-plane HTTP call to notification-core, so — unlike the
	// at-most-once orchestration bridge above — a failure must be retried,
	// not silently dropped, and a fresh deployment should not have to replay
	// 48h of run history to find watchers that could not have existed yet.
	runWatch := bindings[2]
	if runWatch.stream != "MODEL_PLANE_RUN_EVENTS" || runWatch.config.Durable != "capability-core-run-watch-notify" {
		t.Fatalf("run-watch binding = %+v, want MODEL_PLANE_RUN_EVENTS/capability-core-run-watch-notify", runWatch)
	}
	if runWatch.config.AckPolicy != nats.AckExplicitPolicy || runWatch.config.MaxDeliver <= 0 {
		t.Fatalf("run-watch notify consumer must retry a failed delegated notification: %+v", runWatch.config)
	}
	if runWatch.config.DeliverPolicy != nats.DeliverNewPolicy {
		t.Fatalf("run-watch notify consumer should not replay run history on first boot: %+v", runWatch.config)
	}
	if runWatch.config.FilterSubject != "mp.v1.run.*.event" {
		t.Fatalf("run-watch notify consumer filter = %q, want mp.v1.run.*.event", runWatch.config.FilterSubject)
	}
	// A consumer with no DeliverSubject is a server-side PULL consumer, and
	// this Go client binds to it via the push-style
	// QueueSubscribe+nats.Bind (internal/runwatch.Notifier.Run) — verified
	// directly against the pinned nats.go version on 2026-09-17 that this
	// combination fails outright ("must use pull subscribe to bind to pull
	// based consumer") without an explicit DeliverSubject. This durable had
	// shipped without one; it had simply never been exercised live (a
	// separate NATS-connect-retry bug meant capability-core's connection to
	// NATS itself had never succeeded).
	if runWatch.config.DeliverSubject == "" {
		t.Fatalf("run-watch notify consumer has no DeliverSubject; QueueSubscribe+Bind will fail to bind a pull consumer: %+v", runWatch.config)
	}

	// G7 skill-learning review: the newest durable on this stream. Unlike
	// run-watch above, this one MUST replay history on first bind -- the
	// bug it fixes is exactly "messages this consumer did not exist to
	// receive yet", so DeliverNewPolicy would silently repeat that failure
	// for anything published before this durable was first provisioned.
	// AckWait is longer than every sibling consumer on this stream because
	// its handler makes a real LLM call, not a cheap local read/write or a
	// single outbound HTTP call.
	skillReview := bindings[3]
	if skillReview.stream != "MODEL_PLANE_RUN_EVENTS" || skillReview.config.Durable != "capability-core-skill-review" {
		t.Fatalf("skill-review binding = %+v, want MODEL_PLANE_RUN_EVENTS/capability-core-skill-review", skillReview)
	}
	if skillReview.config.AckPolicy != nats.AckExplicitPolicy || skillReview.config.MaxDeliver <= 0 {
		t.Fatalf("skill-review consumer must retry a failed review: %+v", skillReview.config)
	}
	if skillReview.config.DeliverPolicy != nats.DeliverAllPolicy {
		t.Fatalf("skill-review consumer must replay run history to recover missed reviews: %+v", skillReview.config)
	}
	if skillReview.config.AckWait < 60*time.Second {
		t.Fatalf("skill-review consumer AckWait too short for a real LLM review call: %s", skillReview.config.AckWait)
	}
	if skillReview.config.FilterSubject != "mp.v1.run.*.event" {
		t.Fatalf("skill-review consumer filter = %q, want mp.v1.run.*.event", skillReview.config.FilterSubject)
	}
	if skillReview.config.DeliverSubject == "" {
		t.Fatalf("skill-review consumer has no DeliverSubject; QueueSubscribe+Bind will fail to bind a pull consumer: %+v", skillReview.config)
	}
	if runWatch.config.DeliverSubject == skillReview.config.DeliverSubject {
		t.Fatalf("run-watch and skill-review must not share a DeliverSubject: %q", runWatch.config.DeliverSubject)
	}
}

func TestSameStringsIsOrderSensitive(t *testing.T) {
	if !sameStrings([]string{"a", "b"}, []string{"a", "b"}) {
		t.Fatal("equal subjects should match")
	}
	if sameStrings([]string{"a", "b"}, []string{"b", "a"}) {
		t.Fatal("subject order drift must be rejected")
	}
}

// The exact live-production scenario this self-heal exists for: a durable
// that was provisioned before this file required push delivery (no
// DeliverSubject/DeliverGroup) — capability-core-run-watch-notify shipped
// exactly this way and was verified live on 2026-09-17 to be un-bindable by
// its own Go consumer as a result. ensureConsumer must delete and recreate
// it rather than fail closed, and the RESULT must actually be bindable.
func TestEnsureConsumerSelfHealsAPreExistingPullConsumer(t *testing.T) {
	url := os.Getenv("NATS_URL")
	if url == "" {
		t.Skip("requires NATS_URL to a disposable NATS+JetStream server")
	}
	nc, err := nats.Connect(url)
	if err != nil {
		t.Fatalf("connect NATS: %v", err)
	}
	defer nc.Close()
	js, err := nc.JetStream()
	if err != nil {
		t.Fatalf("JetStream context: %v", err)
	}

	stream := "SELF_HEAL_TEST_STREAM"
	subject := "self-heal.run.*.event"
	durable := "self-heal-test-durable"
	// Start from a clean slate: this test seeds a specific PRE-heal state
	// (a pull consumer) and asserts on it, so it must not inherit an
	// already-healed stream/consumer left behind by a prior run against the
	// same disposable server (best-effort; ErrStreamNotFound on a fresh
	// server is expected and ignored).
	_ = js.DeleteStream(stream)
	if err := ensureStream(js, nats.StreamConfig{
		Name:      stream,
		Subjects:  []string{subject},
		Retention: nats.LimitsPolicy,
		Storage:   nats.MemoryStorage,
		MaxAge:    time.Hour,
	}); err != nil {
		t.Fatalf("ensure stream: %v", err)
	}

	// Simulate the pre-existing production state directly: a pull consumer
	// (no DeliverSubject), created the way an OLDER version of this contract
	// (before DeliverSubject was required) would have.
	if _, err := js.AddConsumer(stream, &nats.ConsumerConfig{
		Durable:       durable,
		AckPolicy:     nats.AckExplicitPolicy,
		AckWait:       30 * time.Second,
		MaxDeliver:    5,
		FilterSubject: subject,
		ReplayPolicy:  nats.ReplayInstantPolicy,
		DeliverPolicy: nats.DeliverNewPolicy,
	}); err != nil {
		t.Fatalf("seed stale pull consumer: %v", err)
	}
	preInfo, err := js.ConsumerInfo(stream, durable)
	if err != nil {
		t.Fatalf("consumer info before heal: %v", err)
	}
	if preInfo.Config.DeliverSubject != "" {
		t.Fatalf("test setup bug: seeded consumer is not actually pull-based: %+v", preInfo.Config)
	}

	expected := consumerBinding{
		stream: stream,
		config: nats.ConsumerConfig{
			Durable:        durable,
			DeliverSubject: "deliver." + durable,
			DeliverGroup:   durable,
			AckPolicy:      nats.AckExplicitPolicy,
			AckWait:        30 * time.Second,
			MaxDeliver:     5,
			FilterSubject:  subject,
			ReplayPolicy:   nats.ReplayInstantPolicy,
			DeliverPolicy:  nats.DeliverNewPolicy,
		},
	}
	if err := ensureConsumer(js, expected); err != nil {
		t.Fatalf("ensureConsumer did not self-heal the stale pull consumer: %v", err)
	}

	// Running it again (the normal "already correct" path) must be a no-op,
	// not a second recreation.
	if err := ensureConsumer(js, expected); err != nil {
		t.Fatalf("ensureConsumer on an already-healed consumer should be a no-op: %v", err)
	}

	postInfo, err := js.ConsumerInfo(stream, durable)
	if err != nil {
		t.Fatalf("consumer info after heal: %v", err)
	}
	if postInfo.Config.DeliverSubject != expected.config.DeliverSubject {
		t.Fatalf("DeliverSubject not healed: got %q, want %q", postInfo.Config.DeliverSubject, expected.config.DeliverSubject)
	}

	// The real proof: the healed consumer must actually be bindable by the
	// same push-style call production code uses — this is the whole point,
	// not just that the stored config field changed.
	sub, err := js.QueueSubscribe(subject, durable, func(msg *nats.Msg) {
		_ = msg.Ack()
	}, nats.Bind(stream, durable), nats.ManualAck())
	if err != nil {
		t.Fatalf("healed consumer is still not bindable: %v", err)
	}
	defer sub.Unsubscribe()
}

// TestEnsureConsumerSelfHealsARenamedDeliverSubject covers the drift shape
// found live on 2026-09-17: a consumer already provisioned as push-mode, but
// under an earlier DeliverSubject/DeliverGroup naming convention than the
// current contract, with zero prior deliveries (every delivery attempt had
// failed at the NATS ACL layer, so nothing was ever actually received). This
// must self-heal the same way an empty-DeliverSubject pull consumer does —
// the earlier, narrower check only matched DeliverSubject == "".
func TestEnsureConsumerSelfHealsARenamedDeliverSubject(t *testing.T) {
	url := os.Getenv("NATS_URL")
	if url == "" {
		t.Skip("requires NATS_URL to a disposable NATS+JetStream server")
	}
	nc, err := nats.Connect(url)
	if err != nil {
		t.Fatalf("connect NATS: %v", err)
	}
	defer nc.Close()
	js, err := nc.JetStream()
	if err != nil {
		t.Fatalf("JetStream context: %v", err)
	}

	stream := "SELF_HEAL_RENAME_TEST_STREAM"
	subject := "self-heal-rename.run.*.event"
	durable := "self-heal-rename-test-durable"
	_ = js.DeleteStream(stream)
	if err := ensureStream(js, nats.StreamConfig{
		Name:      stream,
		Subjects:  []string{subject},
		Retention: nats.LimitsPolicy,
		Storage:   nats.MemoryStorage,
		MaxAge:    time.Hour,
	}); err != nil {
		t.Fatalf("ensure stream: %v", err)
	}

	// Seed the consumer under an OLDER, since-abandoned DeliverSubject/
	// DeliverGroup naming — already push-mode, unlike the pull-consumer test
	// above, but still the wrong subject for the current contract.
	oldConfig := nats.ConsumerConfig{
		Durable:        durable,
		DeliverSubject: "deliver." + durable,
		DeliverGroup:   durable,
		AckPolicy:      nats.AckExplicitPolicy,
		AckWait:        30 * time.Second,
		MaxDeliver:     5,
		FilterSubject:  subject,
		ReplayPolicy:   nats.ReplayInstantPolicy,
		DeliverPolicy:  nats.DeliverNewPolicy,
	}
	if _, err := js.AddConsumer(stream, &oldConfig); err != nil {
		t.Fatalf("seed renamed-subject consumer: %v", err)
	}

	expected := consumerBinding{
		stream: stream,
		config: nats.ConsumerConfig{
			Durable:        durable,
			DeliverSubject: "_VEREVON.TEST.DELIVER." + durable,
			DeliverGroup:   "current-" + durable,
			AckPolicy:      nats.AckExplicitPolicy,
			AckWait:        30 * time.Second,
			MaxDeliver:     5,
			FilterSubject:  subject,
			ReplayPolicy:   nats.ReplayInstantPolicy,
			DeliverPolicy:  nats.DeliverNewPolicy,
		},
	}
	if err := ensureConsumer(js, expected); err != nil {
		t.Fatalf("ensureConsumer did not self-heal the renamed-subject consumer: %v", err)
	}

	postInfo, err := js.ConsumerInfo(stream, durable)
	if err != nil {
		t.Fatalf("consumer info after heal: %v", err)
	}
	if postInfo.Config.DeliverSubject != expected.config.DeliverSubject || postInfo.Config.DeliverGroup != expected.config.DeliverGroup {
		t.Fatalf("delivery subject/group not healed: got subject=%q group=%q, want subject=%q group=%q",
			postInfo.Config.DeliverSubject, postInfo.Config.DeliverGroup,
			expected.config.DeliverSubject, expected.config.DeliverGroup)
	}

	sub, err := js.QueueSubscribe(subject, expected.config.DeliverGroup, func(msg *nats.Msg) {
		_ = msg.Ack()
	}, nats.Bind(stream, durable), nats.ManualAck())
	if err != nil {
		t.Fatalf("healed consumer is still not bindable: %v", err)
	}
	defer sub.Unsubscribe()
}

// TestEnsureConsumerRefusesToHealAfterRealDeliveries proves the safety
// boundary on the other side: once a consumer has actually delivered at
// least one message, a DeliverSubject/DeliverGroup mismatch must fail loudly
// rather than be silently recreated, since recreating would discard real
// delivery/ack progress.
func TestEnsureConsumerRefusesToHealAfterRealDeliveries(t *testing.T) {
	url := os.Getenv("NATS_URL")
	if url == "" {
		t.Skip("requires NATS_URL to a disposable NATS+JetStream server")
	}
	nc, err := nats.Connect(url)
	if err != nil {
		t.Fatalf("connect NATS: %v", err)
	}
	defer nc.Close()
	js, err := nc.JetStream()
	if err != nil {
		t.Fatalf("JetStream context: %v", err)
	}

	stream := "SELF_HEAL_REFUSE_TEST_STREAM"
	subject := "self-heal-refuse.run.*.event"
	durable := "self-heal-refuse-test-durable"
	_ = js.DeleteStream(stream)
	if err := ensureStream(js, nats.StreamConfig{
		Name:      stream,
		Subjects:  []string{subject},
		Retention: nats.LimitsPolicy,
		Storage:   nats.MemoryStorage,
		MaxAge:    time.Hour,
	}); err != nil {
		t.Fatalf("ensure stream: %v", err)
	}

	liveConfig := nats.ConsumerConfig{
		Durable:        durable,
		DeliverSubject: "deliver." + durable,
		DeliverGroup:   durable,
		AckPolicy:      nats.AckExplicitPolicy,
		AckWait:        30 * time.Second,
		MaxDeliver:     5,
		FilterSubject:  subject,
		ReplayPolicy:   nats.ReplayInstantPolicy,
		DeliverPolicy:  nats.DeliverAllPolicy,
	}
	if _, err := js.AddConsumer(stream, &liveConfig); err != nil {
		t.Fatalf("seed live consumer: %v", err)
	}

	delivered := make(chan struct{}, 1)
	sub, err := js.QueueSubscribe(subject, durable, func(msg *nats.Msg) {
		_ = msg.Ack()
		select {
		case delivered <- struct{}{}:
		default:
		}
	}, nats.Bind(stream, durable), nats.ManualAck())
	if err != nil {
		t.Fatalf("bind live consumer: %v", err)
	}
	if _, err := js.Publish("self-heal-refuse.run.1.event", []byte("payload")); err != nil {
		t.Fatalf("publish: %v", err)
	}
	select {
	case <-delivered:
	case <-time.After(5 * time.Second):
		t.Fatal("timed out waiting for the seeded consumer to actually receive a message")
	}
	sub.Unsubscribe()

	expected := consumerBinding{
		stream: stream,
		config: nats.ConsumerConfig{
			Durable:        durable,
			DeliverSubject: "_VEREVON.TEST.DELIVER." + durable,
			DeliverGroup:   "current-" + durable,
			AckPolicy:      nats.AckExplicitPolicy,
			AckWait:        30 * time.Second,
			MaxDeliver:     5,
			FilterSubject:  subject,
			ReplayPolicy:   nats.ReplayInstantPolicy,
			DeliverPolicy:  nats.DeliverAllPolicy,
		},
	}
	if err := ensureConsumer(js, expected); err == nil {
		t.Fatal("ensureConsumer silently healed a consumer with real prior deliveries; it must fail loudly instead")
	}
}
