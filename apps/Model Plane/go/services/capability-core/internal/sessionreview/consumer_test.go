package sessionreview

import (
	"context"
	"encoding/json"
	"os"
	"testing"
	"time"

	"github.com/nats-io/nats.go"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"google.golang.org/grpc"
)

// fakeSessionClient satisfies reviewSessionClient (transcript reader + skill
// writer). fakeInfClient satisfies reviewInferenceClient.
//
// transcriptReads counts conversation reads so a test can assert that a
// no-retention run is refused BEFORE any content is read, not merely that
// nothing was persisted afterwards.
type fakeSessionClient struct {
	convo           *mpv1.ListConversationResponse
	skills          *mpv1.ListAgentSkillsResponse
	upserts         []*mpv1.UpsertAgentSkillRequest
	transcriptReads int
}

func (f *fakeSessionClient) ListConversation(_ context.Context, _ *mpv1.ListConversationRequest, _ ...grpc.CallOption) (*mpv1.ListConversationResponse, error) {
	f.transcriptReads++
	return f.convo, nil
}

func (f *fakeSessionClient) ListAgentSkills(_ context.Context, _ *mpv1.ListAgentSkillsRequest, _ ...grpc.CallOption) (*mpv1.ListAgentSkillsResponse, error) {
	return f.skills, nil
}

func (f *fakeSessionClient) UpsertAgentSkill(_ context.Context, in *mpv1.UpsertAgentSkillRequest, _ ...grpc.CallOption) (*mpv1.UpsertAgentSkillResponse, error) {
	f.upserts = append(f.upserts, in)
	return &mpv1.UpsertAgentSkillResponse{Created: true}, nil
}

type fakeInfClient struct{ content string }

func (f *fakeInfClient) Infer(_ context.Context, _ *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	return &mpv1.InferResponse{Content: f.content}, nil
}

// envBytes builds a retainable (`zdr: false`) envelope — the only posture that
// permits a review. Retention must be stated explicitly because absence fails
// closed; see [RetentionPosture].
func envBytes(t *testing.T, eventType, resourceRef, org, payload string) []byte {
	t.Helper()
	retainable := false
	return envBytesWithRetention(t, eventType, resourceRef, org, payload, &retainable)
}

// envBytesWithRetention builds an envelope carrying an explicit `zdr` flag, or
// none at all when zdr is nil.
//
// The flag is injected into the marshalled JSON rather than set on a struct
// field because pkg/envelope.Envelope HAS no `zdr` field — the very gap that
// makes this gate necessary. Injecting it reproduces what a producer at parity
// with the proto `Event` (field 13) and the Rust envelope would put on the wire.
func envBytesWithRetention(t *testing.T, eventType, resourceRef, org, payload string, zdr *bool) []byte {
	t.Helper()
	if payload == "" {
		payload = "{}" // empty RawMessage is invalid JSON
	}
	b, err := json.Marshal(&envelope.Envelope{
		EventType:   eventType,
		OrgID:       org,
		ResourceRef: resourceRef,
		Payload:     json.RawMessage(payload),
	})
	if err != nil {
		t.Fatalf("marshal envelope: %v", err)
	}
	if zdr == nil {
		return b
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(b, &fields); err != nil {
		t.Fatalf("unmarshal envelope for zdr injection: %v", err)
	}
	if *zdr {
		fields["zdr"] = json.RawMessage("true")
	} else {
		fields["zdr"] = json.RawMessage("false")
	}
	withZDR, err := json.Marshal(fields)
	if err != nil {
		t.Fatalf("marshal envelope with zdr: %v", err)
	}
	return withZDR
}

func TestRunCompletedSubjectIsLimitedToCanonicalRunEvents(t *testing.T) {
	if RunCompletedSubject != "mp.v1.run.*.event" {
		t.Fatalf("run-completed subscription = %q, want canonical run-event subject only", RunCompletedSubject)
	}
}

// This is the live-bus proof for the trigger half of G7. The transcript and
// inference boundaries stay fakes so the test does not require credentials or
// an LLM, but the subscription, subject matching, delivery, and cancellation
// all run against a real NATS server supplied by the release harness.
func TestRunConsumerAgainstLiveNATS(t *testing.T) {
	url := os.Getenv("NATS_URL")
	if url == "" {
		t.Skip("requires NATS_URL to a disposable NATS server")
	}
	nc, err := nats.Connect(url)
	if err != nil {
		t.Fatalf("connect NATS: %v", err)
	}
	defer nc.Close()

	sc := &fakeSessionClient{
		convo:  &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{{Role: "user", Content: "live bus"}}},
		skills: &mpv1.ListAgentSkillsResponse{},
	}
	ic := &fakeInfClient{content: `{"skills":[{"name":"LiveBus","content":"retain the verified workflow","confidence":0.95}]}`}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	consumerDone := make(chan error, 1)
	go func() { consumerDone <- RunConsumer(ctx, nc, sc, ic, "test-model") }()

	// Give the asynchronous subscriber a bounded head start, then flush the
	// publish so the callback has a deterministic delivery point.
	time.Sleep(100 * time.Millisecond)
	if err := nc.Publish("mp.v1.run.live-e2e.event", envBytes(t, "RUN_COMPLETED", "run/live-e2e", "org-live", `{"thread_id":"thread-live"}`)); err != nil {
		t.Fatalf("publish RUN_COMPLETED: %v", err)
	}
	if err := nc.Flush(); err != nil {
		t.Fatalf("flush NATS: %v", err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for len(sc.upserts) == 0 && time.Now().Before(deadline) {
		time.Sleep(20 * time.Millisecond)
	}
	if len(sc.upserts) != 1 || sc.upserts[0].GetName() != "LiveBus" {
		t.Fatalf("live RUN_COMPLETED did not persist expected skill: %+v", sc.upserts)
	}
	cancel()
	select {
	case err := <-consumerDone:
		if err != nil {
			t.Fatalf("consumer shutdown: %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("consumer did not stop after context cancellation")
	}
}

// The whole trigger path from a RUN_COMPLETED envelope to a persisted skill,
// faking only the session-core and inference-core boundaries.
func TestHandleRunCompleted_EndToEnd(t *testing.T) {
	sc := &fakeSessionClient{
		convo: &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{
			{Role: "user", Content: "how do I cache?"},
			{Role: "assistant", Content: "use an LRU"},
		}},
		skills: &mpv1.ListAgentSkillsResponse{}, // no existing skills
	}
	ic := &fakeInfClient{content: `{"skills":[{"name":"Caching","content":"use an LRU","confidence":0.9}]}`}

	data := envBytes(t, "RUN_COMPLETED", "run/r-1", "org-7", `{"thread_id":"t-1"}`)
	n, err := HandleRunCompleted(context.Background(), data, sc, ic, "test-model")
	if err != nil {
		t.Fatalf("HandleRunCompleted: %v", err)
	}
	if n != 1 {
		t.Fatalf("expected 1 skill persisted, got %d", n)
	}
	if len(sc.upserts) != 1 || sc.upserts[0].GetName() != "Caching" {
		t.Fatalf("unexpected upserts: %+v", sc.upserts)
	}
	// Provenance forced end-to-end.
	if sc.upserts[0].GetOrigin() != "background_review" {
		t.Fatalf("origin must be background_review, got %q", sc.upserts[0].GetOrigin())
	}
	if sc.upserts[0].GetOrgId() != "org-7" {
		t.Fatalf("org scoping lost: %q", sc.upserts[0].GetOrgId())
	}
}

func TestHandleRunCompleted_NonRunEventIgnored(t *testing.T) {
	data := envBytes(t, "STEP_COMPLETED", "run/r-1", "org-7", "")
	n, err := HandleRunCompleted(context.Background(), data,
		&fakeSessionClient{}, &fakeInfClient{}, "m")
	if err != nil || n != 0 {
		t.Fatalf("non-trigger event must yield 0/nil, got %d / %v", n, err)
	}
}

func TestHandleRunCompleted_BadEnvelopeErrors(t *testing.T) {
	_, err := HandleRunCompleted(context.Background(), []byte("not json"),
		&fakeSessionClient{}, &fakeInfClient{}, "m")
	if err == nil {
		t.Fatal("an undecodable envelope must error")
	}
}

// A run that does not explicitly permit retention must be refused BEFORE the
// transcript is read. Asserting zero reads (not merely zero upserts) is the
// point: a skill distilled from a no-retention conversation is the exact leak
// the platform's ZDR-propagation rule exists to prevent, and reading the content
// at all already crosses the boundary.
func TestHandleRunCompleted_RefusesRunWithoutRetentionPermission(t *testing.T) {
	zdrTrue := true
	for _, tc := range []struct {
		name string
		zdr  *bool
	}{
		{name: "explicit zero data retention", zdr: &zdrTrue},
		// Absent `zdr` fails closed: the Go publish path neither stamps the flag
		// nor suppresses ZDR envelopes, so absence asserts nothing.
		{name: "retention posture unspecified", zdr: nil},
	} {
		t.Run(tc.name, func(t *testing.T) {
			sc := &fakeSessionClient{
				convo: &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{
					{Role: "user", Content: "must-not-be-read"},
				}},
				skills: &mpv1.ListAgentSkillsResponse{},
			}
			ic := &fakeInfClient{content: `{"skills":[{"name":"Leaked","content":"x","confidence":0.9}]}`}

			data := envBytesWithRetention(t, "RUN_COMPLETED", "run/r-1", "org-7",
				`{"thread_id":"t-1"}`, tc.zdr)
			n, err := HandleRunCompleted(context.Background(), data, sc, ic, "test-model")
			if err != nil {
				t.Fatalf("a skipped run is not a failure, got error: %v", err)
			}
			if n != 0 {
				t.Fatalf("persisted %d skills from a run that does not permit retention", n)
			}
			if sc.transcriptReads != 0 {
				t.Fatalf("transcript was read %d time(s); a no-retention run must be refused before any read",
					sc.transcriptReads)
			}
			if len(sc.upserts) != 0 {
				t.Fatalf("wrote %d skill(s) derived from a no-retention run", len(sc.upserts))
			}
		})
	}
}

func TestParseRetentionPostureDistinguishesAbsentFromFalse(t *testing.T) {
	for _, tc := range []struct {
		name  string
		raw   string
		want  RetentionPosture
		allow bool
	}{
		{name: "absent", raw: `{"event_type":"RUN_COMPLETED"}`, want: RetentionUnspecified, allow: false},
		{name: "true", raw: `{"zdr":true}`, want: RetentionZeroData, allow: false},
		{name: "false", raw: `{"zdr":false}`, want: RetentionDurable, allow: true},
		{name: "undecodable", raw: `not json`, want: RetentionUnspecified, allow: false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			got := parseRetentionPosture([]byte(tc.raw))
			if got != tc.want {
				t.Fatalf("posture = %v (%s), want %v (%s)", got, got, tc.want, tc.want)
			}
			if got.AllowsDerivedPersistence() != tc.allow {
				t.Fatalf("AllowsDerivedPersistence() = %v, want %v", !tc.allow, tc.allow)
			}
		})
	}
}
