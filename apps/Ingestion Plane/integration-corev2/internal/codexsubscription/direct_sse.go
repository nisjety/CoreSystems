package codexsubscription

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"regexp"
	"slices"
	"strings"
	"time"
)

const (
	sseInitialBuffer = 64 << 10
	sseMaxEventBytes = 16 << 20
)

var servedModelNamePattern = regexp.MustCompile(`^[A-Za-z0-9._:-]{1,64}$`)

// sseFrame is one dispatched server-sent event.
type sseFrame struct {
	event string
	data  string
}

// sseReader splits an SSE body into events. LF and CRLF line endings are
// accepted, comment lines are ignored, and multiple data lines are joined
// with "\n". A single line or event above 16 MiB is bufio.ErrTooLong.
type sseReader struct {
	scanner *bufio.Scanner
}

func newSSEReader(body io.Reader) *sseReader {
	scanner := bufio.NewScanner(body)
	scanner.Buffer(make([]byte, sseInitialBuffer), sseMaxEventBytes)
	return &sseReader{scanner: scanner}
}

// next returns the next event, io.EOF at a clean end, or the read error.
func (r *sseReader) next() (sseFrame, error) {
	var frame sseFrame
	var data strings.Builder
	hasData := false
	for r.scanner.Scan() {
		line := r.scanner.Text()
		if line == "" {
			if hasData || frame.event != "" {
				frame.data = data.String()
				return frame, nil
			}
			continue
		}
		if strings.HasPrefix(line, ":") {
			continue
		}
		field, value, found := strings.Cut(line, ":")
		if found {
			value = strings.TrimPrefix(value, " ")
		}
		switch field {
		case "data":
			if hasData {
				data.WriteByte('\n')
			}
			if data.Len()+len(value) > sseMaxEventBytes {
				return sseFrame{}, bufio.ErrTooLong
			}
			data.WriteString(value)
			hasData = true
		case "event":
			frame.event = value
		}
	}
	if err := r.scanner.Err(); err != nil {
		return sseFrame{}, err
	}
	if hasData {
		// A final event without its blank line still counts.
		frame.data = data.String()
		return frame, nil
	}
	return sseFrame{}, io.EOF
}

// deltaSinkError reports that forwarding a delta failed: the stream caller
// stopped reading. The call is aborted and never retried.
type deltaSinkError struct{ err error }

func (e *deltaSinkError) Error() string { return "stream Codex subscription delta: " + e.err.Error() }
func (e *deltaSinkError) Unwrap() error { return e.err }

// streamState is shared by every attempt of one call: once any answer text
// arrived, or anything was forwarded, the call can no longer be retried.
type streamState struct {
	onDelta   func(string) error
	deltaSeen bool
	forwarded bool
}

func (s *streamState) started() bool { return s.deltaSeen || s.forwarded }

// streamResult is what one successful stream produced.
type streamResult struct {
	content        string
	usage          *TokenUsage
	createdModel   string
	completedModel string
	// eventModel is the model an event's headers announced (see
	// eventServedModels).
	eventModel  string
	servedTier  string
	firstText   time.Time
	completedAt time.Time
}

// servedModel is the first non-empty served model the stream reported.
func (r streamResult) servedModel(headers http.Header) string {
	return firstNonEmpty(r.completedModel, r.eventModel, r.createdModel, httpServedModel(headers))
}

// httpServedModel is the served model from the response headers.
func httpServedModel(headers http.Header) string {
	return firstNonEmpty(headers.Get("openai-model"), headers.Get("x-openai-model"))
}

// eventServedModels reads the models an SSE event's headers say answered,
// the way Codex does (codex-api sse/responses.rs response_model): the
// event's response.headers first, then its top-level headers, with the header
// name openai-model or x-openai-model in any case. This is how the backend
// announces a rerouted model, often without any response.model. Every value
// is returned, so none can hide behind another.
func eventServedModels(payload []byte) []string {
	if !bytes.Contains(payload, []byte(`"headers"`)) {
		return nil
	}
	var event struct {
		Response json.RawMessage `json:"response"`
		Headers  json.RawMessage `json:"headers"`
	}
	if json.Unmarshal(payload, &event) != nil {
		return nil
	}
	var response struct {
		Headers json.RawMessage `json:"headers"`
	}
	var models []string
	if len(event.Response) > 0 && json.Unmarshal(event.Response, &response) == nil {
		models = headerMapModels(response.Headers)
	}
	return append(models, headerMapModels(event.Headers)...)
}

func headerMapModels(raw json.RawMessage) []string {
	var headers map[string]json.RawMessage
	if len(raw) == 0 || json.Unmarshal(raw, &headers) != nil {
		return nil
	}
	var models []string
	for name, value := range headers {
		if !strings.EqualFold(name, "openai-model") && !strings.EqualFold(name, "x-openai-model") {
			continue
		}
		var model string
		if json.Unmarshal(value, &model) == nil && strings.TrimSpace(model) != "" {
			models = append(models, strings.TrimSpace(model))
		}
	}
	slices.Sort(models)
	return models
}

// mismatchError is the served_model_mismatch failure; it keeps the sanitized
// served model name for the log line.
func mismatchError(served string, headers http.Header) *UpstreamError {
	return &UpstreamError{Kind: upstreamKindMismatch, RequestID: upstreamRequestID(headers), servedModel: sanitizeServedModel(served)}
}

type sseResponse struct {
	ID                string          `json:"id"`
	Model             string          `json:"model"`
	ServiceTier       string          `json:"service_tier"`
	Usage             json.RawMessage `json:"usage"`
	Output            json.RawMessage `json:"output"`
	Error             json.RawMessage `json:"error"`
	IncompleteDetails json.RawMessage `json:"incomplete_details"`
}

type sseOutputItem struct {
	Type    string `json:"type"`
	Content []struct {
		Type string `json:"type"`
		Text string `json:"text"`
	} `json:"content"`
}

// consumeStream reads one Responses SSE body to its terminal event (§5.1).
// Deltas are forwarded exactly as received. Reasoning, encrypted content and
// every other event are ignored and never kept.
func consumeStream(ctx context.Context, body io.Reader, request InvokeRequest, headers http.Header, state *streamState, now func() time.Time) (streamResult, error) {
	reader := newSSEReader(body)
	var answer strings.Builder
	var itemText strings.Builder
	var result streamResult
	sawValid, sawMalformed := false, false
	markText := func() {
		if result.firstText.IsZero() {
			result.firstText = now()
		}
	}
	for {
		frame, err := reader.next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			if errors.Is(err, bufio.ErrTooLong) {
				return streamResult{}, &UpstreamError{Kind: upstreamKindTooLarge, RequestID: upstreamRequestID(headers)}
			}
			if ctxErr := ctx.Err(); ctxErr != nil {
				return streamResult{}, fmt.Errorf("read ChatGPT subscription stream: %w", ctxErr)
			}
			return streamResult{}, &UpstreamError{Kind: upstreamKindTruncated, Code: transportErrorCode(err), RequestID: upstreamRequestID(headers), Retryable: true}
		}
		if frame.data == "" || frame.data == "[DONE]" {
			continue
		}
		payload := []byte(frame.data)
		var head struct {
			Type string `json:"type"`
		}
		if json.Unmarshal(payload, &head) != nil {
			sawMalformed = true
			continue
		}
		sawValid = true
		for _, model := range eventServedModels(payload) {
			if !servedModelMatches(request.Model, model) {
				return streamResult{}, mismatchError(model, headers)
			}
			if result.eventModel == "" {
				result.eventModel = model
			}
		}
		switch head.Type {
		case "response.created":
			var event struct {
				Response sseResponse `json:"response"`
			}
			if json.Unmarshal(payload, &event) == nil {
				result.createdModel = strings.TrimSpace(event.Response.Model)
				if !servedModelMatches(request.Model, result.createdModel) {
					return streamResult{}, mismatchError(result.createdModel, headers)
				}
			}
		case "response.output_text.delta":
			var event struct {
				Delta string `json:"delta"`
			}
			if json.Unmarshal(payload, &event) != nil || event.Delta == "" {
				continue
			}
			markText()
			state.deltaSeen = true
			answer.WriteString(event.Delta)
			if state.onDelta != nil {
				state.forwarded = true
				if err := state.onDelta(event.Delta); err != nil {
					return streamResult{}, &deltaSinkError{err: err}
				}
			}
		case "response.output_item.done":
			var event struct {
				Item sseOutputItem `json:"item"`
			}
			if json.Unmarshal(payload, &event) == nil && event.Item.Type == "message" {
				if text := outputItemText(event.Item); text != "" {
					if answer.Len() == 0 {
						markText()
					}
					itemText.WriteString(text)
				}
			}
		case "response.completed", "response.done":
			var event struct {
				Response sseResponse `json:"response"`
			}
			_ = json.Unmarshal(payload, &event)
			result.completedModel = strings.TrimSpace(event.Response.Model)
			result.servedTier = sanitizeServedTier(event.Response.ServiceTier)
			result.usage = parseResponseUsage(event.Response.Usage)
			if served := result.servedModel(headers); !servedModelMatches(request.Model, served) {
				return streamResult{}, mismatchError(served, headers)
			}
			content := answer.String()
			if content == "" {
				content = itemText.String()
				if content == "" {
					content = completedOutputText(event.Response.Output)
				}
				if strings.TrimSpace(content) != "" {
					markText()
					if state.onDelta != nil {
						state.forwarded = true
						if err := state.onDelta(content); err != nil {
							return streamResult{}, &deltaSinkError{err: err}
						}
					}
				}
			}
			if strings.TrimSpace(content) == "" {
				return streamResult{}, &UpstreamError{Kind: upstreamKindEmpty, RequestID: upstreamRequestID(headers)}
			}
			result.content = content
			result.completedAt = now()
			return result, nil
		case "response.incomplete":
			var event struct {
				Response sseResponse `json:"response"`
			}
			_ = json.Unmarshal(payload, &event)
			var details struct {
				Reason string `json:"reason"`
			}
			_ = json.Unmarshal(event.Response.IncompleteDetails, &details)
			return streamResult{}, &UpstreamError{Kind: upstreamKindIncomplete, Code: sanitizeUpstreamCode(details.Reason), RequestID: upstreamRequestID(headers)}
		case "response.failed":
			var event struct {
				Response sseResponse `json:"response"`
			}
			_ = json.Unmarshal(payload, &event)
			return streamResult{}, classifySSEFailure(parseUpstreamErrorObject(event.Response.Error), headers, time.Now())
		case "error":
			failure := parseUpstreamErrorBody(payload)
			if strings.EqualFold(failure.Type, "error") {
				failure.Type = ""
			}
			return streamResult{}, classifySSEFailure(failure, headers, time.Now())
		}
	}
	if ctxErr := ctx.Err(); ctxErr != nil {
		return streamResult{}, fmt.Errorf("read ChatGPT subscription stream: %w", ctxErr)
	}
	if !sawValid && sawMalformed {
		return streamResult{}, &UpstreamError{Kind: upstreamKindMalformed, RequestID: upstreamRequestID(headers)}
	}
	return streamResult{}, &UpstreamError{Kind: upstreamKindTruncated, RequestID: upstreamRequestID(headers), Retryable: true}
}

// parseUpstreamErrorObject reads a bare `{code,type,message,...}` object.
func parseUpstreamErrorObject(raw json.RawMessage) upstreamFailure {
	var inner rawUpstreamError
	if len(raw) == 0 || json.Unmarshal(raw, &inner) != nil {
		return upstreamFailure{}
	}
	return upstreamFailure{
		Code:            rawCodeString(inner.Code),
		Type:            inner.Type,
		Param:           inner.Param,
		Message:         inner.Message,
		ResetsAt:        inner.ResetsAt,
		ResetsInSeconds: inner.ResetsInSeconds,
	}
}

func outputItemText(item sseOutputItem) string {
	var text strings.Builder
	for _, part := range item.Content {
		if part.Type == "output_text" {
			text.WriteString(part.Text)
		}
	}
	return text.String()
}

// completedOutputText concatenates the message items of response.output, in
// order, with no separator.
func completedOutputText(raw json.RawMessage) string {
	var items []sseOutputItem
	if len(raw) == 0 || json.Unmarshal(raw, &items) != nil {
		return ""
	}
	var text strings.Builder
	for _, item := range items {
		if item.Type == "message" {
			text.WriteString(outputItemText(item))
		}
	}
	return text.String()
}

// parseResponseUsage maps response.usage to TokenUsage (§5.1). Absent or
// malformed usage is nil: telemetry must never fail an answer.
func parseResponseUsage(raw json.RawMessage) *TokenUsage {
	if len(raw) == 0 || string(raw) == "null" {
		return nil
	}
	var usage struct {
		InputTokens        *int64 `json:"input_tokens"`
		OutputTokens       *int64 `json:"output_tokens"`
		InputTokensDetails *struct {
			CachedTokens     int64 `json:"cached_tokens"`
			CacheWriteTokens int64 `json:"cache_write_tokens"`
		} `json:"input_tokens_details"`
		OutputTokensDetails *struct {
			ReasoningTokens int64 `json:"reasoning_tokens"`
		} `json:"output_tokens_details"`
	}
	if json.Unmarshal(raw, &usage) != nil || usage.InputTokens == nil || usage.OutputTokens == nil {
		return nil
	}
	mapped := &TokenUsage{InputTokens: *usage.InputTokens, OutputTokens: *usage.OutputTokens}
	if usage.InputTokensDetails != nil {
		mapped.CachedInputTokens = usage.InputTokensDetails.CachedTokens
		mapped.CacheWriteInputTokens = usage.InputTokensDetails.CacheWriteTokens
	}
	if usage.OutputTokensDetails != nil {
		mapped.ReasoningOutputTokens = usage.OutputTokensDetails.ReasoningTokens
	}
	return mapped
}

// servedModelMatches accepts an empty served model, the requested model, or
// a dated snapshot of it (§5.3).
func servedModelMatches(requested, served string) bool {
	served = strings.TrimSpace(served)
	if served == "" || served == requested {
		return true
	}
	matched, err := regexp.MatchString(`^`+regexp.QuoteMeta(requested)+`-\d{4}-\d{2}-\d{2}$`, served)
	return err == nil && matched
}

func sanitizeServedModel(model string) string {
	model = strings.TrimSpace(model)
	if !servedModelNamePattern.MatchString(model) {
		return ""
	}
	return model
}

func sanitizeServedTier(tier string) string {
	tier = strings.ToLower(strings.TrimSpace(tier))
	if !upstreamCodePattern.MatchString(tier) {
		return ""
	}
	return tier
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if trimmed := strings.TrimSpace(value); trimmed != "" {
			return trimmed
		}
	}
	return ""
}
