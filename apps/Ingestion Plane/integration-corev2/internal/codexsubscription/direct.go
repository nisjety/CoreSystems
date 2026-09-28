package codexsubscription

import (
	"bytes"
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math/rand/v2"
	"net"
	"net/http"
	"net/url"
	"regexp"
	"runtime"
	"runtime/debug"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/google/uuid"
	"github.com/rs/zerolog"

	"github.com/triodelab/integration-corev2/internal/egress"
)

const (
	// DefaultDirectBaseURL is the ChatGPT Codex backend. The Responses
	// endpoint is DefaultDirectBaseURL + "/responses"; api.openai.com rejects
	// a ChatGPT token.
	DefaultDirectBaseURL = "https://chatgpt.com/backend-api/codex"
	defaultOriginator    = "verevon"

	RequestShapeClassic = "classic"
	RequestShapeLite    = "lite"
	PromptModeRoles     = "roles"
	PromptModeFlattened = "flattened"

	DefaultDirectMaxRetries = 2
	MaxDirectRetries        = 3

	defaultInvocationTimeout    = 90 * time.Second
	directConnectTimeout        = 10 * time.Second
	directTLSHandshakeTimeout   = 10 * time.Second
	directResponseHeaderTimeout = 45 * time.Second
	retryDeadlineFloor          = 5 * time.Second
	maxHonouredRetryAfter       = 5 * time.Second
	codexOutputSchemaName       = "codex_output_schema"
)

// buildVersion is the broker version in the User-Agent. Set it with
// -ldflags "-X github.com/triodelab/integration-corev2/internal/codexsubscription.buildVersion=<v>";
// it otherwise comes from the module build info, else "dev".
var buildVersion = ""

var (
	promptCacheNamespace = uuid.NewSHA1(uuid.NameSpaceURL, []byte("urn:verevon:integration-core:codex-subscription:prompt-cache"))
	userAgentVersionSafe = regexp.MustCompile(`[^A-Za-z0-9._+-]`)
	retryBackoff         = []time.Duration{250 * time.Millisecond, 750 * time.Millisecond, 2250 * time.Millisecond}
)

// DirectConfig configures the direct Responses transport. The transport
// trusts it: host validation lives in config, so tests can pass httptest URLs.
type DirectConfig struct {
	// BaseURL defaults to DefaultDirectBaseURL.
	BaseURL string
	// TokenURL is the OAuth token endpoint used for refresh. It is a code
	// constant in production (https://auth.openai.com/oauth/token), never an
	// environment variable.
	TokenURL string
	// Originator identifies the client honestly; defaults to "verevon".
	Originator string
	// RequestShape is "classic" (default) or "lite".
	RequestShape string
	// PromptMode is "roles" (default) or "flattened".
	PromptMode string
	// MaxRetries is the exact number of extra attempts, 0 to 3. The runtime
	// config defaults it to 2; the zero value here means no retries.
	MaxRetries int
	// InvocationTimeout sizes the default client's overall timeout
	// (InvocationTimeout + 10s). Defaults to 90s.
	InvocationTimeout time.Duration
	// HTTPClient nil means an egress.SafeClient with SSE-safe timeouts.
	// Redirects are never followed, whatever client is supplied.
	HTTPClient *http.Client
	// Now is the phase clock; nil means time.Now.
	Now    func() time.Time
	Logger zerolog.Logger
}

// DirectTransport sends one plain Responses API request per call to the
// ChatGPT Codex backend with the connection's own ChatGPT OAuth token, the
// way OpenCode, Crush and pi do.
type DirectTransport struct {
	endpoint   string
	originator string
	userAgent  string
	shape      string
	promptMode string
	maxRetries int
	client     *http.Client
	creds      *credentialStore
	now        func() time.Time
	logger     zerolog.Logger
	tiers      *tierMemory

	sleep           func(context.Context, time.Duration) error
	jitter          func(time.Duration) time.Duration
	clientRequestID func() string
}

func NewDirectTransport(cfg DirectConfig, creds *credentialStore) (*DirectTransport, error) {
	if creds == nil {
		return nil, errors.New("codex subscription direct transport needs a credential store")
	}
	base := strings.TrimRight(strings.TrimSpace(cfg.BaseURL), "/")
	if base == "" {
		base = DefaultDirectBaseURL
	}
	parsed, err := url.Parse(base)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return nil, errors.New("codex subscription direct base URL must be absolute")
	}
	shape, err := normalizeChoice(cfg.RequestShape, RequestShapeClassic, RequestShapeClassic, RequestShapeLite)
	if err != nil {
		return nil, fmt.Errorf("codex subscription request shape: %w", err)
	}
	promptMode, err := normalizeChoice(cfg.PromptMode, PromptModeRoles, PromptModeRoles, PromptModeFlattened)
	if err != nil {
		return nil, fmt.Errorf("codex subscription prompt mode: %w", err)
	}
	if cfg.MaxRetries < 0 || cfg.MaxRetries > MaxDirectRetries {
		return nil, fmt.Errorf("codex subscription max retries must be 0 to %d", MaxDirectRetries)
	}
	originator := strings.TrimSpace(cfg.Originator)
	if originator == "" {
		originator = defaultOriginator
	}
	client := cfg.HTTPClient
	if client == nil {
		client = defaultDirectHTTPClient(cfg.InvocationTimeout)
	}
	now := cfg.Now
	if now == nil {
		now = time.Now
	}
	return &DirectTransport{
		endpoint:        base + "/responses",
		originator:      originator,
		userAgent:       directUserAgent(),
		shape:           shape,
		promptMode:      promptMode,
		maxRetries:      cfg.MaxRetries,
		client:          withoutRedirects(client),
		creds:           creds,
		now:             now,
		logger:          cfg.Logger,
		tiers:           newTierMemory(),
		sleep:           sleepContext,
		jitter:          jitterDuration,
		clientRequestID: uuid.NewString,
	}, nil
}

func normalizeChoice(raw, fallback string, allowed ...string) (string, error) {
	value := strings.ToLower(strings.TrimSpace(raw))
	if value == "" {
		return fallback, nil
	}
	for _, candidate := range allowed {
		if value == candidate {
			return value, nil
		}
	}
	return "", fmt.Errorf("unsupported value %q", raw)
}

// defaultDirectHTTPClient is the vetted egress client, with an overall
// timeout long enough for a streamed answer, a response-header timeout, and
// transparent decompression off so SSE is never gzip-buffered.
func defaultDirectHTTPClient(invocationTimeout time.Duration) *http.Client {
	if invocationTimeout <= 0 {
		invocationTimeout = defaultInvocationTimeout
	}
	client := egress.SafeClient(egress.ClientConfig{
		ConnectTimeout:      directConnectTimeout,
		TLSHandshakeTimeout: directTLSHandshakeTimeout,
		RequestTimeout:      invocationTimeout + 10*time.Second,
	})
	if transport, ok := client.Transport.(*http.Transport); ok {
		transport.ResponseHeaderTimeout = directResponseHeaderTimeout
		transport.DisableCompression = true
	}
	return client
}

func directUserAgent() string {
	return "verevon/" + brokerVersion() + " (" + runtime.GOOS + "; " + runtime.GOARCH + ") integration-core"
}

func brokerVersion() string {
	version := strings.TrimSpace(buildVersion)
	if version == "" {
		if info, ok := debug.ReadBuildInfo(); ok {
			version = strings.TrimSpace(info.Main.Version)
		}
	}
	if version == "" || version == "(devel)" {
		return "dev"
	}
	version = userAgentVersionSafe.ReplaceAllString(version, "")
	if version == "" {
		return "dev"
	}
	return version
}

func (t *DirectTransport) Kind() TransportKind { return TransportDirect }

func (t *DirectTransport) Invoke(ctx context.Context, codeHome string, request InvokeRequest) (InvokeResponse, error) {
	// The backend rejects stream:false, so a unary call is the same stream,
	// collected into one answer.
	return t.invoke(ctx, codeHome, request, nil)
}

func (t *DirectTransport) InvokeStream(ctx context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (InvokeResponse, error) {
	return t.invoke(ctx, codeHome, request, onDelta)
}

func (t *DirectTransport) invoke(ctx context.Context, codeHome string, request InvokeRequest, onDelta func(string) error) (response InvokeResponse, err error) {
	call := &directCallLog{
		requestID:       sanitizeUpstreamRequestID(request.RequestID),
		clientRequestID: t.clientRequestID(),
		connectionID:    sanitizeUpstreamRequestID(request.ConnectionID),
		model:           sanitizeServedModel(request.Model),
		refreshed:       "none",
		queueMs:         queueWaitFrom(ctx).Milliseconds(),
	}
	defer func() { t.logCall(call, err) }()

	refreshStarted := time.Now()
	tokens, refreshed, err := t.creds.current(ctx, codeHome, time.Time{})
	call.refreshMs = time.Since(refreshStarted).Milliseconds()
	if refreshed {
		call.refreshed = "proactive"
	}
	if err != nil {
		return InvokeResponse{}, err
	}

	profile, known := lookupModelProfile(request.Model)
	lite := t.shape == RequestShapeLite && known && profile.lite
	sendTier := request.ServiceTier == "priority" && known && profile.supportsTier("priority") && !t.tiers.rejected(request.ConnectionID, request.Model)
	call.tierDropped = request.ServiceTier != "" && !sendTier
	cacheKey := promptCacheKey(request.ConnectionID, request.Model)
	state := &streamState{onDelta: onDelta}

	started := t.now()
	var headersAt time.Time
	recovered, degraded, retries := false, false, 0
	for {
		call.attempts++
		body, err := t.requestBody(request, profile, known, lite, sendTier, cacheKey)
		if err != nil {
			return InvokeResponse{}, err
		}
		result, err := t.attempt(ctx, request, tokens, body, cacheKey, lite, state, &headersAt, call)
		if err == nil {
			phases := map[string]int64{
				PhaseTurnStart: elapsedMs(started, headersAt),
				PhaseTurnTotal: elapsedMs(headersAt, result.completedAt),
			}
			if !result.firstText.IsZero() {
				phases[PhaseFirstDelta] = elapsedMs(headersAt, result.firstText)
			}
			call.servedModel = sanitizeServedModel(result.servedModel(nil))
			call.servedModelOK = true
			call.servedTier = result.servedTier
			call.usage = result.usage
			call.phases = phases
			return InvokeResponse{
				RequestID: request.RequestID,
				Content:   result.content,
				ModelUsed: request.Model,
				Usage:     result.usage,
				PhasesMs:  phases,
			}, nil
		}
		var unauthorized *unauthorizedError
		if errors.As(err, &unauthorized) {
			// The call gets one credential recovery, as on the app-server
			// transport: only a recovery the token endpoint refuses (or a
			// sign-in for another account) is ErrReauthenticationRequired,
			// which marks the connection needs_refresh. A 401 that a
			// successful recovery did not cure, or one that arrives after
			// answer text, is a transient failure, so a passing 401 never
			// disconnects the user.
			if recovered {
				return InvokeResponse{}, unauthorizedAfterRecovery(unauthorized)
			}
			recovered = true
			call.refreshed = "401"
			tokens, err = t.creds.recoverUnauthorized(ctx, codeHome, tokens)
			if err != nil {
				return InvokeResponse{}, err
			}
			if state.started() {
				// Answer text was already seen or forwarded, so the call
				// cannot be retried; the recovered credential serves the
				// next one.
				return InvokeResponse{}, unauthorizedAfterRecovery(unauthorized)
			}
			continue
		}
		var upstream *UpstreamError
		if errors.As(err, &upstream) {
			if upstream.tierRejected && sendTier && !degraded && !state.started() {
				degraded, sendTier, call.tierDropped = true, false, true
				t.tiers.reject(request.ConnectionID, request.Model)
				continue
			}
			if delay, ok := t.retryDelay(ctx, upstream, state, retries); ok {
				retries++
				if sleepErr := t.sleep(ctx, delay); sleepErr != nil {
					return InvokeResponse{}, fmt.Errorf("wait to retry ChatGPT subscription request: %w", sleepErr)
				}
				continue
			}
		}
		return InvokeResponse{}, err
	}
}

// attempt sends one POST and reads its stream. The attempt's own context is
// cancelled on return, which closes the upstream stream when the caller
// stopped reading.
func (t *DirectTransport) attempt(ctx context.Context, request InvokeRequest, tokens tokenSet, body []byte, cacheKey string, lite bool, state *streamState, headersAt *time.Time, call *directCallLog) (streamResult, error) {
	attemptCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	httpRequest, err := http.NewRequestWithContext(attemptCtx, http.MethodPost, t.endpoint, bytes.NewReader(body))
	if err != nil {
		return streamResult{}, errors.New("build ChatGPT subscription request")
	}
	t.setHeaders(httpRequest.Header, tokens, cacheKey, call.clientRequestID, lite)
	response, err := t.client.Do(httpRequest)
	if err != nil {
		if ctxErr := ctx.Err(); ctxErr != nil {
			return streamResult{}, fmt.Errorf("send ChatGPT subscription request: %w", ctxErr)
		}
		return streamResult{}, &UpstreamError{Kind: upstreamKindTransport, Code: transportErrorCode(err), Retryable: true}
	}
	defer response.Body.Close()
	*headersAt = t.now()
	call.observe(response)
	if response.StatusCode == http.StatusUnauthorized {
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, maxUpstreamErrorBody))
		return streamResult{}, &unauthorizedError{requestID: upstreamRequestID(response.Header)}
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		errorBody, _ := io.ReadAll(io.LimitReader(response.Body, maxUpstreamErrorBody))
		return streamResult{}, classifyHTTPFailure(response.StatusCode, response.Header, errorBody, time.Now())
	}
	for _, served := range []string{response.Header.Get("openai-model"), response.Header.Get("x-openai-model")} {
		if !servedModelMatches(request.Model, served) {
			mismatch := mismatchError(served, response.Header)
			mismatch.HTTPStatus = response.StatusCode
			return streamResult{}, mismatch
		}
	}
	return consumeStream(attemptCtx, response.Body, request, response.Header, state, t.now)
}

// setHeaders writes the exact header set of §4.2. Nothing else identifying
// is sent: no OpenAI-Beta, version, conversation or telemetry headers, no
// cookies, and no Content-Encoding.
func (t *DirectTransport) setHeaders(headers http.Header, tokens tokenSet, cacheKey, clientRequestID string, lite bool) {
	headers.Set("Authorization", "Bearer "+tokens.Access.reveal())
	if tokens.AccountID != "" {
		headers.Set("ChatGPT-Account-ID", tokens.AccountID)
	}
	headers.Set("originator", t.originator)
	headers.Set("User-Agent", t.userAgent)
	headers.Set("Accept", "text/event-stream")
	headers.Set("Accept-Encoding", "identity")
	headers.Set("Content-Type", "application/json")
	headers.Set("session-id", cacheKey)
	headers.Set("x-client-request-id", clientRequestID)
	if tokens.Residency != "" {
		headers.Set("x-openai-internal-codex-residency", tokens.Residency)
	}
	if tokens.FedRAMP {
		headers.Set("X-OpenAI-Fedramp", "true")
	}
	if lite {
		headers.Set("x-openai-internal-codex-responses-lite", "true")
	}
}

// retryDelay decides whether a failed attempt is retried and after how long
// (§5.5). Nothing is retried once answer text arrived or was forwarded.
func (t *DirectTransport) retryDelay(ctx context.Context, upstream *UpstreamError, state *streamState, retries int) (time.Duration, bool) {
	if !upstream.Retryable || state.started() || retries >= t.maxRetries {
		return 0, false
	}
	if deadline, ok := ctx.Deadline(); ok && time.Until(deadline) < retryDeadlineFloor {
		return 0, false
	}
	if upstream.hasRetryAfter {
		if upstream.retryAfter < 0 || upstream.retryAfter > maxHonouredRetryAfter {
			return 0, false
		}
		return upstream.retryAfter, true
	}
	return t.jitter(retryBackoff[min(retries, len(retryBackoff)-1)]), true
}

func sleepContext(ctx context.Context, delay time.Duration) error {
	if delay <= 0 {
		return ctx.Err()
	}
	timer := time.NewTimer(delay)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-timer.C:
		return nil
	}
}

// jitterDuration spreads a backoff by ±20%.
func jitterDuration(base time.Duration) time.Duration {
	return time.Duration(float64(base) * (0.8 + 0.4*rand.Float64()))
}

// promptCacheKey is scoped to one connection and model: stable across calls,
// never shared between users, and opaque, so the connection id never leaves
// the broker.
func promptCacheKey(connectionID, model string) string {
	return uuid.NewSHA1(promptCacheNamespace, []byte(connectionID+"\x00"+model)).String()
}

type directContentPart struct {
	Type string `json:"type"`
	Text string `json:"text"`
}

type directInputItem struct {
	Type    string              `json:"type"`
	Role    string              `json:"role"`
	Content []directContentPart `json:"content"`
}

type directReasoning struct {
	Effort  string `json:"effort"`
	Context string `json:"context,omitempty"`
}

type directTextFormat struct {
	Type   string          `json:"type"`
	Name   string          `json:"name"`
	Strict bool            `json:"strict"`
	Schema json.RawMessage `json:"schema"`
}

type directText struct {
	Verbosity string            `json:"verbosity,omitempty"`
	Format    *directTextFormat `json:"format,omitempty"`
}

// directRequestBody is the Responses request. store:false and stream:true
// are hard backend requirements; max_output_tokens, sampling parameters,
// previous_response_id, tools and reasoning.summary are never sent.
type directRequestBody struct {
	Model             string            `json:"model"`
	Instructions      string            `json:"instructions,omitempty"`
	Input             []directInputItem `json:"input"`
	Reasoning         directReasoning   `json:"reasoning"`
	Store             bool              `json:"store"`
	Stream            bool              `json:"stream"`
	Include           []string          `json:"include"`
	PromptCacheKey    string            `json:"prompt_cache_key"`
	ServiceTier       string            `json:"service_tier,omitempty"`
	ParallelToolCalls *bool             `json:"parallel_tool_calls,omitempty"`
	Text              *directText       `json:"text,omitempty"`
}

func (t *DirectTransport) requestBody(request InvokeRequest, profile modelProfile, known, lite, sendTier bool, cacheKey string) ([]byte, error) {
	instructions, input := mapPrompt(request.Messages, t.promptMode)
	body := directRequestBody{
		Model:          request.Model,
		Instructions:   instructions,
		Input:          input,
		Reasoning:      directReasoning{Effort: request.ReasoningEffort},
		Store:          false,
		Stream:         true,
		Include:        []string{"reasoning.encrypted_content"},
		PromptCacheKey: cacheKey,
	}
	if lite {
		body.Instructions = ""
		body.Input = append([]directInputItem{textItem("developer", "input_text", instructions)}, input...)
		body.Reasoning.Context = "all_turns"
		parallel := false
		body.ParallelToolCalls = &parallel
	}
	if sendTier {
		body.ServiceTier = "priority"
	}
	var text directText
	if known && profile.verbosity != "" {
		text.Verbosity = profile.verbosity
	}
	if len(request.OutputSchema) > 0 {
		var schema bytes.Buffer
		if err := json.Compact(&schema, request.OutputSchema); err != nil {
			return nil, fmt.Errorf("%w: output schema is not valid JSON", ErrInvalidRequest)
		}
		text.Format = &directTextFormat{Type: "json_schema", Name: codexOutputSchemaName, Strict: true, Schema: json.RawMessage(schema.Bytes())}
	}
	if text.Verbosity != "" || text.Format != nil {
		body.Text = &text
	}
	var encoded bytes.Buffer
	encoder := json.NewEncoder(&encoded)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(body); err != nil {
		return nil, errors.New("encode ChatGPT subscription request")
	}
	return bytes.TrimRight(encoded.Bytes(), "\n"), nil
}

func textItem(role, partType, text string) directInputItem {
	return directInputItem{Type: "message", Role: role, Content: []directContentPart{{Type: partType, Text: text}}}
}

// mapPrompt maps the conversation onto instructions and input items (§4.4).
// In roles mode the guard plus leading system messages become instructions,
// later system messages developer items, and user and assistant messages
// role items; nothing is dropped. flattened reproduces the app-server
// transport's single turn text exactly.
func mapPrompt(messages []ChatMessage, mode string) (string, []directInputItem) {
	if mode == PromptModeFlattened {
		return flattenedPrompt(messages)
	}
	var leading []string
	index := 0
	for ; index < len(messages); index++ {
		role := normalizedRole(messages[index].Role)
		if role != "system" && role != "developer" {
			break
		}
		if strings.TrimSpace(messages[index].Content) != "" {
			leading = append(leading, messageText(messages[index]))
		}
	}
	var input []directInputItem
	for _, message := range messages[index:] {
		if strings.TrimSpace(message.Content) == "" {
			continue
		}
		switch role := normalizedRole(message.Role); role {
		case "system", "developer":
			input = append(input, textItem("developer", "input_text", messageText(message)))
		case "user":
			input = append(input, textItem("user", "input_text", messageText(message)))
		case "assistant":
			input = append(input, textItem("assistant", "output_text", messageText(message)))
		default:
			input = append(input, textItem("user", "input_text", roleHeaderText(role, message)))
		}
	}
	if len(input) == 0 {
		return flattenedPrompt(messages)
	}
	instructions := codexBaseInstructions
	if len(leading) > 0 {
		instructions += "\n\n" + strings.Join(leading, "\n\n")
	}
	return instructions, input
}

func flattenedPrompt(messages []ChatMessage) (string, []directInputItem) {
	return codexBaseInstructions, []directInputItem{textItem("user", "input_text", renderPrompt(messages))}
}

func normalizedRole(role string) string {
	role = strings.ToLower(strings.TrimSpace(role))
	if role == "" {
		return "user"
	}
	return role
}

func messageText(message ChatMessage) string {
	if name := strings.TrimSpace(message.Name); name != "" {
		return "(" + name + ")\n" + message.Content
	}
	return message.Content
}

// roleHeaderText keeps a message of any other role as user text under the
// same header renderPrompt writes, so no content is dropped.
func roleHeaderText(role string, message ChatMessage) string {
	var text strings.Builder
	text.WriteString(strings.ToUpper(role))
	if name := strings.TrimSpace(message.Name); name != "" {
		text.WriteString(" (")
		text.WriteString(name)
		text.WriteString(")")
	}
	text.WriteString(":\n")
	text.WriteString(message.Content)
	return text.String()
}

// transportErrorCode names a transport failure without keeping its text.
func transportErrorCode(err error) string {
	var netErr net.Error
	var dnsErr *net.DNSError
	var recordErr tls.RecordHeaderError
	var certErr *tls.CertificateVerificationError
	var unknownAuthority x509.UnknownAuthorityError
	switch {
	case errors.Is(err, egress.ErrBlocked):
		return "blocked"
	case errors.Is(err, io.EOF), errors.Is(err, io.ErrUnexpectedEOF):
		return "eof"
	case errors.As(err, &dnsErr):
		return "dns"
	case errors.As(err, &recordErr), errors.As(err, &certErr), errors.As(err, &unknownAuthority):
		return "tls"
	case errors.Is(err, syscall.ECONNRESET):
		return "connection_reset"
	case errors.As(err, &netErr) && netErr.Timeout():
		return "timeout"
	}
	return "other"
}

type queueWaitKey struct{}

// withQueueWait records how long the call queued for a concurrency slot, for
// the transport's log line. It is not a phase.
func withQueueWait(ctx context.Context, wait time.Duration) context.Context {
	return context.WithValue(ctx, queueWaitKey{}, wait)
}

func queueWaitFrom(ctx context.Context) time.Duration {
	wait, _ := ctx.Value(queueWaitKey{}).(time.Duration)
	return wait
}

// directCallLog collects the content-free fields of the one log line per
// direct call (§8). It never holds a header value other than those named, a
// body, a prompt, an answer, an upstream message, a claim, or a token.
type directCallLog struct {
	requestID, clientRequestID, connectionID, model string
	servedModel, servedTier, upstreamRequestID      string
	cfRay, refreshed                                string
	servedModelOK, tierDropped                      bool
	httpStatus, attempts                            int
	queueMs, refreshMs                              int64
	phases                                          map[string]int64
	usage                                           *TokenUsage
	rateLimitHeaders                                map[string]float64
}

var rateLimitLogHeaders = []struct{ header, field string }{
	{"x-codex-primary-used-percent", "primary_used_percent"},
	{"x-codex-secondary-used-percent", "secondary_used_percent"},
	{"x-codex-primary-reset-at", "primary_reset_at"},
}

func (c *directCallLog) observe(response *http.Response) {
	c.httpStatus = response.StatusCode
	c.upstreamRequestID = upstreamRequestID(response.Header)
	c.cfRay = sanitizeUpstreamRequestID(response.Header.Get("cf-ray"))
	for _, item := range rateLimitLogHeaders {
		if value, err := strconv.ParseFloat(strings.TrimSpace(response.Header.Get(item.header)), 64); err == nil {
			if c.rateLimitHeaders == nil {
				c.rateLimitHeaders = map[string]float64{}
			}
			c.rateLimitHeaders[item.field] = value
		}
	}
}

func (t *DirectTransport) logCall(call *directCallLog, err error) {
	event := t.logger.Info()
	if err != nil {
		event = t.logger.Warn()
	}
	event = event.
		Str("request_id", call.requestID).
		Str("client_request_id", call.clientRequestID).
		Str("connection_id", call.connectionID).
		Str("transport", string(TransportDirect)).
		Str("model", call.model).
		Bool("served_model_ok", call.servedModelOK).
		Int("attempts", call.attempts).
		Str("refreshed", call.refreshed).
		Int64("queue_ms", call.queueMs).
		Int64("refresh_ms", call.refreshMs)
	if call.servedModel != "" {
		event = event.Str("served_model", call.servedModel)
	}
	if call.servedTier != "" {
		event = event.Str("served_tier", call.servedTier)
	}
	if call.tierDropped {
		event = event.Bool("service_tier_dropped", true)
	}
	if call.httpStatus != 0 {
		event = event.Int("http_status", call.httpStatus)
	}
	if call.upstreamRequestID != "" {
		event = event.Str("upstream_request_id", call.upstreamRequestID)
	}
	if call.cfRay != "" {
		event = event.Str("cf_ray", call.cfRay)
	}
	for _, item := range rateLimitLogHeaders {
		if value, ok := call.rateLimitHeaders[item.field]; ok {
			event = event.Float64(item.field, value)
		}
	}
	for _, phase := range []string{PhaseTurnStart, PhaseFirstDelta, PhaseTurnTotal} {
		if ms, ok := call.phases[phase]; ok {
			event = event.Int64(phase+"_ms", ms)
		}
	}
	if call.usage != nil {
		event = event.
			Int64("input_tokens", call.usage.InputTokens).
			Int64("cached_input_tokens", call.usage.CachedInputTokens).
			Int64("cache_write_input_tokens", call.usage.CacheWriteInputTokens).
			Int64("output_tokens", call.usage.OutputTokens).
			Int64("reasoning_output_tokens", call.usage.ReasoningOutputTokens)
	}
	if err != nil {
		kind, code := errorLogFields(err)
		event = event.Str("error_kind", kind)
		if code != "" {
			event = event.Str("error_code", code)
		}
		var upstream *UpstreamError
		if call.servedModel == "" && errors.As(err, &upstream) && upstream.servedModel != "" {
			event = event.Str("served_model", upstream.servedModel)
		}
		var limited *RateLimitError
		if errors.As(err, &limited) {
			event = event.Str("rate_limit_kind", string(limited.Kind))
			if limited.RetryAfter > 0 {
				event = event.Int64("retry_after_s", CeilSeconds(limited.RetryAfter))
			}
		}
		failure := ClassifyFailure(err, time.Now())
		event = event.Str("reason", string(failure.Code))
		if failure.Code == FailureUsageLimit {
			event = event.Str("resets_at", failure.FormatResetsAt()).Int("window_mins", failure.WindowMinutes).Str("limit_reached_type", failure.LimitReachedType)
		}
	}
	event.Msg("codex_subscription_upstream")
}

// errorLogFields maps an error to a content-free kind and sanitized code. A
// rate limit logs its kind (usage_limit, throttle, broker_queue) and the
// sanitized upstream code or app-server variant behind it.
func errorLogFields(err error) (string, string) {
	var upstream *UpstreamError
	var limited *RateLimitError
	var sink *deltaSinkError
	switch {
	case errors.As(err, &upstream):
		return upstream.Kind, upstream.Code
	case errors.As(err, &limited):
		return string(limited.Kind), limited.upstreamCode
	case errors.Is(err, ErrReauthenticationRequired):
		return "reauthentication_required", ""
	case errors.Is(err, context.DeadlineExceeded):
		return "deadline_exceeded", ""
	case errors.Is(err, context.Canceled):
		return "canceled", ""
	case errors.As(err, &sink):
		return "stream_client_gone", ""
	case errors.Is(err, ErrInvalidRequest):
		return "invalid_request", ""
	}
	return "internal", ""
}
