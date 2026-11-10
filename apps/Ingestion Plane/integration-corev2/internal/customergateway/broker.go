// Package customergateway brokers a customer's own OpenAI-compatible inference
// endpoint (a "customer gateway") for Model Plane.
//
// An organisation admin registers one gateway: its https base URL, an API key and
// the model ids it serves. The key is sealed with the service vault (AAD = the
// connection id) and lives only in the connection's encrypted credential column;
// it is never returned, logged, leased or placed in an error. Model Plane never
// holds the key: it names a connection and a model, and this broker makes the
// call to {base_url}/v1/chat/completions through the egress-guarded client with
// no redirects and a 120 second timeout.
//
// The connection is never zero-data-retention: a form cannot assert ZDR or
// residency for an endpoint this service does not operate, so the connection's
// ZDR assessment is always not_supported (see the API layer). The broker keeps no
// message content: it forwards a request, relays the answer, and returns only
// fixed, classified messages when the gateway fails.
package customergateway

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/store"
)

const (
	// ProviderKey is the provider of a customer-gateway connection. It is deliberately not in
	// the provider catalog: the catalog drives the generic connect flows and the ZDR reconcile,
	// and this connection is created only by its own route.
	ProviderKey = "openai_compatible_gateway"
	// ConnectorType is the connector type of a customer-gateway connection.
	ConnectorType = "openai_compatible_gateway"
	// Capability is the one capability the connection carries.
	Capability = "model.inference"
	// AuthMode is the auth mode recorded in the connection's ZDR assessment.
	AuthMode = "api_key"
	// ZDRBasisRef names the fixed assessment every customer gateway gets.
	ZDRBasisRef = "openai-compatible-gateway@1"
	// MaxModelIDs is the most model ids one connection may list.
	MaxModelIDs = 20
	// RequestTimeout bounds one gateway call, including a streamed body.
	RequestTimeout = 120 * time.Second

	// ContextBaseURL and ContextModelIDs are the provider-context keys that hold the
	// non-secret configuration. Model ids are comma separated; an id never contains a comma.
	ContextBaseURL  = "base_url"
	ContextModelIDs = "model_ids"

	chatCompletionsPath = "/v1/chat/completions"
	maxResponseBytes    = 16 << 20
	maxStreamLineBytes  = 1 << 20
	maxBaseURLLength    = 2048
	maxNameLength       = 80
	maxModelIDLength    = 128
	maxAPIKeyLength     = 4096
	maxMessages         = 2000
	userAgent           = "verevon-integration-core/customer-gateway"
)

// Failure is a classified, content-free failure. Code and Message are fixed text and are safe to
// return to a caller; Cause is a scrubbed diagnostic for the service log only.
type Failure struct {
	Status int
	Code   string
	// Message is fixed English, never derived from the gateway's response.
	Message string
	// RetryAfterSeconds is set when the gateway sent a usable Retry-After.
	RetryAfterSeconds int
	// Cause is for logs only: the key is scrubbed from it and it is never returned to a caller.
	Cause string
}

func (f *Failure) Error() string { return f.Code + ": " + f.Message }

func fail(status int, code, message string) *Failure {
	return &Failure{Status: status, Code: code, Message: message}
}

// IsFailure reports whether err is a Failure and returns it.
func IsFailure(err error) (*Failure, bool) {
	var failure *Failure
	if errors.As(err, &failure) {
		return failure, true
	}
	return nil, false
}

// Scrub removes every occurrence of the API key from text.
func Scrub(text, apiKey string) string {
	if apiKey == "" {
		return text
	}
	return strings.ReplaceAll(text, apiKey, "[redacted]")
}

// Config wires a Broker.
type Config struct {
	// Vault seals and opens the API key. Required.
	Vault *secretcrypto.Vault
	// Guard vets destinations at registration time. Nil uses the process-wide egress.Default().
	Guard *egress.Guard
	// Client makes the gateway calls. Nil builds an egress.SafeClient over Guard with no
	// redirects and RequestTimeout. A test may inject its own.
	Client *http.Client
}

// Broker registers and calls customer gateways.
type Broker struct {
	vault  *secretcrypto.Vault
	guard  *egress.Guard
	client *http.Client
}

// New builds a Broker.
func New(cfg Config) (*Broker, error) {
	if cfg.Vault == nil {
		return nil, errors.New("customer gateway broker requires a credential vault")
	}
	guard := cfg.Guard
	if guard == nil {
		guard = egress.Default()
	}
	client := cfg.Client
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{Guard: guard, RequestTimeout: RequestTimeout})
	}
	return &Broker{vault: cfg.Vault, guard: guard, client: client}, nil
}

// ValidateName trims and checks a connection display name.
func ValidateName(raw string) (string, error) {
	name := strings.TrimSpace(raw)
	if name == "" {
		return "", fail(400, "invalid_name", "name is required.")
	}
	if len([]rune(name)) > maxNameLength {
		return "", fail(400, "invalid_name", fmt.Sprintf("name must be at most %d characters.", maxNameLength))
	}
	for _, r := range name {
		if r < 0x20 || r == 0x7f {
			return "", fail(400, "invalid_name", "name must not contain control characters.")
		}
	}
	return name, nil
}

// ValidateAPIKey checks an API key without storing or echoing it. A key that could not be sent
// as one header value (whitespace, control characters) is refused.
func ValidateAPIKey(raw string) (string, error) {
	key := strings.TrimSpace(raw)
	if key == "" {
		return "", fail(400, "invalid_api_key", "api_key is required.")
	}
	if len(key) > maxAPIKeyLength {
		return "", fail(400, "invalid_api_key", "api_key is too long.")
	}
	for _, r := range key {
		if r <= 0x20 || r >= 0x7f {
			return "", fail(400, "invalid_api_key", "api_key must be a single printable token.")
		}
	}
	return key, nil
}

// ValidateModelIDs trims, checks and de-duplicates the model ids, keeping their order.
func ValidateModelIDs(raw []string) ([]string, error) {
	if len(raw) == 0 {
		return nil, fail(400, "invalid_model_ids", "model_ids must list at least one model.")
	}
	if len(raw) > MaxModelIDs {
		return nil, fail(400, "invalid_model_ids", fmt.Sprintf("model_ids may list at most %d models.", MaxModelIDs))
	}
	seen := make(map[string]struct{}, len(raw))
	out := make([]string, 0, len(raw))
	for _, item := range raw {
		id := strings.TrimSpace(item)
		if !validModelID(id) {
			return nil, fail(400, "invalid_model_ids", "model_ids must be non-empty model names of letters, digits and . _ : / @ + - only.")
		}
		if _, dup := seen[id]; dup {
			continue
		}
		seen[id] = struct{}{}
		out = append(out, id)
	}
	return out, nil
}

func validModelID(id string) bool {
	if id == "" || len(id) > maxModelIDLength {
		return false
	}
	for i := 0; i < len(id); i++ {
		ch := id[i]
		switch {
		case ch >= 'a' && ch <= 'z', ch >= 'A' && ch <= 'Z', ch >= '0' && ch <= '9':
		case i > 0 && strings.IndexByte("._:/@+-", ch) >= 0:
		default:
			return false
		}
	}
	return true
}

// NormalizeBaseURL checks the syntax of a gateway base URL and returns its canonical form: https
// only, no credentials, query or fragment, a plain host, and no trailing slash or trailing /v1
// (the broker appends /v1/chat/completions itself). It does not resolve the host; see
// Broker.CheckDestination.
func NormalizeBaseURL(raw string) (string, error) {
	text := strings.TrimSpace(raw)
	if text == "" {
		return "", fail(400, "invalid_base_url", "base_url is required.")
	}
	if len(text) > maxBaseURLLength {
		return "", fail(400, "invalid_base_url", "base_url is too long.")
	}
	parsed, err := url.Parse(text)
	if err != nil {
		return "", fail(400, "invalid_base_url", "base_url is not a valid URL.")
	}
	switch {
	case !strings.EqualFold(parsed.Scheme, "https"):
		return "", fail(400, "invalid_base_url", "base_url must use https.")
	case parsed.Opaque != "" || parsed.Hostname() == "":
		return "", fail(400, "invalid_base_url", "base_url must name a host.")
	case parsed.User != nil:
		return "", fail(400, "invalid_base_url", "base_url must not contain credentials.")
	case parsed.RawQuery != "" || parsed.ForceQuery || parsed.Fragment != "":
		return "", fail(400, "invalid_base_url", "base_url must not contain a query or fragment.")
	case parsed.RawPath != "" || strings.Contains(parsed.Path, "%"):
		return "", fail(400, "invalid_base_url", "base_url must not contain encoded path characters.")
	}
	if port := parsed.Port(); port != "" {
		number, err := strconv.Atoi(port)
		if err != nil || number < 1 || number > 65535 {
			return "", fail(400, "invalid_base_url", "base_url has an invalid port.")
		}
	}
	path := strings.TrimRight(parsed.Path, "/")
	path = strings.TrimSuffix(path, "/v1")
	return "https://" + strings.ToLower(parsed.Host) + path, nil
}

// CheckDestination refuses a base URL whose host does not resolve to public addresses only.
// Every call re-vets the destination as it dials (the egress client pins the vetted address);
// this check is so a registration of an unreachable or private host fails up front.
func (b *Broker) CheckDestination(ctx context.Context, baseURL string) error {
	parsed, err := url.Parse(baseURL)
	if err != nil || parsed.Hostname() == "" {
		return fail(400, "invalid_base_url", "base_url is not a valid URL.")
	}
	if _, err := b.guard.ResolveAndVet(ctx, parsed.Hostname()); err != nil {
		if errors.Is(err, egress.ErrBlocked) {
			return fail(400, "base_url_not_public", "base_url must point to a public host.")
		}
		return fail(400, "base_url_unresolvable", "base_url host could not be resolved.")
	}
	return nil
}

// SealAPIKey encrypts the API key for storage on the connection with the given id. The id is the
// AAD, so a ciphertext copied onto another connection does not open.
func (b *Broker) SealAPIKey(connectionID, apiKey string) (string, error) {
	sealed, err := b.vault.Encrypt(apiKey, []byte(connectionID))
	if err != nil {
		return "", fail(503, "gateway_credential_unavailable", "The gateway credential could not be stored safely.")
	}
	return sealed, nil
}

// Target is one connection resolved for a call. It holds the opened key, so it must not be
// logged or serialised; its String and GoString methods omit it.
type Target struct {
	ConnectionID string
	BaseURL      string
	ModelIDs     []string
	apiKey       string
}

func (t Target) String() string   { return "customergateway.Target{" + t.ConnectionID + "}" }
func (t Target) GoString() string { return t.String() }

// HasModel reports whether the connection lists the model.
func (t Target) HasModel(model string) bool {
	for _, id := range t.ModelIDs {
		if id == model {
			return true
		}
	}
	return false
}

// IsGatewayConnection reports whether the connection is a live customer-gateway row.
func IsGatewayConnection(connection store.Connection) bool {
	return connection.ProviderKey == ProviderKey && connection.ConnectorType == ConnectorType && connection.DeletedAt == nil
}

// ModelIDsFromContext reads the model ids from a connection's provider context.
func ModelIDsFromContext(providerContext map[string]string) []string {
	var ids []string
	for _, part := range strings.Split(providerContext[ContextModelIDs], ",") {
		if id := strings.TrimSpace(part); id != "" {
			ids = append(ids, id)
		}
	}
	return ids
}

// TargetFor opens the connection's key and reads its configuration.
func (b *Broker) TargetFor(connection store.Connection) (Target, error) {
	if !IsGatewayConnection(connection) {
		return Target{}, store.ErrNotFound
	}
	baseURL := strings.TrimSpace(connection.ProviderContext[ContextBaseURL])
	if baseURL == "" || connection.EncryptedAccessToken == "" {
		return Target{}, fail(409, "gateway_connection_incomplete", "The gateway connection is not fully configured.")
	}
	apiKey, err := b.vault.Decrypt(connection.EncryptedAccessToken, []byte(connection.ID))
	if err != nil {
		return Target{}, fail(409, "gateway_credential_unreadable", "The gateway credential could not be read. Register the gateway again.")
	}
	return Target{
		ConnectionID: connection.ID,
		BaseURL:      baseURL,
		ModelIDs:     ModelIDsFromContext(connection.ProviderContext),
		apiKey:       apiKey,
	}, nil
}

// InferRequest is one chat completion to send.
type InferRequest struct {
	Model string
	// Messages are OpenAI chat messages, passed through untouched.
	Messages []json.RawMessage
	// Params are optional OpenAI sampling and tool parameters. Only the allow-listed names are
	// forwarded; model, messages and stream are always set by the broker.
	Params map[string]json.RawMessage
	Stream bool
}

// allowedParams are the request parameters a caller may pass through.
var allowedParams = map[string]struct{}{
	"temperature": {}, "top_p": {}, "max_tokens": {}, "max_completion_tokens": {}, "stop": {},
	"presence_penalty": {}, "frequency_penalty": {}, "seed": {}, "tools": {}, "tool_choice": {},
	"parallel_tool_calls": {}, "response_format": {}, "reasoning_effort": {}, "n": {},
	"logit_bias": {}, "user": {},
}

// CheckRequest validates the shape of a request before any call is made.
func CheckRequest(req InferRequest) error {
	if strings.TrimSpace(req.Model) == "" || len(req.Messages) == 0 {
		return fail(400, "invalid_gateway_request", "model and messages are required.")
	}
	if len(req.Messages) > maxMessages {
		return fail(400, "invalid_gateway_request", "too many messages.")
	}
	for _, message := range req.Messages {
		trimmed := bytes.TrimSpace(message)
		if len(trimmed) == 0 || trimmed[0] != '{' || !json.Valid(trimmed) {
			return fail(400, "invalid_gateway_request", "each message must be a JSON object.")
		}
	}
	for name, value := range req.Params {
		if _, ok := allowedParams[name]; !ok {
			return fail(400, "invalid_gateway_param", "params contains a parameter the gateway broker does not forward.")
		}
		if !json.Valid(value) {
			return fail(400, "invalid_gateway_param", "params contains a value that is not valid JSON.")
		}
	}
	return nil
}

func buildBody(req InferRequest) ([]byte, error) {
	body := make(map[string]json.RawMessage, len(req.Params)+4)
	for name, value := range req.Params {
		if _, ok := allowedParams[name]; ok {
			body[name] = value
		}
	}
	model, err := json.Marshal(req.Model)
	if err != nil {
		return nil, fail(400, "invalid_gateway_request", "model is not valid.")
	}
	messages, err := json.Marshal(req.Messages)
	if err != nil {
		return nil, fail(400, "invalid_gateway_request", "messages are not valid.")
	}
	body["model"] = model
	body["messages"] = messages
	if req.Stream {
		body["stream"] = json.RawMessage("true")
		body["stream_options"] = json.RawMessage(`{"include_usage":true}`)
	} else {
		body["stream"] = json.RawMessage("false")
	}
	return json.Marshal(body)
}

// Result is a completed unary call. Body is the gateway's JSON answer, relayed as is.
type Result struct {
	Body   []byte
	Status int
	ID     string
	Model  string
	Usage  json.RawMessage
}

// Invoke sends one non-streaming chat completion.
func (b *Broker) Invoke(ctx context.Context, target Target, req InferRequest) (Result, error) {
	req.Stream = false
	resp, cancel, err := b.send(ctx, target, req, "application/json")
	if err != nil {
		return Result{}, err
	}
	defer cancel()
	defer resp.Body.Close()
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxResponseBytes+1))
	if err != nil {
		return Result{}, b.transportFailure(err, target)
	}
	if len(body) > maxResponseBytes || !json.Valid(body) {
		return Result{}, fail(502, "gateway_bad_response", "The gateway returned a response that could not be used.")
	}
	id, model, usage := completionInfo(body)
	return Result{Body: body, Status: resp.StatusCode, ID: id, Model: model, Usage: usage}, nil
}

// Stream is an open streamed chat completion. The caller must Close it.
type Stream struct {
	resp    *http.Response
	scanner *bufio.Scanner
	cancel  context.CancelFunc
	target  Target
	broker  *Broker
}

// OpenStream sends a streaming chat completion and returns once the gateway has answered with a
// stream, so a refusal (rate limit, bad key, bad request) is still an ordinary error before any
// byte is relayed.
func (b *Broker) OpenStream(ctx context.Context, target Target, req InferRequest) (*Stream, error) {
	req.Stream = true
	resp, cancel, err := b.send(ctx, target, req, "text/event-stream")
	if err != nil {
		return nil, err
	}
	if !strings.HasPrefix(strings.ToLower(resp.Header.Get("Content-Type")), "text/event-stream") {
		resp.Body.Close()
		cancel()
		return nil, fail(502, "gateway_stream_unsupported", "The gateway did not answer with a stream.")
	}
	scanner := bufio.NewScanner(resp.Body)
	scanner.Buffer(make([]byte, 0, 64<<10), maxStreamLineBytes)
	return &Stream{resp: resp, scanner: scanner, cancel: cancel, target: target, broker: b}, nil
}

// Next returns the next chunk's JSON payload. done is true after the gateway's [DONE] marker or a
// clean end of stream.
func (s *Stream) Next() (chunk []byte, done bool, err error) {
	for s.scanner.Scan() {
		line := s.scanner.Text()
		if !strings.HasPrefix(line, "data:") {
			continue
		}
		payload := strings.TrimSpace(strings.TrimPrefix(line, "data:"))
		switch {
		case payload == "":
			continue
		case payload == "[DONE]":
			return nil, true, nil
		case !json.Valid([]byte(payload)):
			continue
		}
		return []byte(payload), false, nil
	}
	if err := s.scanner.Err(); err != nil {
		return nil, false, s.broker.transportFailure(err, s.target)
	}
	return nil, true, nil
}

// Close releases the connection.
func (s *Stream) Close() {
	if s == nil {
		return
	}
	_ = s.resp.Body.Close()
	s.cancel()
}

// ChunkInfo reads the model and the usage block (when present and not null) from a streamed chunk.
func ChunkInfo(chunk []byte) (model string, usage json.RawMessage) {
	_, model, usage = completionInfo(chunk)
	return model, usage
}

func completionInfo(body []byte) (id, model string, usage json.RawMessage) {
	var envelope struct {
		ID    string          `json:"id"`
		Model string          `json:"model"`
		Usage json.RawMessage `json:"usage"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return "", "", nil
	}
	if trimmed := bytes.TrimSpace(envelope.Usage); len(trimmed) > 0 && !bytes.Equal(trimmed, []byte("null")) {
		usage = envelope.Usage
	}
	return envelope.ID, envelope.Model, usage
}

// TokenCounts reads the token counts from a usage block, with zero for any that is absent.
func TokenCounts(usage json.RawMessage) (prompt, completion, total int64) {
	var counts struct {
		Prompt     int64 `json:"prompt_tokens"`
		Completion int64 `json:"completion_tokens"`
		Total      int64 `json:"total_tokens"`
	}
	if len(usage) == 0 || json.Unmarshal(usage, &counts) != nil {
		return 0, 0, 0
	}
	return counts.Prompt, counts.Completion, counts.Total
}

// send makes the request and returns the response only when the gateway answered 2xx. The cancel
// func owns the 120 second deadline and must be called when the body is no longer needed.
func (b *Broker) send(ctx context.Context, target Target, req InferRequest, accept string) (*http.Response, context.CancelFunc, error) {
	if err := CheckRequest(req); err != nil {
		return nil, nil, err
	}
	payload, err := buildBody(req)
	if err != nil {
		return nil, nil, err
	}
	callCtx, cancel := context.WithTimeout(ctx, RequestTimeout)
	httpReq, err := http.NewRequestWithContext(callCtx, http.MethodPost, target.BaseURL+chatCompletionsPath, bytes.NewReader(payload))
	if err != nil {
		cancel()
		return nil, nil, fail(409, "gateway_connection_incomplete", "The gateway connection is not fully configured.")
	}
	httpReq.Header.Set("Authorization", "Bearer "+target.apiKey)
	httpReq.Header.Set("Content-Type", "application/json")
	httpReq.Header.Set("Accept", accept)
	httpReq.Header.Set("User-Agent", userAgent)
	resp, err := b.client.Do(httpReq)
	if err != nil {
		cancel()
		return nil, nil, b.transportFailure(err, target)
	}
	if resp.StatusCode < 200 || resp.StatusCode > 299 {
		failure := statusFailure(resp)
		// The body is discarded unread: a gateway's error text is never relayed, because it can
		// echo the request, and with it the key.
		_ = resp.Body.Close()
		cancel()
		failure.Cause = Scrub(failure.Cause, target.apiKey)
		return nil, nil, failure
	}
	return resp, cancel, nil
}

// statusFailure classifies a non-2xx answer by status alone.
func statusFailure(resp *http.Response) *Failure {
	code := resp.StatusCode
	var failure *Failure
	switch {
	case code == http.StatusUnauthorized || code == http.StatusForbidden:
		failure = fail(502, "gateway_credential_rejected", "The gateway rejected the stored API key.")
	case code == http.StatusTooManyRequests:
		failure = fail(429, "gateway_rate_limited", "The gateway is rate limiting requests. Retry later.")
		if seconds, err := strconv.Atoi(strings.TrimSpace(resp.Header.Get("Retry-After"))); err == nil && seconds > 0 && seconds <= 3600 {
			failure.RetryAfterSeconds = seconds
		}
	case code == http.StatusRequestEntityTooLarge:
		failure = fail(413, "gateway_context_too_long", "The conversation is too long for the gateway model.")
	case code == http.StatusBadRequest || code == http.StatusNotFound || code == http.StatusUnprocessableEntity:
		failure = fail(422, "gateway_request_rejected", "The gateway rejected the request.")
	case code >= 300 && code < 400:
		failure = fail(502, "gateway_redirect_refused", "The gateway answered with a redirect, which is not followed.")
	case code >= 500:
		failure = fail(502, "gateway_upstream_error", "The gateway reported an error. Try again shortly.")
	default:
		failure = fail(502, "gateway_unexpected_status", "The gateway answered with an unexpected status.")
	}
	failure.Cause = "gateway status " + strconv.Itoa(code)
	return failure
}

// transportFailure classifies a transport error. The error text goes only into Cause, scrubbed of
// the key; the returned message is fixed.
func (b *Broker) transportFailure(err error, target Target) *Failure {
	var failure *Failure
	var netErr net.Error
	switch {
	case errors.Is(err, egress.ErrBlocked):
		failure = fail(502, "gateway_destination_blocked", "The gateway address is not allowed.")
	case errors.Is(err, context.DeadlineExceeded) || (errors.As(err, &netErr) && netErr.Timeout()):
		failure = fail(504, "gateway_timeout", "The gateway did not answer in time.")
	case errors.Is(err, context.Canceled):
		failure = fail(502, "gateway_request_canceled", "The gateway request was canceled.")
	default:
		failure = fail(502, "gateway_unreachable", "The gateway could not be reached.")
	}
	failure.Cause = Scrub(err.Error(), target.apiKey)
	return failure
}
