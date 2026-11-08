package errs

import "encoding/json"

// Fetch sub-codes of DRIVER_FAILED (E3.8.1). This file mirrors quarry-core's
// error::driver_failed_reason: the same four strings and the same retryable
// flags. Change both or neither. The edge names a sub-code only when its
// QUARRY_EDGE__FETCH_SUB_CODES flag is on, so an ordinary DRIVER_FAILED has none.
//
// dns: the host name did not resolve. tls: the handshake or the certificate
// check failed. connect: no TCP connection. all_drivers_failed: every tier was
// tried and none served the page, and the tiers did not agree on one cause.
const (
	DriverFailedDNS              = "dns"
	DriverFailedTLS              = "tls"
	DriverFailedConnect          = "connect"
	DriverFailedAllDriversFailed = "all_drivers_failed"
)

// edgeDriverFailed is the edge's error code for a failed fetch waterfall.
const edgeDriverFailed = "DRIVER_FAILED"

// DriverFailedRetryable reports the retryable flag of a fetch sub-code. ok is
// false for a string outside the closed list. dns and connect can clear on a
// repeat; tls and all_drivers_failed do not.
func DriverFailedRetryable(reason string) (retryable bool, ok bool) {
	switch reason {
	case DriverFailedDNS, DriverFailedConnect:
		return true, true
	case DriverFailedTLS, DriverFailedAllDriversFailed:
		return false, true
	default:
		return false, false
	}
}

// driverFailedEnvelope is the part of an edge error envelope a sub-code needs.
type driverFailedEnvelope struct {
	Error struct {
		Code    string         `json:"code"`
		Details map[string]any `json:"details"`
	} `json:"error"`
}

// DriverFailedReasonOf reads the fetch sub-code of an edge error body. ok is
// false unless the body is a DRIVER_FAILED whose details.reason is one of the
// four sub-codes. A free-text reason is never returned, and a body that is not
// JSON is not one.
func DriverFailedReasonOf(body []byte) (reason string, ok bool) {
	var env driverFailedEnvelope
	if err := json.Unmarshal(body, &env); err != nil {
		return "", false
	}
	if env.Error.Code != edgeDriverFailed {
		return "", false
	}
	reason, _ = env.Error.Details["reason"].(string)
	if _, known := DriverFailedRetryable(reason); !known {
		return "", false
	}
	return reason, true
}
