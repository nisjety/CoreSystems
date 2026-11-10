package actions

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/events"
	"github.com/triodelab/integration-corev2/internal/htmlstrip"
)

// mail.otp.read (E3.1-g, ADR-2a) is an executor-only read of ONE one-time
// code from the connected user's own mailbox (Outlook or Gmail). It exists so
// Quarry can complete an approved fill_otp without the model, the run
// transcript or any receipt ever seeing mail content:
//
//   - It returns only the code ({"found": true, "code": "123456"}) or
//     {"found": false}. Never a subject, sender, message id, snippet or body.
//   - It is executor-only. The caller must be a verified service principal
//     (auth-core plane token, audience ingestion, R-13 PR 1) holding the narrow
//     scope integration:otp:read, minted for the connection's own
//     organization. The shared IC internal key, an interactive user session and
//     a token without that scope are refused. Quarry is deliberately NOT a
//     member of INTEGRATION_TOKEN_LEASE_CONSUMERS: the lease list hands out raw
//     provider tokens, which this path must never do.
//   - Every attempt that reaches the caller check is audited through the
//     events publisher (type integration.mail.otp.read). The audit event never
//     carries the code or any mail content. A read that cannot be audited
//     fails closed: no publisher means no read, and a failed publish means the
//     code is not returned.
//   - It ships dark: INTEGRATION_OTP_READ must be on (default off).
//
// The HTTP route wiring lives in internal/api (executeConnectionAction fills
// ExecuteInput.Caller from the verified principal, applies the owner rule,
// and requiredCapabilityForOperation maps the operation to mail.read /
// gmail.read), in internal/auth (the per-route admission of
// integration:otp:read) and in cmd/api (SetOTPAuditPublisher(cfg.Events)).
//
// Params (all optional):
//
//	windowSeconds  how far back to look, default 600, min 30, max 1800
//	from           sender address to restrict to (a single address)
//	codeLength     exact number of digits expected, 4 to 10
//	approvalId     the per-use fill_otp approval id, recorded in the audit
//	               event only (never used for authorization here)
//	acting_user_id the run's acting user (the fill_otp approval's actor). The
//	               HTTP layer compares it with the personal connection's owner
//	               (otp_owner_mismatch on a difference) before this service
//	               runs; here it is only shape-checked and never audited or
//	               logged.

const (
	// OperationMailOTPRead is the canonical operation string.
	OperationMailOTPRead = "mail.otp.read"
	// ScopeIntegrationOTPRead is the only scope that admits a caller to
	// mail.otp.read (registry row quarry-edge, audience ingestion).
	ScopeIntegrationOTPRead = "integration:otp:read"
	// OTPActingUserIDParam is the params key that names the acting user of the
	// run the approval belongs to. The HTTP layer binds it to the personal
	// connection's owner (decision D-E31g-5).
	OTPActingUserIDParam = "acting_user_id"
	// OTPReadFlagEnv turns the operation on. Default off.
	OTPReadFlagEnv = "INTEGRATION_OTP_READ"
	// OTPReadAuditEventType is the events-publisher type for every attempt.
	OTPReadAuditEventType = "integration.mail.otp.read"

	otpIntegrationReadScope  = "integration:read"
	otpIntegrationWriteScope = "integration:write"

	otpDefaultWindow   = 10 * time.Minute
	otpMinWindow       = 30 * time.Second
	otpMaxWindow       = 30 * time.Minute
	otpMaxMessages     = 5
	otpMaxScanRunes    = 64 << 10
	otpKeywordLookback = 48
	otpMaxApprovalID   = 128
	otpMaxActingUserID = 200
)

var (
	// ErrOTPReadDisabled is returned while INTEGRATION_OTP_READ is off.
	ErrOTPReadDisabled = errors.New("mail.otp.read is disabled")
	// ErrOTPCallerNotAllowed is returned for any caller that is not a verified
	// service principal with integration:otp:read for the connection's org.
	ErrOTPCallerNotAllowed = errors.New("mail.otp.read is executor-only: a service token with integration:otp:read for this organization is required")
	// ErrOTPAuditUnavailable is returned when no events publisher is wired;
	// the read is refused before the mailbox is touched.
	ErrOTPAuditUnavailable = errors.New("mail.otp.read cannot be audited: no events publisher is configured")
	// ErrOTPAuditFailed is returned when the audit event could not be
	// published after the read; the code is withheld.
	ErrOTPAuditFailed = errors.New("mail.otp.read audit event could not be published; the code is withheld")
	// ErrOTPScopeOperationRefused is returned when a caller that holds only
	// integration:otp:read asks for any other operation.
	ErrOTPScopeOperationRefused = errors.New("a token scoped to integration:otp:read may only call mail.otp.read")
	// ErrOTPProviderUnsupported is returned for a provider without a mailbox.
	ErrOTPProviderUnsupported = errors.New("mail.otp.read supports only microsoft and google mail connections")
)

// CallerIdentity is the verified identity of the caller of an action, filled
// by the HTTP layer from the request's authenticated principal. The zero value
// means "not provided"; every pre-existing operation ignores it, and
// mail.otp.read refuses it.
type CallerIdentity struct {
	// PrincipalType is "service" for an auth-core plane token, "user" for an
	// interactive session.
	PrincipalType string
	// ServiceID is the verified token subject (service id for a service
	// principal). Recorded in the audit event.
	ServiceID string
	// OrganizationID is the organization the token was minted for.
	OrganizationID string
	// Scopes are the verified token scopes.
	Scopes []string
	// InternalKey is true when the request authenticated with the shared IC
	// internal API key instead of a verified bearer token.
	InternalKey bool
}

func (c CallerIdentity) hasScope(scope string) bool {
	for _, granted := range c.Scopes {
		if strings.TrimSpace(granted) == scope {
			return true
		}
	}
	return false
}

// otpOnly reports whether the caller holds integration:otp:read and no
// general integration scope, which limits it to mail.otp.read.
func (c CallerIdentity) otpOnly() bool {
	return c.hasScope(ScopeIntegrationOTPRead) &&
		!c.hasScope(otpIntegrationReadScope) &&
		!c.hasScope(otpIntegrationWriteScope)
}

// isMailOTPReadOperation matches the canonical operation and its provider
// aliases. Matching is case-insensitive so a mixed-case spelling can never
// slip past the executor-only gate into another switch.
func isMailOTPReadOperation(operation string) bool {
	switch strings.ToLower(strings.TrimSpace(operation)) {
	case OperationMailOTPRead, "microsoft." + OperationMailOTPRead, "google." + OperationMailOTPRead:
		return true
	}
	return false
}

// IsMailOTPReadOperation reports whether operation names mail.otp.read (or a
// provider alias of it), for the HTTP layer's owner rule and capability map.
func IsMailOTPReadOperation(operation string) bool {
	return isMailOTPReadOperation(operation)
}

// refuseOTPOnlyCaller keeps a token scoped to integration:otp:read off every
// other operation, independent of the HTTP layer's own admission rule.
func refuseOTPOnlyCaller(caller CallerIdentity, operation string) error {
	if caller.otpOnly() && !isMailOTPReadOperation(operation) {
		return ErrOTPScopeOperationRefused
	}
	return nil
}

// otpReadFlagFromEnv reads INTEGRATION_OTP_READ. Only an explicit true value
// turns the operation on.
func otpReadFlagFromEnv() bool {
	enabled, err := strconv.ParseBool(strings.TrimSpace(os.Getenv(OTPReadFlagEnv)))
	return err == nil && enabled
}

// SetOTPReadEnabled overrides the INTEGRATION_OTP_READ flag (config wiring
// and tests).
func (s *Service) SetOTPReadEnabled(enabled bool) {
	s.otpReadEnabled = enabled
}

// SetOTPAuditPublisher wires the events publisher mail.otp.read audits
// through. Without one the operation refuses every call.
func (s *Service) SetOTPAuditPublisher(publisher events.Publisher) {
	s.otpAudit = publisher
}

type otpReadParams struct {
	window     time.Duration
	from       string
	codeLength int
	approvalID string
}

var otpAllowedParams = map[string]struct{}{
	"windowSeconds": {},
	"from":          {},
	"codeLength":    {},
	"approvalId":    {},
	// OTPActingUserIDParam is checked by the HTTP layer; accepted here so the
	// strict allow-list does not refuse it.
	OTPActingUserIDParam: {},
}

// otpSenderAddress is a single mailbox address; anything else (a display
// name, a query operator, a quote) is refused rather than composed into a
// provider query.
var otpSenderAddress = regexp.MustCompile(`^[A-Za-z0-9._%+'-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}$`)

var otpApprovalID = regexp.MustCompile(`^[A-Za-z0-9._:-]+$`)

func parseOTPReadParams(params map[string]any) (otpReadParams, error) {
	parsed := otpReadParams{window: otpDefaultWindow}
	for key := range params {
		if _, ok := otpAllowedParams[key]; !ok {
			return otpReadParams{}, fmt.Errorf("mail.otp.read does not accept param %q", key)
		}
	}
	if raw, ok := params["windowSeconds"]; ok && raw != nil {
		seconds, err := otpIntParam(raw)
		if err != nil {
			return otpReadParams{}, fmt.Errorf("windowSeconds: %w", err)
		}
		window := time.Duration(seconds) * time.Second
		if window < otpMinWindow || window > otpMaxWindow {
			return otpReadParams{}, fmt.Errorf("windowSeconds must be between %d and %d", int(otpMinWindow/time.Second), int(otpMaxWindow/time.Second))
		}
		parsed.window = window
	}
	if raw, ok := params["from"]; ok && raw != nil {
		from := strings.TrimSpace(stringFromAny(raw))
		if from != "" {
			if len(from) > 254 || !otpSenderAddress.MatchString(from) {
				return otpReadParams{}, fmt.Errorf("from must be a single email address")
			}
			parsed.from = from
		}
	}
	if raw, ok := params["codeLength"]; ok && raw != nil {
		length, err := otpIntParam(raw)
		if err != nil {
			return otpReadParams{}, fmt.Errorf("codeLength: %w", err)
		}
		if length < 4 || length > 10 {
			return otpReadParams{}, fmt.Errorf("codeLength must be between 4 and 10")
		}
		parsed.codeLength = length
	}
	if raw, ok := params[OTPActingUserIDParam]; ok && raw != nil {
		acting, isString := raw.(string)
		if !isString || len(strings.TrimSpace(acting)) > otpMaxActingUserID {
			return otpReadParams{}, fmt.Errorf("%s is malformed", OTPActingUserIDParam)
		}
	}
	if raw, ok := params["approvalId"]; ok && raw != nil {
		approvalID := strings.TrimSpace(stringFromAny(raw))
		if approvalID != "" {
			if len(approvalID) > otpMaxApprovalID || !otpApprovalID.MatchString(approvalID) {
				return otpReadParams{}, fmt.Errorf("approvalId is malformed")
			}
			parsed.approvalID = approvalID
		}
	}
	return parsed, nil
}

func otpIntParam(raw any) (int, error) {
	switch value := raw.(type) {
	case float64:
		if value != float64(int(value)) {
			return 0, fmt.Errorf("must be a whole number")
		}
		return int(value), nil
	case int:
		return value, nil
	case int64:
		return int(value), nil
	case string:
		parsed, err := strconv.Atoi(strings.TrimSpace(value))
		if err != nil {
			return 0, fmt.Errorf("must be a whole number")
		}
		return parsed, nil
	default:
		return 0, fmt.Errorf("must be a whole number")
	}
}

// authorizeOTPCaller is the executor-only gate. It never consults the
// connection's own user: the read is authorized by the verified service token
// (and, upstream, by the single-use fill_otp approval Quarry enforces).
func authorizeOTPCaller(caller CallerIdentity, organizationID string) error {
	if caller.InternalKey {
		return ErrOTPCallerNotAllowed
	}
	if strings.TrimSpace(caller.PrincipalType) != "service" || strings.TrimSpace(caller.ServiceID) == "" {
		return ErrOTPCallerNotAllowed
	}
	if !caller.hasScope(ScopeIntegrationOTPRead) {
		return ErrOTPCallerNotAllowed
	}
	org := strings.TrimSpace(organizationID)
	if org == "" || strings.TrimSpace(caller.OrganizationID) != org {
		return ErrOTPCallerNotAllowed
	}
	return nil
}

// executeMailOTPRead runs mail.otp.read end to end: flag, caller, audit
// precondition, params, provider read, code extraction, audit, result.
func (s *Service) executeMailOTPRead(ctx context.Context, input ExecuteInput, operation string) (any, error) {
	if !s.otpReadEnabled {
		return nil, ErrOTPReadDisabled
	}
	providerKey := input.Connection.ProviderKey
	lowered := strings.ToLower(strings.TrimSpace(operation))
	if (strings.HasPrefix(lowered, "microsoft.") && providerKey != "microsoft") ||
		(strings.HasPrefix(lowered, "google.") && providerKey != "google") {
		return nil, ErrOTPProviderUnsupported
	}
	if s.otpAudit == nil {
		return nil, ErrOTPAuditUnavailable
	}
	if err := authorizeOTPCaller(input.Caller, input.Connection.OrganizationID); err != nil {
		// Best effort: a refusal is worth a trace, but it never changes the
		// answer.
		_ = s.publishOTPAudit(ctx, input, "refused", otpReadParams{}, 0)
		return nil, err
	}
	params, err := parseOTPReadParams(input.Params)
	if err != nil {
		return nil, err
	}

	since := s.now().UTC().Add(-params.window)
	var texts []string
	switch providerKey {
	case "microsoft":
		texts, err = s.microsoftOTPCandidateTexts(ctx, input.AccessToken, since, params)
	case "google":
		texts, err = s.googleOTPCandidateTexts(ctx, input.AccessToken, since, params)
	default:
		return nil, ErrOTPProviderUnsupported
	}
	if err != nil {
		_ = s.publishOTPAudit(ctx, input, "provider_error", params, 0)
		return nil, err
	}

	code := ""
	for _, text := range texts {
		if candidate := extractOTPCode(text, params.codeLength); candidate != "" {
			code = candidate
			break
		}
	}
	outcome := "not_found"
	if code != "" {
		outcome = "found"
	}
	if err := s.publishOTPAudit(ctx, input, outcome, params, len(texts)); err != nil {
		return nil, ErrOTPAuditFailed
	}
	if code == "" {
		return map[string]any{"found": false}, nil
	}
	return map[string]any{"found": true, "code": code}, nil
}

// publishOTPAudit emits the audit event. It carries who asked, for which
// connection, under which approval, and the outcome. Never the code, the
// sender, the subject or any other mail content.
func (s *Service) publishOTPAudit(ctx context.Context, input ExecuteInput, outcome string, params otpReadParams, scanned int) error {
	if s.otpAudit == nil {
		return ErrOTPAuditUnavailable
	}
	data := map[string]any{
		"operation":       OperationMailOTPRead,
		"outcome":         outcome,
		"callerService":   strings.TrimSpace(input.Caller.ServiceID),
		"callerType":      strings.TrimSpace(input.Caller.PrincipalType),
		"messagesScanned": scanned,
	}
	if params.window > 0 {
		data["windowSeconds"] = int(params.window / time.Second)
	}
	if params.approvalID != "" {
		data["approvalId"] = params.approvalID
	}
	if params.from != "" {
		// Whether a sender filter was used, not which sender.
		data["senderFiltered"] = true
	}
	return s.otpAudit.Publish(ctx, events.Event{
		Type:           OTPReadAuditEventType,
		OrganizationID: input.Connection.OrganizationID,
		WorkspaceID:    input.Connection.WorkspaceID,
		UserID:         input.Connection.UserID,
		ConnectionID:   input.Connection.ID,
		ProviderKey:    input.Connection.ProviderKey,
		Data:           data,
		CreatedAt:      s.now().UTC(),
	})
}

// microsoftOTPCandidateTexts lists the newest inbox messages received since
// `since` (Graph, text bodies) and returns subject + body per message, newest
// first. receivedDateTime is the first $filter clause because Graph requires
// the $orderby property to lead the $filter.
func (s *Service) microsoftOTPCandidateTexts(ctx context.Context, token string, since time.Time, params otpReadParams) ([]string, error) {
	base := strings.TrimRight(s.cfg.MicrosoftGraphBaseURL, "/") + "/v1.0"
	filter := "receivedDateTime ge " + since.Format(time.RFC3339)
	if params.from != "" {
		filter += " and from/emailAddress/address eq '" + odataEscape(params.from) + "'"
	}
	values := url.Values{}
	values.Set("$top", strconv.Itoa(otpMaxMessages))
	values.Set("$orderby", "receivedDateTime desc")
	values.Set("$filter", filter)
	values.Set("$select", "receivedDateTime,subject,body")
	endpoint := base + "/me/mailFolders/inbox/messages?" + values.Encode()
	raw, err := s.getBearerMap(ctx, token, endpoint, map[string]string{
		"Prefer": `outlook.body-content-type="text"`,
	})
	if err != nil {
		return nil, err
	}
	items, _ := raw["value"].([]any)
	texts := make([]string, 0, len(items))
	for _, item := range items {
		message, ok := item.(map[string]any)
		if !ok {
			continue
		}
		body := ""
		if bodyField, ok := message["body"].(map[string]any); ok {
			body = stringFromAny(bodyField["content"])
			if strings.EqualFold(stringFromAny(bodyField["contentType"]), "html") {
				body = htmlstrip.StripHTMLTags(body)
			}
		}
		texts = append(texts, stringFromAny(message["subject"])+"\n"+body)
	}
	return texts, nil
}

// googleOTPCandidateTexts lists Gmail messages received after `since`
// (epoch-second `after:`), skipping sent mail and drafts, then fetches each
// (format=full, bounded by otpMaxMessages) and returns subject + body, newest
// first. internalDate is re-checked because `after:` is not exact.
func (s *Service) googleOTPCandidateTexts(ctx context.Context, token string, since time.Time, params otpReadParams) ([]string, error) {
	base := strings.TrimRight(s.cfg.GoogleAPIBaseURL, "/")
	query := "after:" + strconv.FormatInt(since.Unix(), 10) + " -in:sent -in:drafts"
	if params.from != "" {
		query += " from:" + params.from
	}
	values := url.Values{}
	values.Set("maxResults", strconv.Itoa(otpMaxMessages))
	values.Set("q", query)
	list, err := s.getBearerMap(ctx, token, base+"/gmail/v1/users/me/messages?"+values.Encode(), nil)
	if err != nil {
		return nil, err
	}
	listed, _ := list["messages"].([]any)
	texts := make([]string, 0, len(listed))
	sinceMillis := since.UnixMilli()
	for index, item := range listed {
		if index >= otpMaxMessages {
			break
		}
		entry, ok := item.(map[string]any)
		if !ok {
			continue
		}
		id := strings.TrimSpace(stringFromAny(entry["id"]))
		if id == "" {
			continue
		}
		message, err := s.getBearerMap(ctx, token, base+"/gmail/v1/users/me/messages/"+url.PathEscape(id)+"?format=full", nil)
		if err != nil {
			return nil, err
		}
		if internal, err := strconv.ParseInt(stringFromAny(message["internalDate"]), 10, 64); err == nil && internal < sinceMillis {
			continue
		}
		payload, _ := message["payload"].(map[string]any)
		headers := gmailHeaderMap(payload)
		text, html := extractGmailMessageBodies(payload)
		if text == "" && html != "" {
			text = htmlstrip.StripHTMLTags(html)
		}
		texts = append(texts, headers["subject"]+"\n"+text)
	}
	return texts, nil
}

// otpKeyword marks text that introduces a one-time code (English and
// Norwegian wording).
var otpKeyword = regexp.MustCompile(`(?i)(code|otp|passcode|pass code|pin|verification|verify|security|one[- ]time|login|sign[- ]in|kode|engangskode|bekreft|verifiser)`)

// otpCandidate is a run of 4 to 10 digits, or two groups of three digits
// separated by a space or hyphen ("123 456", "123-456"), standing alone.
var otpCandidate = regexp.MustCompile(`\b([0-9]{3}[ -][0-9]{3}|[0-9]{4,10})\b`)

// extractOTPCode returns the most likely one-time code in text, or "".
// Preference: the first candidate with an OTP keyword shortly before it;
// otherwise the only distinct six-digit candidate in the text. Anything more
// ambiguous returns "" (a wrong code typed into a form is worse than asking
// the user to type it).
func extractOTPCode(text string, codeLength int) string {
	if text == "" {
		return ""
	}
	if runes := []rune(text); len(runes) > otpMaxScanRunes {
		text = string(runes[:otpMaxScanRunes])
	}
	matches := otpCandidate.FindAllStringSubmatchIndex(text, -1)
	sixDigit := map[string]struct{}{}
	for _, match := range matches {
		start, end := match[2], match[3]
		digits := strings.NewReplacer(" ", "", "-", "").Replace(text[start:end])
		if !otpLengthAccepted(digits, codeLength) {
			continue
		}
		lookbackStart := start - otpKeywordLookback
		if lookbackStart < 0 {
			lookbackStart = 0
		}
		if otpKeyword.MatchString(text[lookbackStart:start]) {
			return digits
		}
		if len(digits) == 6 {
			sixDigit[digits] = struct{}{}
		}
	}
	if codeLength == 0 || codeLength == 6 {
		if len(sixDigit) == 1 {
			for digits := range sixDigit {
				return digits
			}
		}
	}
	return ""
}

func otpLengthAccepted(digits string, codeLength int) bool {
	if codeLength > 0 {
		return len(digits) == codeLength
	}
	if len(digits) < 4 || len(digits) > 8 {
		return false
	}
	// A bare four-digit year is not a code.
	if len(digits) == 4 && (strings.HasPrefix(digits, "19") || strings.HasPrefix(digits, "20")) {
		return false
	}
	return true
}
