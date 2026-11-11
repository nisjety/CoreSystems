package grant

import (
	"errors"
	"strconv"
	"strings"
)

// fill_otp approvals (Q3-E3.1-b1-c, ADR-2a). A fill_otp approval is one-time
// authority to read a one-time code from one mail connection and type it into
// one frame of one https origin. Both bindings are required for fill_otp and
// refused for every other action, so no other approval can carry either.
// Single use stays Quarry's job (used_sensitive_approval_grants). The values are
// stored verbatim and never logged.

// ActionFillOtp is the sensitive action of a fill_otp approval.
const ActionFillOtp = "fill_otp"

// maxMailConnectionIDBytes bounds a mail connection id.
const maxMailConnectionIDBytes = 128

var (
	// ErrFillOtpBindingInvalid means a fill_otp approval lacks a canonical https
	// grant_origin or a well-formed mail_connection_id.
	ErrFillOtpBindingInvalid = errors.New("a fill_otp approval requires a canonical https grant_origin and a mail_connection_id of 1 to 128 letters, digits, '_' or '-'")
	// ErrFillOtpBindingUnexpected means a grant_origin or mail_connection_id was
	// sent for anything other than a fill_otp approval.
	ErrFillOtpBindingUnexpected = errors.New("grant_origin and mail_connection_id are only for a fill_otp approval")
	// ErrFillOtpZdrRefused means a fill_otp approval was asked for under a Zero
	// Data Retention parent or request. Refused (conservative, dark): the binding
	// would keep an origin and a mail connection for a ZDR run.
	ErrFillOtpZdrRefused = errors.New("a fill_otp approval is refused for a Zero Data Retention run")
	// ErrFillOtpNotDurable means the durable store was asked for a fill_otp
	// approval. It has no columns for the binding yet, so a validated grant would
	// come back without it; refuse rather than drop the binding silently.
	ErrFillOtpNotDurable = errors.New("a fill_otp approval is not available until the durable store persists its binding")
)

// IsFillOtp reports whether a sensitive action list names fill_otp.
func IsFillOtp(actions []string) bool {
	for _, a := range actions {
		if strings.TrimSpace(a) == ActionFillOtp {
			return true
		}
	}
	return false
}

// checkFillOtpBinding validates the origin and mail connection of an approval
// whose canonical actions are given. For fill_otp both are required, the origin
// a canonical https origin whose host the parent may reach; for any other
// action both must be empty.
func checkFillOtpBinding(actions []string, origin, mailConnectionID string, parent *Grant) error {
	if !IsFillOtp(actions) {
		if origin != "" || mailConnectionID != "" {
			return ErrFillOtpBindingUnexpected
		}
		return nil
	}
	host, ok := canonicalHTTPSOriginHost(origin)
	if !ok || !ValidMailConnectionID(mailConnectionID) {
		return ErrFillOtpBindingInvalid
	}
	if parent != nil && !hostWithinGrant(host, parent) {
		return ErrDeriveWidensParent
	}
	return nil
}

// ValidMailConnectionID reports an id matching ^[A-Za-z0-9_-]{1,128}$.
func ValidMailConnectionID(id string) bool {
	if id == "" || len(id) > maxMailConnectionIDBytes {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if (c < 'a' || c > 'z') && (c < 'A' || c > 'Z') && (c < '0' || c > '9') && c != '_' && c != '-' {
			return false
		}
	}
	return true
}

// canonicalHTTPSOriginHost returns the host of a canonical https origin:
// exactly "https://" + a lowercase DNS host + an optional port (1-65535, no
// leading zero, never the default 443), with no user info, path, query or
// fragment. It is checked as sent, never rewritten.
func canonicalHTTPSOriginHost(origin string) (string, bool) {
	const scheme = "https://"
	if !strings.HasPrefix(origin, scheme) {
		return "", false
	}
	rest := origin[len(scheme):]
	if strings.ContainsAny(rest, "/?#@\\ ") {
		return "", false
	}
	host, port, hasPort := strings.Cut(rest, ":")
	if hasPort {
		if port == "" || port[0] == '0' || len(port) > 5 {
			return "", false
		}
		n, err := strconv.Atoi(port)
		if err != nil || n < 1 || n > 65535 || n == 443 || strconv.Itoa(n) != port {
			return "", false
		}
	}
	if !isCanonicalDomain(host) {
		return "", false
	}
	return host, true
}
