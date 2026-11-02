package activities

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"

	"go.temporal.io/sdk/temporal"
)

// maxControlRefusalBody bounds how much of a refusal body is read to find its
// code; the body is never logged or stored.
const maxControlRefusalBody = 4 << 10

// ControlRefusedError is Control answering NO to a service-to-service decision
// request: a 4xx that retrying cannot change (a typed Space refusal such as
// space_entitlement_off, a membership miss, a malformed intent). It is distinct
// from Control being unable to ANSWER (a 503, a 5xx, a dead socket), which a
// Temporal activity must keep retrying.
type ControlRefusedError struct {
	Operation string
	Status    int
	// Code is the typed denial code from the response body, when it carried one.
	Code string
}

func (e *ControlRefusedError) Error() string {
	if e.Code != "" {
		return fmt.Sprintf("%s rejected: status %d (%s)", e.Operation, e.Status, e.Code)
	}
	return fmt.Sprintf("%s rejected: status %d", e.Operation, e.Status)
}

// controlRefusalStatus reports whether a non-200 Control status is a final
// refusal. 401 is the service credential (it can be rotated into place) and
// 408, 425 and 429 are transient by definition, so those retry like a 5xx.
func controlRefusalStatus(status int) bool {
	if status < 400 || status >= 500 {
		return false
	}
	switch status {
	case http.StatusUnauthorized, http.StatusRequestTimeout, http.StatusTooEarly, http.StatusTooManyRequests:
		return false
	}
	return true
}

// controlRejection classifies a non-200 Control response: a final refusal is a
// *ControlRefusedError, anything else keeps the plain error the callers always
// returned.
func controlRejection(operation string, response *http.Response) error {
	if !controlRefusalStatus(response.StatusCode) {
		return fmt.Errorf("%s rejected: status %d", operation, response.StatusCode)
	}
	refused := &ControlRefusedError{Operation: operation, Status: response.StatusCode}
	var body struct {
		Code string `json:"code"`
	}
	if raw, err := io.ReadAll(io.LimitReader(response.Body, maxControlRefusalBody)); err == nil && json.Unmarshal(raw, &body) == nil {
		refused.Code = body.Code
	}
	return refused
}

// nonRetryableIfRefused turns a Control refusal anywhere in err's chain into a
// non-retryable Temporal application error, so the activity's retry policy does
// not ask Control the same question again. Every other error, including a
// Control outage, is returned unchanged and keeps retrying.
func nonRetryableIfRefused(err error) error {
	var refused *ControlRefusedError
	if err == nil || !errors.As(err, &refused) {
		return err
	}
	return temporal.NewNonRetryableApplicationError(err.Error(), "ControlRefused", err)
}
