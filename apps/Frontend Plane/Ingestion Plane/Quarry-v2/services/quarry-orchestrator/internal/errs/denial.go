package errs

import (
	"encoding/json"
	"errors"

	"go.temporal.io/sdk/temporal"
)

// edgeSecurityBlocked is the edge's error code for a refusal by the web policy,
// the crawl scope or robots.txt (quarry-core ErrorCode::SecurityBlocked).
const edgeSecurityBlocked = "SECURITY_BLOCKED"

// allCategories lists every category, so a Temporal error type can be mapped back.
var allCategories = []Category{
	CategoryNetwork,
	CategoryTimeout,
	CategoryServer5xx,
	CategoryClient4xx,
	CategoryValidation,
	CategoryBlocklist,
	CategoryAuth,
}

// FromHTTPResponse classifies an edge response and reads the denial the edge
// attaches to a refusal (WS14.S3). A 403 whose envelope says SECURITY_BLOCKED and
// carries details.denial is a blocklist refusal: non-retryable, with the denial
// kept on the error. Anything else is classified by FromHTTPStatus, unchanged.
func FromHTTPResponse(op string, status int, body []byte) *Error {
	if status == 403 {
		if denial, ok := denialFromBody(body); ok {
			e := FromHTTPStatus(op, status, body)
			e.Category = CategoryBlocklist
			e.Denial = denial
			return e
		}
	}
	return FromHTTPStatus(op, status, body)
}

// edgeEnvelope is the part of the edge's error envelope a denial needs.
type edgeEnvelope struct {
	Error struct {
		Code    string `json:"code"`
		Details struct {
			Denial map[string]any `json:"denial"`
		} `json:"details"`
	} `json:"error"`
}

// denialFromBody reads details.denial from an edge error envelope. A denial
// without a code is not a denial.
func denialFromBody(body []byte) (map[string]any, bool) {
	var env edgeEnvelope
	if err := json.Unmarshal(body, &env); err != nil {
		return nil, false
	}
	if env.Error.Code != edgeSecurityBlocked {
		return nil, false
	}
	denial := env.Error.Details.Denial
	code, _ := denial["code"].(string)
	if code == "" {
		return nil, false
	}
	return denial, true
}

// details is the payload of the Temporal application error. A denial's own
// fields (code, message, remedy, path, host, user_agent) ride along, so the
// workflow can read them back with DenialOf.
func (e *Error) details() map[string]any {
	d := map[string]any{"status": e.Status, "op": e.Op}
	for k, v := range e.Denial {
		d[k] = v
	}
	return d
}

// CategoryOf classifies an error a workflow received from an activity (WS14.S3).
// It reads the orchestrator's own *Error when it is still there, otherwise the
// Temporal application error's type, which Temporal() sets to the category name.
// Anything else is CategoryUnknown.
func CategoryOf(err error) Category {
	if e, ok := Classify(err); ok {
		return e.Category
	}
	var appErr *temporal.ApplicationError
	if errors.As(err, &appErr) {
		for _, c := range allCategories {
			if c.String() == appErr.Type() {
				return c
			}
		}
	}
	return CategoryUnknown
}

// DenialOf returns the denial an activity error carries (WS14.S3): the edge's
// refusal for a page, with its code, message and remedy. False when the error
// is not a denial.
func DenialOf(err error) (map[string]any, bool) {
	if e, ok := Classify(err); ok {
		return e.Denial, e.Denial != nil
	}
	var appErr *temporal.ApplicationError
	if !errors.As(err, &appErr) || appErr.Type() != CategoryBlocklist.String() {
		return nil, false
	}
	var details map[string]any
	if appErr.Details(&details) != nil {
		return nil, false
	}
	code, _ := details["code"].(string)
	return details, code != ""
}
