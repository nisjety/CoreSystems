package quarrycontracts

// RESTEnvelope mirrors CONTRACTS §2.
type RESTEnvelope[T any] struct {
	Data  *T             `json:"data"`
	Meta  EnvelopeMeta   `json:"meta"`
	Error *ErrorEnvelope `json:"error"`
}

type EnvelopeMeta struct {
	RequestID string    `json:"request_id"`
	Page      *PageMeta `json:"page,omitempty"`
}

type PageMeta struct {
	Cursor *string `json:"cursor,omitempty"`
	Next   *string `json:"next,omitempty"`
	Limit  uint32  `json:"limit"`
}

type ErrorEnvelope struct {
	Code      ErrorCode      `json:"code"`
	Message   string         `json:"message"`
	Details   map[string]any `json:"details,omitempty"`
	Retryable bool           `json:"retryable"`
}

type ErrorCode string

const (
	CodeBadRequest       ErrorCode = "BAD_REQUEST"
	CodeUnauthorized     ErrorCode = "UNAUTHORIZED"
	CodeForbidden        ErrorCode = "FORBIDDEN"
	CodeNotFound         ErrorCode = "NOT_FOUND"
	CodeConflict         ErrorCode = "CONFLICT"
	CodeRateLimited      ErrorCode = "RATE_LIMITED"
	CodeTimeout          ErrorCode = "TIMEOUT"
	CodeSecurityBlocked  ErrorCode = "SECURITY_BLOCKED"
	CodeDriverFailed     ErrorCode = "DRIVER_FAILED"
	CodeUpstreamBlocked  ErrorCode = "UPSTREAM_BLOCKED"
	CodeInternal         ErrorCode = "INTERNAL"
	// CodeUnsupported mirrors Rust's `ErrorCode::Unsupported`
	// (501 Not Implemented). Reserved for routes that are wired
	// but require a runtime dependency that isn't configured
	// (Temporal client, GPU driver, etc.) — never a silent
	// 200-OK lie.
	CodeUnsupported      ErrorCode = "UNSUPPORTED"
)

func (c ErrorCode) HTTPStatus() int {
	switch c {
	case CodeBadRequest:
		return 400
	case CodeUnauthorized:
		return 401
	case CodeForbidden, CodeSecurityBlocked:
		return 403
	case CodeNotFound:
		return 404
	case CodeConflict:
		return 409
	case CodeRateLimited:
		return 429
	case CodeTimeout:
		return 504
	case CodeDriverFailed, CodeUpstreamBlocked:
		return 502
	case CodeUnsupported:
		return 501
	default:
		return 500
	}
}

func OK[T any](requestID string, data T) RESTEnvelope[T] {
	return RESTEnvelope[T]{Data: &data, Meta: EnvelopeMeta{RequestID: requestID}}
}

func Err(requestID string, err ErrorEnvelope) RESTEnvelope[any] {
	return RESTEnvelope[any]{Meta: EnvelopeMeta{RequestID: requestID}, Error: &err}
}
