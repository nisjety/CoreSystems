package codexsubscription

import (
	"encoding/json"
	"errors"
	"math"
	"net/http"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// ErrUpstreamRateLimited matches every *RateLimitError, so callers can test
// for a rate limit with errors.Is without caring which kind it was.
var ErrUpstreamRateLimited = errors.New("ChatGPT subscription rate limit reached")

// RateLimitKind says why a call was rate limited.
type RateLimitKind string

const (
	// RateLimitUsage is an exhausted ChatGPT plan window.
	RateLimitUsage RateLimitKind = "usage_limit"
	// RateLimitThrottle is a short-term upstream rate limit.
	RateLimitThrottle RateLimitKind = "throttle"
	// RateLimitQueue is the broker's own concurrency queue timing out.
	RateLimitQueue RateLimitKind = "broker_queue"
)

const (
	minRetryAfter           = time.Second
	maxRetryAfter           = 7 * 24 * time.Hour
	defaultThrottleRetry    = time.Second
	maxUpstreamErrorBody    = 64 << 10
	upstreamCodeOther       = "other"
	upstreamKindHTTPStatus  = "http_status"
	upstreamKindTransport   = "transport"
	upstreamKindIncomplete  = "incomplete"
	upstreamKindTruncated   = "stream_truncated"
	upstreamKindMalformed   = "malformed_stream"
	upstreamKindTooLarge    = "stream_event_too_large"
	upstreamKindEmpty       = "empty_answer"
	upstreamKindMismatch    = "served_model_mismatch"
	upstreamKindFailed      = "response_failed"
	upstreamKindQuota       = "quota"
	upstreamKindNotIncluded = "usage_not_included"
	upstreamKindContext     = "context_length_exceeded"
	upstreamKindPolicy      = "policy"
	upstreamKindRefresh     = "token_refresh_unavailable"
	// upstreamKindUnauthorized is a direct 401 after the call's one
	// successful credential recovery, or after answer text.
	upstreamKindUnauthorized = "unauthorized_after_recovery"
	// upstreamKindTurnFailed is an app-server turn that ended failed; Code is
	// its codexErrorInfo variant in snake_case.
	upstreamKindTurnFailed = "turn_failed"
)

// RateLimitError reports an upstream usage limit or throttle, or the broker's
// own queue timing out. A throttle or queue RetryAfter is always between 1s
// and 7 days. A usage limit carries its reset when the upstream reported one:
// ResetsAt (zero when unknown) and RetryAfter (0 when unknown); there is no
// made-up default. WindowMinutes and LimitReachedType describe the exhausted
// window when known. None of it is content: no plan type or upstream text.
type RateLimitError struct {
	Kind             RateLimitKind
	RetryAfter       time.Duration
	ResetsAt         time.Time
	WindowMinutes    int
	LimitReachedType string

	// upstreamCode is the sanitized upstream code or app-server variant, for
	// the log line only.
	upstreamCode string
	// resetSource names where the reset came from (notification or read on
	// the app-server; resets_at, resets_in_seconds, headers or retry_after on
	// the direct transport; none), for the log line only.
	resetSource string
	// limitID is the app-server rate-limit bucket a notification named, so
	// account/rateLimits/read can look up the same bucket.
	limitID string
}

func (e *RateLimitError) Error() string {
	return "ChatGPT subscription rate limited (" + string(e.Kind) + ")"
}

func (e *RateLimitError) Is(target error) bool { return target == ErrUpstreamRateLimited }

// UpstreamError is a content-free description of a failed direct call: it
// never carries upstream message text, headers, or any part of a prompt,
// answer, or credential.
type UpstreamError struct {
	Kind       string
	HTTPStatus int
	// Code is a sanitized upstream error code from a known list, else "other".
	Code string
	// RequestID is the upstream's own request id, sanitized.
	RequestID string
	Retryable bool

	// retryAfter is the upstream's retry hint, when it sent one.
	retryAfter    time.Duration
	hasRetryAfter bool
	// tierRejected marks a failure that names service_tier.
	tierRejected bool
	// servedModel is the sanitized model a served_model_mismatch reported,
	// for the log line only.
	servedModel string
	// upstreamRetries counts the app-server's willRetry:true error
	// notifications before the turn failed: Codex's own retries.
	upstreamRetries int
	// retryHTTPStatus is the upstream HTTP status the latest of those
	// notifications named, 0 when none did (a dropped stream, a timeout).
	retryHTTPStatus int
}

func (e *UpstreamError) Error() string {
	var message strings.Builder
	message.WriteString("ChatGPT subscription upstream failed (")
	message.WriteString(e.Kind)
	if e.HTTPStatus != 0 {
		message.WriteString(", status ")
		message.WriteString(strconv.Itoa(e.HTTPStatus))
	}
	if e.Code != "" {
		message.WriteString(", code ")
		message.WriteString(e.Code)
	}
	message.WriteString(")")
	return message.String()
}

// unauthorizedError is an upstream 401, which the direct transport answers
// with the one-time credential recovery rather than a retry.
type unauthorizedError struct{ requestID string }

func (unauthorizedError) Error() string {
	return "ChatGPT subscription upstream rejected the credential"
}

// unauthorizedAfterRecovery is a direct-transport 401 that the call's one
// successful credential recovery did not cure, or one that arrived after
// answer text. It is transient (subscription_upstream_unavailable), as the
// app-server transport treats a 401 after a successful recovery: the sign-in
// itself was accepted, so the connection is not marked needs_refresh.
func unauthorizedAfterRecovery(unauthorized *unauthorizedError) *UpstreamError {
	return &UpstreamError{
		Kind:       upstreamKindUnauthorized,
		HTTPStatus: http.StatusUnauthorized,
		Code:       "unauthorized",
		RequestID:  unauthorized.requestID,
		Retryable:  true,
	}
}

var (
	upstreamCodePattern      = regexp.MustCompile(`^[a-z0-9_.-]{1,64}$`)
	upstreamRequestIDPattern = regexp.MustCompile(`^[A-Za-z0-9_.:-]{1,128}$`)
	tryAgainPattern          = regexp.MustCompile(`(?i)try again in\s*([0-9]+(?:\.[0-9]+)?)\s*(ms|milliseconds?|s|secs?|seconds?)\b`)

	knownUpstreamCodes = map[string]struct{}{
		"usage_limit_reached": {}, "rate_limit_exceeded": {}, "usage_not_included": {},
		"insufficient_quota": {}, "credit_balance_exhausted": {}, "context_length_exceeded": {},
		"cyber_policy": {}, "misalignment_policy_violation": {}, "invalid_prompt": {}, "bio_policy": {},
		"server_error": {}, "server_is_overloaded": {}, "slow_down": {}, "invalid_request_error": {},
		"invalid_value": {}, "unsupported_value": {}, "unsupported_parameter": {}, "missing_required_parameter": {},
		"model_not_found": {}, "not_found": {}, "unauthorized": {}, "forbidden": {}, "permission_denied": {},
		"max_output_tokens": {}, "content_filter": {}, "invalid_grant": {}, "refresh_token_expired": {},
		"refresh_token_reused": {}, "refresh_token_invalidated": {}, "invalid_client": {},
		"token_expired": {}, "invalid_api_key": {}, "service_unavailable": {}, "timeout": {}, "eof": {},
		"connection_reset": {}, "tls": {}, "dns": {}, "redirect": {}, "refresh_backoff": {},
		"invalid_store": {}, "invalid_stream": {}, "invalid_instructions": {},
	}
	usageLimitCodes   = map[string]struct{}{"usage_limit_reached": {}}
	throttleCodes     = map[string]struct{}{"rate_limit_exceeded": {}}
	quotaCodes        = map[string]struct{}{"insufficient_quota": {}, "credit_balance_exhausted": {}}
	policyCodes       = map[string]struct{}{"cyber_policy": {}, "misalignment_policy_violation": {}, "invalid_prompt": {}, "bio_policy": {}}
	retryableSSECodes = map[string]struct{}{"server_error": {}, "server_is_overloaded": {}, "slow_down": {}}
)

// sanitizeUpstreamCode keeps a code only when it is a known, well-formed
// upstream code; anything else becomes "other" so no free text is kept.
func sanitizeUpstreamCode(raw string) string {
	code := strings.ToLower(strings.TrimSpace(raw))
	if code == "" {
		return ""
	}
	if !upstreamCodePattern.MatchString(code) {
		return upstreamCodeOther
	}
	if _, known := knownUpstreamCodes[code]; !known {
		return upstreamCodeOther
	}
	return code
}

func sanitizeUpstreamRequestID(raw string) string {
	value := strings.TrimSpace(raw)
	if !upstreamRequestIDPattern.MatchString(value) {
		return ""
	}
	return value
}

func upstreamRequestID(headers http.Header) string {
	if id := sanitizeUpstreamRequestID(headers.Get("x-request-id")); id != "" {
		return id
	}
	return sanitizeUpstreamRequestID(headers.Get("x-oai-request-id"))
}

// upstreamFailure is the parsed, still-unsanitized shape of an upstream error
// body or SSE failure event. Message and Detail are used only for matching
// (the retry-after pattern, a service_tier mention, a fixed code for a
// known detail); they are never logged, returned, or stored.
type upstreamFailure struct {
	Code            string
	Type            string
	Param           string
	Message         string
	StatusCode      int
	ResetsAt        *float64
	ResetsInSeconds *float64
	// Detail is a FastAPI-style `detail` body, the shape the ChatGPT Codex
	// backend answers a rejected parameter with; DetailLocs holds the
	// field names a validation-error detail points at.
	Detail     string
	DetailLocs []string
}

// detailCodes maps the start of a known `detail` text to a fixed, sanitized
// code, so a rejected field stays diagnosable without keeping the text.
var detailCodes = []struct{ prefix, code string }{
	{"unsupported parameter", "unsupported_parameter"},
	{"unsupported value", "unsupported_value"},
	{"missing required parameter", "missing_required_parameter"},
	{"invalid value", "invalid_value"},
	{"store must be", "invalid_store"},
	{"stream must be", "invalid_stream"},
	{"instructions are", "invalid_instructions"},
}

// parseDetail reads a `detail` that is a string or a FastAPI list of
// {loc, msg, type} validation errors.
func parseDetail(raw json.RawMessage) (string, []string) {
	if len(raw) == 0 || string(raw) == "null" {
		return "", nil
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return text, nil
	}
	var items []struct {
		Loc []json.RawMessage `json:"loc"`
		Msg string            `json:"msg"`
	}
	if json.Unmarshal(raw, &items) != nil {
		return "", nil
	}
	var messages, locs []string
	for _, item := range items {
		if item.Msg != "" {
			messages = append(messages, item.Msg)
		}
		for _, element := range item.Loc {
			var name string
			if json.Unmarshal(element, &name) == nil && name != "" {
				locs = append(locs, name)
			}
		}
	}
	return strings.Join(messages, "\n"), locs
}

type rawUpstreamError struct {
	Code            json.RawMessage `json:"code"`
	Type            string          `json:"type"`
	Param           string          `json:"param"`
	Message         string          `json:"message"`
	ResetsAt        *float64        `json:"resets_at"`
	ResetsInSeconds *float64        `json:"resets_in_seconds"`
}

// parseUpstreamErrorBody reads `{"error":{...}}`, `{"error":"code"}`, a
// bare `{"code":...,"type":...,"param":...}` (also the shape of an SSE
// `error` event), or a FastAPI `{"detail":...}` without keeping anything but
// the fields the classifier needs.
func parseUpstreamErrorBody(body []byte) upstreamFailure {
	var envelope struct {
		Error      json.RawMessage `json:"error"`
		Code       json.RawMessage `json:"code"`
		Type       string          `json:"type"`
		Param      json.RawMessage `json:"param"`
		Message    string          `json:"message"`
		StatusCode int             `json:"status_code"`
		Detail     json.RawMessage `json:"detail"`
	}
	if json.Unmarshal(body, &envelope) != nil {
		return upstreamFailure{}
	}
	failure := upstreamFailure{Code: rawCodeString(envelope.Code), Type: envelope.Type, Param: rawCodeString(envelope.Param), Message: envelope.Message, StatusCode: envelope.StatusCode}
	failure.Detail, failure.DetailLocs = parseDetail(envelope.Detail)
	if len(envelope.Error) == 0 || string(envelope.Error) == "null" {
		return failure
	}
	var asString string
	if json.Unmarshal(envelope.Error, &asString) == nil {
		if failure.Code == "" {
			failure.Code = asString
		}
		return failure
	}
	var inner rawUpstreamError
	if json.Unmarshal(envelope.Error, &inner) != nil {
		return failure
	}
	if code := rawCodeString(inner.Code); code != "" {
		failure.Code = code
	}
	if inner.Type != "" {
		failure.Type = inner.Type
	}
	if inner.Message != "" {
		failure.Message = inner.Message
	}
	if inner.Param != "" {
		failure.Param = inner.Param
	}
	failure.ResetsAt = inner.ResetsAt
	failure.ResetsInSeconds = inner.ResetsInSeconds
	return failure
}

// rawCodeString accepts a code sent as a JSON string or number.
func rawCodeString(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	var text string
	if json.Unmarshal(raw, &text) == nil {
		return text
	}
	var number json.Number
	if json.Unmarshal(raw, &number) == nil {
		return number.String()
	}
	return ""
}

func (f upstreamFailure) names(set map[string]struct{}) bool {
	for _, value := range []string{f.Code, f.Type} {
		if _, ok := set[strings.ToLower(strings.TrimSpace(value))]; ok {
			return true
		}
	}
	return false
}

// mentionsServiceTier reports a failure that names service_tier: as the
// param (nested or top level), in the code or type, in a detail text or a
// detail loc, or in the message.
func (f upstreamFailure) mentionsServiceTier() bool {
	if strings.EqualFold(strings.TrimSpace(f.Param), "service_tier") {
		return true
	}
	for _, loc := range f.DetailLocs {
		if strings.EqualFold(strings.TrimSpace(loc), "service_tier") {
			return true
		}
	}
	for _, value := range []string{f.Code, f.Type, f.Detail, f.Message} {
		if strings.Contains(strings.ToLower(value), "service_tier") {
			return true
		}
	}
	return false
}

func (f upstreamFailure) sanitizedCode() string {
	if code := sanitizeUpstreamCode(f.Code); code != "" {
		return code
	}
	if code := sanitizeUpstreamCode(f.Type); code != "" {
		return code
	}
	return f.detailCode()
}

// detailCode is the fixed code for a known detail text, "other" for any
// other detail, and "" without one.
func (f upstreamFailure) detailCode() string {
	detail := strings.ToLower(strings.TrimSpace(f.Detail))
	if detail == "" {
		return ""
	}
	for _, known := range detailCodes {
		if strings.HasPrefix(detail, known.prefix) {
			return known.code
		}
	}
	return upstreamCodeOther
}

// classifyHTTPFailure maps a non-2xx, non-401 response to the broker's error
// types (§7). It never keeps the body or the upstream message.
func classifyHTTPFailure(status int, headers http.Header, body []byte, now time.Time) error {
	failure := parseUpstreamErrorBody(body)
	requestID := upstreamRequestID(headers)
	if status == http.StatusTooManyRequests {
		switch {
		case failure.names(usageLimitCodes):
			return usageLimitError(failure, headers, now)
		case failure.names(map[string]struct{}{"usage_not_included": {}}):
			return &UpstreamError{Kind: upstreamKindNotIncluded, HTTPStatus: status, Code: "usage_not_included", RequestID: requestID}
		default:
			return &RateLimitError{Kind: RateLimitThrottle, RetryAfter: throttleRetryAfter(failure, headers)}
		}
	}
	upstream := &UpstreamError{Kind: upstreamKindHTTPStatus, HTTPStatus: status, Code: failure.sanitizedCode(), RequestID: requestID}
	switch status {
	case http.StatusRequestTimeout, http.StatusInternalServerError, http.StatusBadGateway, http.StatusServiceUnavailable, http.StatusGatewayTimeout:
		upstream.Retryable = true
	case http.StatusBadRequest:
		upstream.tierRejected = failure.mentionsServiceTier()
	}
	if hint, ok := headerRetryHint(headers); ok {
		upstream.retryAfter, upstream.hasRetryAfter = hint, true
	}
	return upstream
}

// classifySSEFailure maps a response.failed or error event (§7).
func classifySSEFailure(failure upstreamFailure, headers http.Header, now time.Time) error {
	requestID := upstreamRequestID(headers)
	switch {
	case failure.StatusCode == http.StatusUnauthorized:
		return &unauthorizedError{requestID: requestID}
	case failure.names(usageLimitCodes):
		return usageLimitError(failure, headers, now)
	case failure.names(throttleCodes), failure.StatusCode == http.StatusTooManyRequests && failure.sanitizedCode() == "":
		return &RateLimitError{Kind: RateLimitThrottle, RetryAfter: throttleRetryAfter(failure, headers)}
	case failure.names(map[string]struct{}{"usage_not_included": {}}):
		return &UpstreamError{Kind: upstreamKindNotIncluded, Code: "usage_not_included", RequestID: requestID}
	case failure.names(quotaCodes):
		return &UpstreamError{Kind: upstreamKindQuota, Code: failure.sanitizedCode(), RequestID: requestID}
	case failure.names(map[string]struct{}{"context_length_exceeded": {}}):
		return &UpstreamError{Kind: upstreamKindContext, Code: "context_length_exceeded", RequestID: requestID}
	case failure.names(policyCodes):
		return &UpstreamError{Kind: upstreamKindPolicy, Code: failure.sanitizedCode(), RequestID: requestID}
	}
	upstream := &UpstreamError{
		Kind:         upstreamKindFailed,
		HTTPStatus:   failure.StatusCode,
		Code:         failure.sanitizedCode(),
		RequestID:    requestID,
		Retryable:    failure.names(retryableSSECodes) || failure.StatusCode >= 500,
		tierRejected: failure.mentionsServiceTier(),
	}
	return upstream
}

// usageLimitError is the usage-limit RateLimitError of a direct failure:
// its reset (usageReset), the exhausted window's length and the allowlisted
// x-codex-rate-limit-reached-type. Nothing else of the body or headers is
// kept.
func usageLimitError(failure upstreamFailure, headers http.Header, now time.Time) *RateLimitError {
	resetsAt, retryAfter, source := usageResetFrom(failure, headers, now)
	limited := &RateLimitError{
		Kind:             RateLimitUsage,
		RetryAfter:       retryAfter,
		ResetsAt:         resetsAt,
		LimitReachedType: allowedLimitReachedType(headers.Get("x-codex-rate-limit-reached-type")),
		upstreamCode:     "usage_limit_reached",
		resetSource:      source,
	}
	if window, ok := exhaustedHeaderWindow(headers, now); ok {
		limited.WindowMinutes = window.minutes
	}
	return limited
}

// usageReset reads when an exhausted usage window resets, from the first of:
// error.resets_at (unix seconds), error.resets_in_seconds, the exhausted
// window of the active x-<family>-primary-* / -secondary-* rate-limit headers
// (the later reset when both are exhausted), and Retry-After, which sets only
// the duration. A reset outside (now, now+7d] is not a source, whichever of
// the four named it. ok is false when nothing gave a reset: there is no
// made-up default.
func usageReset(failure upstreamFailure, headers http.Header, now time.Time) (time.Time, time.Duration, bool) {
	resetsAt, retryAfter, source := usageResetFrom(failure, headers, now)
	return resetsAt, retryAfter, source != "none"
}

// usageResetFrom is usageReset plus the name of the source it used, "none"
// without one.
func usageResetFrom(failure upstreamFailure, headers http.Header, now time.Time) (time.Time, time.Duration, string) {
	if failure.ResetsAt != nil {
		if resetsAt, ok := unixReset(*failure.ResetsAt, now); ok {
			return resetsAt, resetsAt.Sub(now), "resets_at"
		}
	}
	if failure.ResetsInSeconds != nil {
		seconds := *failure.ResetsInSeconds
		if seconds > 0 && seconds <= maxRetryAfter.Seconds() {
			in := secondsDuration(seconds)
			return now.Add(in), in, "resets_in_seconds"
		}
	}
	if window, ok := exhaustedHeaderWindow(headers, now); ok {
		return window.resetsAt, window.resetsAt.Sub(now), "headers"
	}
	if hint, ok := usageRetryAfterHint(headers.Get("Retry-After"), now); ok {
		return time.Time{}, clampRetryAfter(hint), "retry_after"
	}
	return time.Time{}, 0, "none"
}

// usageRetryAfterHint reads a usage limit's Retry-After (delta-seconds or an
// HTTP-date) only when it lies in (0, 7d]. A longer hint is dropped rather
// than clamped: a clamped hint would name a reset earlier than the real one,
// so it stays unknown, as a resets_at or resets_in_seconds beyond 7 days
// does. A throttle's hint (throttleRetryAfter) is still clamped, since there
// it is only a wait, never shown as a reset time.
func usageRetryAfterHint(raw string, now time.Time) (time.Duration, bool) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return 0, false
	}
	if seconds, err := strconv.ParseFloat(raw, 64); err == nil {
		if math.IsNaN(seconds) || seconds <= 0 || seconds > maxRetryAfter.Seconds() {
			return 0, false
		}
		return secondsDuration(seconds), true
	}
	if at, err := http.ParseTime(raw); err == nil {
		if until := at.Sub(now); until > 0 && until <= maxRetryAfter {
			return until, true
		}
	}
	return 0, false
}

// unixReset converts a unix-seconds reset inside (now, now+7d]; anything
// else, including a value too large for time.Time, is not a reset.
func unixReset(unixSeconds float64, now time.Time) (time.Time, bool) {
	if math.IsNaN(unixSeconds) || math.IsInf(unixSeconds, 0) {
		return time.Time{}, false
	}
	until := unixSeconds - (float64(now.Unix()) + float64(now.Nanosecond())/1e9)
	if until <= 0 || until > maxRetryAfter.Seconds() {
		return time.Time{}, false
	}
	whole := math.Floor(unixSeconds)
	resetsAt := time.Unix(int64(whole), int64((unixSeconds-whole)*1e9)).UTC()
	if !resetsAt.After(now) {
		return time.Time{}, false
	}
	return resetsAt, true
}

// headerWindow is one exhausted rate-limit window read from headers.
type headerWindow struct {
	resetsAt time.Time
	minutes  int
}

// exhaustedHeaderWindow reads the active limit's primary and secondary
// windows the way Codex's rate_limits.rs parse_rate_limit_for_limit does and
// returns the exhausted one (used-percent >= 100) with a reset inside (now,
// now+7d], the later reset when both are.
func exhaustedHeaderWindow(headers http.Header, now time.Time) (headerWindow, bool) {
	prefix := "x-" + rateLimitHeaderFamily(headers.Get("x-codex-active-limit"))
	var chosen headerWindow
	found := false
	for _, window := range []string{"primary", "secondary"} {
		used, err := strconv.ParseFloat(strings.TrimSpace(headers.Get(prefix+"-"+window+"-used-percent")), 64)
		if err != nil || math.IsNaN(used) || used < 100 {
			continue
		}
		reset, err := strconv.ParseFloat(strings.TrimSpace(headers.Get(prefix+"-"+window+"-reset-at")), 64)
		if err != nil {
			continue
		}
		resetsAt, ok := unixReset(reset, now)
		if !ok || (found && !resetsAt.After(chosen.resetsAt)) {
			continue
		}
		chosen, found = headerWindow{resetsAt: resetsAt}, true
		if minutes, err := strconv.Atoi(strings.TrimSpace(headers.Get(prefix + "-" + window + "-window-minutes"))); err == nil && minutes > 0 && minutes <= maxWindowMinutes {
			chosen.minutes = minutes
		}
	}
	return chosen, found
}

var rateLimitFamilyPattern = regexp.MustCompile(`^[a-z0-9-]{1,64}$`)

// rateLimitHeaderFamily is the x-codex-active-limit value lowercased with
// "_" turned into "-", defaulting to "codex" when absent or malformed.
func rateLimitHeaderFamily(activeLimit string) string {
	family := strings.ReplaceAll(strings.ToLower(strings.TrimSpace(activeLimit)), "_", "-")
	if !rateLimitFamilyPattern.MatchString(family) {
		return "codex"
	}
	return family
}

// throttleRetryAfter picks the first of retry-after-ms, Retry-After, the
// "try again in N" message pattern, else 1s.
func throttleRetryAfter(failure upstreamFailure, headers http.Header) time.Duration {
	if hint, ok := headerRetryHint(headers); ok {
		return clampRetryAfter(hint)
	}
	if match := tryAgainPattern.FindStringSubmatch(failure.Message); match != nil {
		if value, err := strconv.ParseFloat(match[1], 64); err == nil {
			unit := strings.ToLower(match[2])
			if strings.HasPrefix(unit, "ms") || strings.HasPrefix(unit, "milli") {
				return clampRetryAfter(millisecondsDuration(value))
			}
			return clampRetryAfter(secondsDuration(value))
		}
	}
	return defaultThrottleRetry
}

// headerRetryHint reads retry-after-ms, then Retry-After.
func headerRetryHint(headers http.Header) (time.Duration, bool) {
	if raw := strings.TrimSpace(headers.Get("retry-after-ms")); raw != "" {
		if ms, err := strconv.ParseFloat(raw, 64); err == nil && ms >= 0 {
			return millisecondsDuration(ms), true
		}
	}
	return parseRetryAfterHeader(headers.Get("Retry-After"), time.Now())
}

// parseRetryAfterHeader accepts delta-seconds or an HTTP-date.
func parseRetryAfterHeader(raw string, now time.Time) (time.Duration, bool) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return 0, false
	}
	if seconds, err := strconv.ParseFloat(raw, 64); err == nil && seconds >= 0 {
		return secondsDuration(seconds), true
	}
	if at, err := http.ParseTime(raw); err == nil {
		return max(at.Sub(now), 0), true
	}
	return 0, false
}

// secondsDuration converts a hint in seconds. Above the 7-day ceiling it is
// the ceiling, so a huge value can never overflow time.Duration into a
// negative delay.
func secondsDuration(seconds float64) time.Duration {
	if seconds > maxRetryAfter.Seconds() {
		return maxRetryAfter
	}
	if seconds < -maxRetryAfter.Seconds() {
		return -maxRetryAfter
	}
	return time.Duration(seconds * float64(time.Second))
}

// millisecondsDuration is secondsDuration for a hint in milliseconds.
func millisecondsDuration(ms float64) time.Duration {
	if ms > float64(maxRetryAfter.Milliseconds()) {
		return maxRetryAfter
	}
	if ms < -float64(maxRetryAfter.Milliseconds()) {
		return -maxRetryAfter
	}
	return time.Duration(ms * float64(time.Millisecond))
}

func clampRetryAfter(value time.Duration) time.Duration {
	return min(max(value, minRetryAfter), maxRetryAfter)
}

// CeilSeconds is the whole-second Retry-After value for a duration: rounded
// up, and never below 1.
func CeilSeconds(value time.Duration) int64 {
	seconds := int64(math.Ceil(value.Seconds()))
	return max(seconds, 1)
}
