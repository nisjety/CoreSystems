package sessionreview

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/envelope"
	"google.golang.org/grpc"
)

// fakeControls answers GetMemoryControls and records the question.
type fakeControls struct {
	controls *mpv1.MemoryControls
	err      error
	asked    []*mpv1.GetMemoryControlsRequest
}

func (f *fakeControls) GetMemoryControls(_ context.Context, in *mpv1.GetMemoryControlsRequest, _ ...grpc.CallOption) (*mpv1.GetMemoryControlsResponse, error) {
	f.asked = append(f.asked, in)
	if f.err != nil {
		return nil, f.err
	}
	return &mpv1.GetMemoryControlsResponse{Controls: f.controls}, nil
}

// ownerSessionClient is the session client of the review, recording the user
// the existing skills were listed for.
type ownerSessionClient struct {
	fakeSessionClient
	listedFor []string
}

func (f *ownerSessionClient) ListAgentSkills(ctx context.Context, in *mpv1.ListAgentSkillsRequest, opts ...grpc.CallOption) (*mpv1.ListAgentSkillsResponse, error) {
	f.listedFor = append(f.listedFor, in.GetForUserId())
	return f.fakeSessionClient.ListAgentSkills(ctx, in, opts...)
}

func newOwnerSessionClient() *ownerSessionClient {
	return &ownerSessionClient{fakeSessionClient: fakeSessionClient{
		convo:  &mpv1.ListConversationResponse{Messages: []*mpv1.SessionMessage{{Role: "user", Content: "weekly report again"}}},
		skills: &mpv1.ListAgentSkillsResponse{},
	}}
}

const reviewReply = `{"skills":[{"name":"Weekly report","content":"steps","confidence":0.9}]}`

func TestParseRunCompleted_CarriesUserID(t *testing.T) {
	withUser := &envelope.Envelope{
		EventType: RunCompletedEventType, OrgID: "org-9", UserID: "user-7",
		ResourceRef: "run/r-1", Payload: json.RawMessage(`{"thread_id":"t-1"}`),
	}
	ref, ok := ParseRunCompleted(withUser)
	if !ok || ref.UserID != "user-7" {
		t.Fatalf("the run's owner must reach the review: %+v ok=%v", ref, ok)
	}
}

// A paused (or off, or org-disabled) owner is learned from by no one: the
// review asks before it reads a word of the conversation and says why it
// stopped.
func TestHandleRunCompleted_SkipsPausedUserBeforeTranscriptRead(t *testing.T) {
	for _, test := range []struct {
		name     string
		controls *mpv1.MemoryControls
		want     string
	}{
		{"paused", &mpv1.MemoryControls{LearningAllowed: false, Reason: "user_paused"}, "memory_state"},
		{"off", &mpv1.MemoryControls{LearningAllowed: false, Reason: "user_off"}, "memory_state"},
		{"org disabled", &mpv1.MemoryControls{LearningAllowed: false, Reason: "org_disabled"}, "org_policy"},
		{"org has not opted in", &mpv1.MemoryControls{LearningAllowed: false, Reason: "on"}, "org_policy"},
		{"policy unreadable", &mpv1.MemoryControls{LearningAllowed: false, Reason: "policy_unavailable"}, "org_policy"},
		{"unknown reason", &mpv1.MemoryControls{LearningAllowed: false, Reason: "something_new"}, "org_policy"},
	} {
		t.Run(test.name, func(t *testing.T) {
			sc, ic := newOwnerSessionClient(), &fakeInfClient{content: reviewReply}
			controls := &fakeControls{controls: test.controls}
			outcome, err := handleRunCompleted(context.Background(),
				envBytes(t, "RUN_COMPLETED", "run/r-1", "org-7", `{"thread_id":"t-1"}`), sc, ic, "m",
				WithMemoryControls(controls))
			if err != nil || string(outcome.Skipped) != test.want {
				t.Fatalf("outcome=%+v err=%v, want skip %q", outcome, err, test.want)
			}
			if sc.transcriptReads != 0 || len(sc.listedFor) != 0 || len(sc.upserts) != 0 {
				t.Fatalf("a refused owner's conversation was read or written: reads=%d lists=%d upserts=%d", sc.transcriptReads, len(sc.listedFor), len(sc.upserts))
			}
			if len(controls.asked) != 1 || controls.asked[0].GetOrgId() != "org-7" || controls.asked[0].GetUserId() != "user-1" {
				t.Fatalf("controls asked for the wrong user: %+v", controls.asked)
			}
		})
	}
}

// An answer that cannot be had is not a yes: the delivery fails and is retried.
func TestHandleRunCompleted_UnreadableControlsFailClosedAndRetry(t *testing.T) {
	for _, controls := range []*fakeControls{
		{err: errors.New("session core down")},
		{controls: nil},
	} {
		sc := newOwnerSessionClient()
		_, err := handleRunCompleted(context.Background(),
			envBytes(t, "RUN_COMPLETED", "run/r-1", "org-7", `{"thread_id":"t-1"}`), sc, &fakeInfClient{content: reviewReply}, "m",
			WithMemoryControls(controls))
		if err == nil {
			t.Fatal("an unreadable decision was read as permission")
		}
		if sc.transcriptReads != 0 {
			t.Fatal("the conversation was read without a decision")
		}
	}
}

// An envelope without an owner is not reviewed: a proposal would belong to no
// one.
func TestHandleRunCompleted_RunWithoutOwnerIsNotReviewed(t *testing.T) {
	retainable := false
	body, err := json.Marshal(&envelope.Envelope{
		EventType: "RUN_COMPLETED", OrgID: "org-7", ResourceRef: "run/r-1",
		Payload: json.RawMessage(`{"thread_id":"t-1"}`), Zdr: &retainable,
	})
	if err != nil {
		t.Fatal(err)
	}
	sc := newOwnerSessionClient()
	outcome, err := handleRunCompleted(context.Background(), body, sc, &fakeInfClient{content: reviewReply}, "m")
	if err != nil || outcome.Skipped != skipOwnerUnknown || sc.transcriptReads != 0 {
		t.Fatalf("outcome=%+v err=%v reads=%d", outcome, err, sc.transcriptReads)
	}
}

// An allowed owner is reviewed, their existing skills are listed as them, and
// the proposal is written as theirs with the run and thread it came from.
func TestHandleRunCompleted_AllowedOwnerGetsAnOwnedProposal(t *testing.T) {
	sc, ic := newOwnerSessionClient(), &fakeInfClient{content: reviewReply}
	controls := &fakeControls{controls: &mpv1.MemoryControls{LearningAllowed: true, Reason: "on"}}
	outcome, err := handleRunCompleted(context.Background(),
		envBytes(t, "RUN_COMPLETED", "run/r-1", "org-7", `{"thread_id":"t-1"}`), sc, ic, "m",
		WithMemoryControls(controls))
	if err != nil || outcome.Persisted != 1 {
		t.Fatalf("outcome=%+v err=%v", outcome, err)
	}
	if len(sc.listedFor) != 1 || sc.listedFor[0] != "user-1" {
		t.Fatalf("existing skills must be listed as the run's owner: %v", sc.listedFor)
	}
	r := sc.upserts[0]
	if r.GetOwnerUserId() != "user-1" || r.GetSourceRunId() != "r-1" || r.GetSourceThreadId() != "t-1" || r.GetEnabled() || r.GetScope() != "user" || r.GetReviewState() != "proposed" {
		t.Fatalf("not an owned, disabled proposal: %+v", r)
	}
}
