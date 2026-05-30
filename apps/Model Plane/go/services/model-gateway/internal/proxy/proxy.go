// Package proxy implements the model-gateway invoke fan-out:
//
//  1. AppendMessage(user)         -> session-core
//  2. StartRun                    -> session-core
//  3. GetContextAssembly          -> session-core
//  4. Infer / InferStream         -> inference-core
//  5. AppendMessage(assistant)    -> session-core
//
// The proxy depends on small interfaces (SessionClient, InferenceClient) so
// tests can swap in fakes without standing up real gRPC servers. The default
// production wiring binds these to gRPC clients in cmd/main.go.
package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"strings"

	"google.golang.org/grpc"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

// SessionClient is the subset of mpv1.SessionCoreClient used by the proxy.
type SessionClient interface {
	AppendMessage(ctx context.Context, in *mpv1.AppendMessageRequest, opts ...grpc.CallOption) (*mpv1.AppendMessageResponse, error)
	StartRun(ctx context.Context, in *mpv1.StartRunRequest, opts ...grpc.CallOption) (*mpv1.StartRunResponse, error)
	GetContextAssembly(ctx context.Context, in *mpv1.GetContextAssemblyRequest, opts ...grpc.CallOption) (*mpv1.GetContextAssemblyResponse, error)
}

// InferStreamRecv is the minimal stream-receive surface needed from inference-core.
type InferStreamRecv interface {
	Recv() (*mpv1.InferChunk, error)
}

// InferenceClient is the subset of mpv1.InferenceCoreClient used by the proxy.
type InferenceClient interface {
	Infer(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (*mpv1.InferResponse, error)
	InferStream(ctx context.Context, in *mpv1.InferRequest, opts ...grpc.CallOption) (grpc.ServerStreamingClient[mpv1.InferChunk], error)
}

// ChunkSink receives streaming chunks back to the gateway client.
// In production this is mpv1.ModelGateway_InvokeStreamServer; in tests, a fake.
type ChunkSink interface {
	Send(*mpv1.InvokeChunk) error
	Context() context.Context
}

// SemanticCache is the subset of a semantic-cache backend (e.g. Redis
// LangCache) used by the proxy. Lookup returns hit=false on a miss; a
// non-nil error means the lookup itself failed and the caller should treat
// it as a miss and proceed to inference. orgID scopes entries so one org
// never reads another org's cached responses.
type SemanticCache interface {
	Lookup(ctx context.Context, prompt, orgID, model string) (response string, hit bool, err error)
	Store(ctx context.Context, prompt, orgID, model, response string) error
}

// Proxy holds the downstream clients used to fulfill ModelGateway calls.
type Proxy struct {
	Session   SessionClient
	Inference InferenceClient
	// cache is optional. When nil the proxy behaves exactly as before
	// (every Invoke hits inference-core).
	cache SemanticCache
}

// Option configures a Proxy at construction time.
type Option func(*Proxy)

// WithCache attaches a semantic cache. Passing a nil cache is a no-op.
func WithCache(c SemanticCache) Option {
	return func(p *Proxy) {
		if c != nil {
			p.cache = c
		}
	}
}

// New constructs a Proxy.
func New(session SessionClient, inference InferenceClient, opts ...Option) *Proxy {
	p := &Proxy{Session: session, Inference: inference}
	for _, opt := range opts {
		opt(p)
	}
	return p
}

// cacheLookup queries the semantic cache for a response to the user prompt.
// Returns hit=false when no cache is configured, on a miss, or on any cache
// error — errors degrade to a miss so inference still runs.
func (p *Proxy) cacheLookup(ctx context.Context, req *mpv1.InvokeRequest) (string, bool) {
	if p.cache == nil {
		return "", false
	}
	resp, hit, err := p.cache.Lookup(ctx, req.GetContent(), req.GetOrgId(), req.GetModel())
	if err != nil || !hit {
		return "", false
	}
	return resp, true
}

// cacheStore writes a freshly generated response back to the semantic cache.
// Best-effort: a nil cache, empty response, or write error is ignored.
func (p *Proxy) cacheStore(ctx context.Context, req *mpv1.InvokeRequest, response string) {
	if p.cache == nil || response == "" {
		return
	}
	_ = p.cache.Store(ctx, req.GetContent(), req.GetOrgId(), req.GetModel(), response)
}

// Invoke executes the unary fan-out.
func (p *Proxy) Invoke(ctx context.Context, req *mpv1.InvokeRequest) (*mpv1.InvokeResponse, error) {
	threadID, runID, messages, err := p.prepare(ctx, req)
	if err != nil {
		return nil, err
	}

	// Semantic-cache lookup. On a hit we skip inference entirely (the
	// expensive, token-billed call) but still record the assistant turn so
	// thread history stays consistent. Zero token counts mark a cache hit
	// for downstream usage accounting.
	if cached, hit := p.cacheLookup(ctx, req); hit {
		if _, err := p.Session.AppendMessage(ctx, &mpv1.AppendMessageRequest{
			ThreadId: threadID,
			Role:     "assistant",
			Content:  cached,
		}); err != nil {
			return nil, fmt.Errorf("session-core AppendMessage(assistant): %w", err)
		}
		return &mpv1.InvokeResponse{
			RequestId:  req.GetRequestId(),
			Content:    cached,
			ModelUsed:  req.GetModel(),
			StopReason: "end_turn",
		}, nil
	}

	resp, err := p.Inference.Infer(ctx, &mpv1.InferRequest{
		RequestId:    req.GetRequestId(),
		OrgId:        req.GetOrgId(),
		Model:        req.GetModel(),
		ProviderHint: req.GetProvider(),
		Messages:     messages,
		Temperature:  req.GetTemperature(),
		MaxTokens:    req.GetMaxTokens(),
	})
	if err != nil {
		return nil, fmt.Errorf("inference-core Infer: %w", err)
	}

	if _, err := p.Session.AppendMessage(ctx, &mpv1.AppendMessageRequest{
		ThreadId: threadID,
		Role:     "assistant",
		Content:  resp.GetContent(),
	}); err != nil {
		return nil, fmt.Errorf("session-core AppendMessage(assistant): %w", err)
	}

	p.cacheStore(ctx, req, resp.GetContent())

	_ = runID // run lifecycle (CompleteStep / completion) lands in a later slice.

	return &mpv1.InvokeResponse{
		RequestId:    req.GetRequestId(),
		Content:      resp.GetContent(),
		ModelUsed:    resp.GetModelUsed(),
		StopReason:   resp.GetStopReason(),
		InputTokens:  resp.GetInputTokens(),
		OutputTokens: resp.GetOutputTokens(),
	}, nil
}

// InvokeStream executes the streaming fan-out, forwarding InferChunks to the sink.
func (p *Proxy) InvokeStream(req *mpv1.InvokeRequest, sink ChunkSink) error {
	ctx := sink.Context()
	threadID, runID, messages, err := p.prepare(ctx, req)
	if err != nil {
		return err
	}
	_ = runID

	// Cache hit: emit the cached response as a single terminal chunk and
	// persist the assistant turn, skipping inference-core.
	if cached, hit := p.cacheLookup(ctx, req); hit {
		if err := sink.Send(&mpv1.InvokeChunk{
			RequestId: req.GetRequestId(),
			Delta:     cached,
			Done:      true,
			ModelUsed: req.GetModel(),
		}); err != nil {
			return fmt.Errorf("gateway sink send (cached): %w", err)
		}
		if _, err := p.Session.AppendMessage(ctx, &mpv1.AppendMessageRequest{
			ThreadId: threadID,
			Role:     "assistant",
			Content:  cached,
		}); err != nil {
			return fmt.Errorf("session-core AppendMessage(assistant): %w", err)
		}
		return nil
	}

	stream, err := p.Inference.InferStream(ctx, &mpv1.InferRequest{
		RequestId:    req.GetRequestId(),
		OrgId:        req.GetOrgId(),
		Model:        req.GetModel(),
		ProviderHint: req.GetProvider(),
		Messages:     messages,
		Temperature:  req.GetTemperature(),
		MaxTokens:    req.GetMaxTokens(),
	})
	if err != nil {
		return fmt.Errorf("inference-core InferStream: %w", err)
	}

	var assembled strings.Builder
	for {
		chunk, recvErr := stream.Recv()
		if errors.Is(recvErr, io.EOF) {
			break
		}
		if recvErr != nil {
			return fmt.Errorf("inference-core stream recv: %w", recvErr)
		}

		assembled.WriteString(chunk.GetDelta())

		if err := sink.Send(&mpv1.InvokeChunk{
			RequestId:    req.GetRequestId(),
			Delta:        chunk.GetDelta(),
			Done:         chunk.GetDone(),
			ModelUsed:    chunk.GetModelUsed(),
			InputTokens:  chunk.GetInputTokens(),
			OutputTokens: chunk.GetOutputTokens(),
		}); err != nil {
			return fmt.Errorf("gateway sink send: %w", err)
		}

		if chunk.GetDone() {
			break
		}
	}

	if _, err := p.Session.AppendMessage(ctx, &mpv1.AppendMessageRequest{
		ThreadId: threadID,
		Role:     "assistant",
		Content:  assembled.String(),
	}); err != nil {
		return fmt.Errorf("session-core AppendMessage(assistant): %w", err)
	}

	p.cacheStore(ctx, req, assembled.String())
	return nil
}

// prepare runs the shared session-core preamble: AppendMessage(user), StartRun,
// GetContextAssembly. It returns the thread id, run id, and the assembled
// message list ready for inference.
func (p *Proxy) prepare(ctx context.Context, req *mpv1.InvokeRequest) (threadID, runID string, messages []*mpv1.ChatMessage, err error) {
	threadID = req.GetThreadId()
	if threadID == "" {
		// Thread autocreation is not yet implemented at the gateway.
		// Callers must supply a thread_id until CreateThread is wired in.
		return "", "", nil, errors.New("thread_id is required (thread autocreation not yet implemented)")
	}

	if _, err := p.Session.AppendMessage(ctx, &mpv1.AppendMessageRequest{
		ThreadId: threadID,
		Role:     "user",
		Content:  req.GetContent(),
	}); err != nil {
		return "", "", nil, fmt.Errorf("session-core AppendMessage(user): %w", err)
	}

	startResp, err := p.Session.StartRun(ctx, &mpv1.StartRunRequest{
		ThreadId: threadID,
		Goal:     req.GetContent(),
		Mode:     "execute",
		OrgId:    req.GetOrgId(),
	})
	if err != nil {
		return "", "", nil, fmt.Errorf("session-core StartRun: %w", err)
	}
	runID = startResp.GetRunId()

	ctxResp, err := p.Session.GetContextAssembly(ctx, &mpv1.GetContextAssemblyRequest{
		ThreadId:  threadID,
		RunId:     runID,
		MaxTokens: uint32(req.GetMaxTokens()),
	})
	if err != nil {
		return "", "", nil, fmt.Errorf("session-core GetContextAssembly: %w", err)
	}

	messages = segmentsToMessages(ctxResp.GetSegments(), req.GetContent())
	return threadID, runID, messages, nil
}

// segmentsToMessages flattens ContextSegments into a ChatMessage slice.
//
// Mapping (first slice — kept intentionally simple):
//   - "user"    -> role "user"
//   - "prompt"  -> role "user"
//   - everything else (policy, workspace, agent, thread, episodic,
//     skill_index, skill_expansion, retrieval) -> role "system"
//
// If no segment has kind="prompt" or "user", the original prompt content is
// appended as a final user message so inference always sees the user turn.
func segmentsToMessages(segments []*mpv1.ContextSegment, fallbackPrompt string) []*mpv1.ChatMessage {
	out := make([]*mpv1.ChatMessage, 0, len(segments)+1)
	hasUser := false
	for _, seg := range segments {
		role := "system"
		switch seg.GetKind() {
		case "user", "prompt":
			role = "user"
			hasUser = true
		}
		out = append(out, &mpv1.ChatMessage{Role: role, Content: seg.GetContent()})
	}
	if !hasUser && fallbackPrompt != "" {
		out = append(out, &mpv1.ChatMessage{Role: "user", Content: fallbackPrompt})
	}
	return out
}
