package proxy

import (
	"context"
	"errors"
	"io"
	"testing"

	"google.golang.org/grpc"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// --- fakes ---

type fakeSession struct {
	appendCalls []*mpv1.AppendMessageRequest
	startCalls  []*mpv1.StartRunRequest
	ctxCalls    []*mpv1.GetContextAssemblyRequest
	runID       string
	segments    []*mpv1.ContextSegment
	failOn      string // "append-user", "append-assistant", "start", "context"
}

func (f *fakeSession) AppendMessage(_ context.Context, in *mpv1.AppendMessageRequest, _ ...grpc.CallOption) (*mpv1.AppendMessageResponse, error) {
	f.appendCalls = append(f.appendCalls, in)
	if f.failOn == "append-user" && in.GetRole() == "user" {
		return nil, errors.New("boom")
	}
	if f.failOn == "append-assistant" && in.GetRole() == "assistant" {
		return nil, errors.New("boom")
	}
	return &mpv1.AppendMessageResponse{}, nil
}

func (f *fakeSession) StartRun(_ context.Context, in *mpv1.StartRunRequest, _ ...grpc.CallOption) (*mpv1.StartRunResponse, error) {
	f.startCalls = append(f.startCalls, in)
	if f.failOn == "start" {
		return nil, errors.New("boom")
	}
	return &mpv1.StartRunResponse{RunId: f.runID}, nil
}

func (f *fakeSession) GetContextAssembly(_ context.Context, in *mpv1.GetContextAssemblyRequest, _ ...grpc.CallOption) (*mpv1.GetContextAssemblyResponse, error) {
	f.ctxCalls = append(f.ctxCalls, in)
	if f.failOn == "context" {
		return nil, errors.New("boom")
	}
	return &mpv1.GetContextAssemblyResponse{Segments: f.segments}, nil
}

type fakeInference struct {
	inferReq     *mpv1.InferRequest
	inferResp    *mpv1.InferResponse
	streamReq    *mpv1.InferRequest
	streamChunks []*mpv1.InferChunk
	failInfer    bool
	failStream   bool
}

func (f *fakeInference) Infer(_ context.Context, in *mpv1.InferRequest, _ ...grpc.CallOption) (*mpv1.InferResponse, error) {
	f.inferReq = in
	if f.failInfer {
		return nil, errors.New("boom")
	}
	return f.inferResp, nil
}

func (f *fakeInference) InferStream(_ context.Context, in *mpv1.InferRequest, _ ...grpc.CallOption) (grpc.ServerStreamingClient[mpv1.InferChunk], error) {
	f.streamReq = in
	if f.failStream {
		return nil, errors.New("boom")
	}
	return &fakeInferStream{chunks: f.streamChunks}, nil
}

type fakeInferStream struct {
	grpc.ServerStreamingClient[mpv1.InferChunk]
	chunks []*mpv1.InferChunk
	idx    int
}

func (s *fakeInferStream) Recv() (*mpv1.InferChunk, error) {
	if s.idx >= len(s.chunks) {
		return nil, io.EOF
	}
	c := s.chunks[s.idx]
	s.idx++
	return c, nil
}

type fakeSink struct {
	ctx  context.Context
	sent []*mpv1.InvokeChunk
}

func (f *fakeSink) Send(c *mpv1.InvokeChunk) error { f.sent = append(f.sent, c); return nil }
func (f *fakeSink) Context() context.Context       { return f.ctx }

type storedEntry struct{ prompt, org, model, response string }

type fakeCache struct {
	hitResp   string
	hit       bool
	lookupErr error
	lookups   int
	stored    []storedEntry
}

func (f *fakeCache) Lookup(_ context.Context, _, _, _ string) (string, bool, error) {
	f.lookups++
	return f.hitResp, f.hit, f.lookupErr
}

func (f *fakeCache) Store(_ context.Context, prompt, org, model, response string) error {
	f.stored = append(f.stored, storedEntry{prompt, org, model, response})
	return nil
}

// --- tests ---

func TestInvoke_FullHappyPath(t *testing.T) {
	sess := &fakeSession{
		runID: "run-42",
		segments: []*mpv1.ContextSegment{
			{Kind: "policy", Content: "be helpful"},
			{Kind: "thread", Content: "earlier turn"},
		},
	}
	inf := &fakeInference{
		inferResp: &mpv1.InferResponse{
			RequestId:    "req-1",
			Content:      "hello back",
			ModelUsed:    "claude-sonnet-4",
			StopReason:   "end_turn",
			InputTokens:  10,
			OutputTokens: 5,
		},
	}
	p := New(sess, inf)

	resp, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{
		RequestId:   "req-1",
		OrgId:       "org-1",
		ThreadId:    "thread-1",
		Content:     "hello",
		Model:       "claude-sonnet-4",
		Provider:    "anthropic",
		MaxTokens:   100,
		Temperature: 0.5,
	})
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}

	// Two AppendMessage calls: user then assistant.
	if len(sess.appendCalls) != 2 {
		t.Fatalf("appendCalls = %d, want 2", len(sess.appendCalls))
	}
	if sess.appendCalls[0].GetRole() != "user" || sess.appendCalls[0].GetContent() != "hello" {
		t.Errorf("first AppendMessage = %+v, want user/hello", sess.appendCalls[0])
	}
	if sess.appendCalls[1].GetRole() != "assistant" || sess.appendCalls[1].GetContent() != "hello back" {
		t.Errorf("second AppendMessage = %+v, want assistant/hello back", sess.appendCalls[1])
	}

	// StartRun carries thread + goal + org.
	if len(sess.startCalls) != 1 || sess.startCalls[0].GetThreadId() != "thread-1" ||
		sess.startCalls[0].GetGoal() != "hello" || sess.startCalls[0].GetOrgId() != "org-1" {
		t.Errorf("startCalls = %+v", sess.startCalls)
	}

	// GetContextAssembly carries thread + runID + max_tokens.
	if len(sess.ctxCalls) != 1 || sess.ctxCalls[0].GetThreadId() != "thread-1" ||
		sess.ctxCalls[0].GetRunId() != "run-42" || sess.ctxCalls[0].GetMaxTokens() != 100 {
		t.Errorf("ctxCalls = %+v", sess.ctxCalls)
	}

	// Infer received translated request with assembled messages
	// (2 system segments + 1 fallback user).
	if inf.inferReq == nil {
		t.Fatal("inference Infer was not called")
	}
	if inf.inferReq.GetProviderHint() != "anthropic" || inf.inferReq.GetModel() != "claude-sonnet-4" {
		t.Errorf("inferReq routing fields = %+v", inf.inferReq)
	}
	if got := len(inf.inferReq.GetMessages()); got != 3 {
		t.Fatalf("inferReq messages = %d, want 3", got)
	}
	last := inf.inferReq.GetMessages()[2]
	if last.GetRole() != "user" || last.GetContent() != "hello" {
		t.Errorf("last assembled message = %+v, want user/hello", last)
	}

	// Response is the translated InferResponse.
	if resp.GetContent() != "hello back" || resp.GetModelUsed() != "claude-sonnet-4" ||
		resp.GetStopReason() != "end_turn" || resp.GetInputTokens() != 10 || resp.GetOutputTokens() != 5 {
		t.Errorf("InvokeResponse = %+v", resp)
	}
}

func TestInvoke_RequiresThreadID(t *testing.T) {
	p := New(&fakeSession{}, &fakeInference{})
	_, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{Content: "hi"})
	if err == nil {
		t.Fatal("want error for missing thread_id")
	}
}

func TestInvoke_PropagatesAppendUserError(t *testing.T) {
	p := New(&fakeSession{failOn: "append-user"}, &fakeInference{})
	_, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{ThreadId: "t", Content: "x"})
	if err == nil {
		t.Fatal("want error from AppendMessage(user) failure")
	}
}

func TestInvoke_PropagatesInferError(t *testing.T) {
	p := New(&fakeSession{runID: "r"}, &fakeInference{failInfer: true})
	_, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{ThreadId: "t", Content: "x"})
	if err == nil {
		t.Fatal("want error from Infer failure")
	}
}

func TestInvokeStream_ForwardsChunksAndPersistsAssistant(t *testing.T) {
	sess := &fakeSession{runID: "run-1"}
	inf := &fakeInference{
		streamChunks: []*mpv1.InferChunk{
			{RequestId: "req-2", Delta: "hel"},
			{RequestId: "req-2", Delta: "lo"},
			{RequestId: "req-2", Delta: "", Done: true, ModelUsed: "gpt-4o", InputTokens: 1, OutputTokens: 2},
		},
	}
	p := New(sess, inf)

	sink := &fakeSink{ctx: context.Background()}
	if err := p.InvokeStream(&mpv1.InvokeRequest{
		RequestId: "req-2",
		ThreadId:  "thread-x",
		Content:   "stream me",
	}, sink); err != nil {
		t.Fatalf("InvokeStream: %v", err)
	}

	if len(sink.sent) != 3 {
		t.Fatalf("sink.sent = %d, want 3", len(sink.sent))
	}
	if sink.sent[0].GetDelta() != "hel" || sink.sent[1].GetDelta() != "lo" || !sink.sent[2].GetDone() {
		t.Errorf("forwarded chunks wrong: %+v", sink.sent)
	}
	if sink.sent[2].GetModelUsed() != "gpt-4o" {
		t.Errorf("final chunk model = %q", sink.sent[2].GetModelUsed())
	}

	// AppendMessage(assistant) should receive the concatenated delta "hello".
	if len(sess.appendCalls) != 2 {
		t.Fatalf("appendCalls = %d, want 2", len(sess.appendCalls))
	}
	if sess.appendCalls[1].GetRole() != "assistant" || sess.appendCalls[1].GetContent() != "hello" {
		t.Errorf("assistant append = %+v", sess.appendCalls[1])
	}
}

func TestInvoke_CacheHit_SkipsInference(t *testing.T) {
	sess := &fakeSession{runID: "r", segments: []*mpv1.ContextSegment{{Kind: "thread", Content: "earlier"}}}
	inf := &fakeInference{}
	cache := &fakeCache{hit: true, hitResp: "cached answer"}
	p := New(sess, inf, WithCache(cache))

	resp, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{
		RequestId: "req-1", OrgId: "org-1", ThreadId: "t1", Content: "hello", Model: "m1",
	})
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	if inf.inferReq != nil {
		t.Error("Infer must not be called on a cache hit")
	}
	if resp.GetContent() != "cached answer" || resp.GetModelUsed() != "m1" {
		t.Errorf("InvokeResponse = %+v", resp)
	}
	if resp.GetInputTokens() != 0 || resp.GetOutputTokens() != 0 {
		t.Errorf("cache hit should report zero tokens, got in=%d out=%d", resp.GetInputTokens(), resp.GetOutputTokens())
	}
	if len(sess.appendCalls) != 2 || sess.appendCalls[1].GetRole() != "assistant" ||
		sess.appendCalls[1].GetContent() != "cached answer" {
		t.Errorf("appendCalls = %+v", sess.appendCalls)
	}
	if cache.lookups != 1 {
		t.Errorf("cache lookups = %d, want 1", cache.lookups)
	}
	if len(cache.stored) != 0 {
		t.Errorf("cache should not be written on a hit, stored = %+v", cache.stored)
	}
}

func TestInvoke_CacheMiss_StoresResponse(t *testing.T) {
	sess := &fakeSession{runID: "r"}
	inf := &fakeInference{inferResp: &mpv1.InferResponse{Content: "fresh"}}
	cache := &fakeCache{hit: false}
	p := New(sess, inf, WithCache(cache))

	if _, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{
		ThreadId: "t1", Content: "q", OrgId: "o", Model: "m",
	}); err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	if inf.inferReq == nil {
		t.Fatal("Infer must be called on a cache miss")
	}
	if len(cache.stored) != 1 {
		t.Fatalf("stored = %d entries, want 1", len(cache.stored))
	}
	got := cache.stored[0]
	if got.prompt != "q" || got.org != "o" || got.model != "m" || got.response != "fresh" {
		t.Errorf("stored entry = %+v", got)
	}
}

func TestInvoke_CacheError_FallsThroughToInference(t *testing.T) {
	sess := &fakeSession{runID: "r"}
	inf := &fakeInference{inferResp: &mpv1.InferResponse{Content: "fresh"}}
	cache := &fakeCache{lookupErr: errors.New("cache down")}
	p := New(sess, inf, WithCache(cache))

	resp, err := p.Invoke(context.Background(), &mpv1.InvokeRequest{ThreadId: "t1", Content: "q"})
	if err != nil {
		t.Fatalf("Invoke: %v", err)
	}
	if inf.inferReq == nil {
		t.Error("Infer must be called when the cache lookup errors")
	}
	if resp.GetContent() != "fresh" {
		t.Errorf("content = %q, want fresh", resp.GetContent())
	}
}

func TestInvokeStream_CacheHit_EmitsSingleChunk(t *testing.T) {
	sess := &fakeSession{runID: "r"}
	inf := &fakeInference{}
	cache := &fakeCache{hit: true, hitResp: "cached stream"}
	p := New(sess, inf, WithCache(cache))

	sink := &fakeSink{ctx: context.Background()}
	if err := p.InvokeStream(&mpv1.InvokeRequest{ThreadId: "t", Content: "hi", Model: "m1"}, sink); err != nil {
		t.Fatalf("InvokeStream: %v", err)
	}
	if inf.streamReq != nil {
		t.Error("InferStream must not be called on a cache hit")
	}
	if len(sink.sent) != 1 || sink.sent[0].GetDelta() != "cached stream" || !sink.sent[0].GetDone() {
		t.Errorf("sink.sent = %+v", sink.sent)
	}
	if len(sess.appendCalls) != 2 || sess.appendCalls[1].GetContent() != "cached stream" {
		t.Errorf("appendCalls = %+v", sess.appendCalls)
	}
}
