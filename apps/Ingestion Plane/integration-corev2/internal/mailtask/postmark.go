package mailtask

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"unicode"
)

// postmarkInboundPayload mirrors Postmark's inbound webhook JSON contract.
// See https://postmarkapp.com/support/article/1197-parse-an-inbound-email.
// Only the fields this receiver actually uses are declared; everything else
// in the real payload is ignored by json.Unmarshal.
type postmarkInboundPayload struct {
	MessageID string `json:"MessageID"`
	From      string `json:"From"`
	FromFull  struct {
		Email string `json:"Email"`
		Name  string `json:"Name"`
	} `json:"FromFull"`
	OriginalRecipient string           `json:"OriginalRecipient"`
	To                string           `json:"To"`
	Subject           string           `json:"Subject"`
	TextBody          string           `json:"TextBody"`
	HtmlBody          string           `json:"HtmlBody"`
	Headers           []postmarkHeader `json:"Headers"`
	Attachments       []struct {
		Name        string `json:"Name"`
		Content     string `json:"Content"` // base64
		ContentType string `json:"ContentType"`
	} `json:"Attachments"`
}

// postmarkHeader is one entry of the payload's Headers array: the message's
// own headers plus the ones Postmark adds on receipt.
type postmarkHeader struct {
	Name  string `json:"Name"`
	Value string `json:"Value"`
}

// maxInboundPayloadBytes caps the raw webhook body BEFORE any parsing, so a
// malformed or hostile MIME message cannot drive memory exhaustion in an
// endpoint that is, by construction, reachable by anyone who has an alias
// (spec §6, "large or malformed MIME").
const maxInboundPayloadBytes = 15 * 1024 * 1024 // 15MB: generous for a handful of small attachments, bounded

// ErrPayloadTooLarge is returned by ParsePostmarkInboundPayload when the raw
// body exceeds maxInboundPayloadBytes.
var ErrPayloadTooLarge = fmt.Errorf("mail task inbound payload exceeds %d bytes", maxInboundPayloadBytes)

// ParsePostmarkInboundPayload parses Postmark's inbound-webhook JSON body
// into the common InboundMailTaskEvent shape.
func ParsePostmarkInboundPayload(body []byte) (InboundMailTaskEvent, error) {
	if len(body) > maxInboundPayloadBytes {
		return InboundMailTaskEvent{}, ErrPayloadTooLarge
	}
	var raw postmarkInboundPayload
	if err := json.Unmarshal(body, &raw); err != nil {
		return InboundMailTaskEvent{}, fmt.Errorf("decode postmark inbound payload: %w", err)
	}

	recipient := strings.TrimSpace(raw.OriginalRecipient)
	if recipient == "" {
		recipient = strings.TrimSpace(raw.To)
	}
	alias, domain := splitAliasAndDomain(recipient)

	fromEmail := strings.TrimSpace(raw.FromFull.Email)
	if fromEmail == "" {
		fromEmail = strings.TrimSpace(raw.From)
	}

	attachments := make([]InboundAttachment, 0, len(raw.Attachments))
	for _, attachment := range raw.Attachments {
		content, err := base64.StdEncoding.DecodeString(attachment.Content)
		if err != nil {
			// A malformed attachment must not fail the whole message (spec
			// §3.2, §6) -- it is simply recorded as unextractable downstream
			// by carrying zero bytes, which attachments.go's size/allow-list
			// check will exclude.
			content = nil
		}
		attachments = append(attachments, InboundAttachment{
			Filename:    strings.TrimSpace(attachment.Name),
			ContentType: strings.TrimSpace(attachment.ContentType),
			Content:     content,
		})
	}

	auth := derivePostmarkAuth(raw.Headers)

	return InboundMailTaskEvent{
		Provider:          "postmark",
		ProviderMessageID: strings.TrimSpace(raw.MessageID),
		Alias:             strings.ToLower(alias),
		ToDomain:          strings.ToLower(domain),
		FromEmail:         fromEmail,
		FromName:          strings.TrimSpace(raw.FromFull.Name),
		AuthResult:        auth.Verdict,
		AuthDiagnostic:    auth,
		Subject:           raw.Subject,
		TextBody:          raw.TextBody,
		HTMLBody:          raw.HtmlBody,
		Attachments:       attachments,
	}, nil
}

func splitAliasAndDomain(address string) (alias, domain string) {
	address = strings.TrimSpace(address)
	// A recipient can arrive as "Name <alias@domain>" — take the angle-bracket
	// form when present.
	if start := strings.LastIndex(address, "<"); start != -1 {
		if end := strings.Index(address[start:], ">"); end != -1 {
			address = address[start+1 : start+end]
		}
	}
	local, dom, ok := strings.Cut(address, "@")
	if !ok {
		return "", ""
	}
	return strings.TrimSpace(local), strings.TrimSpace(dom)
}

var spfResultPattern = regexp.MustCompile(`(?i)\bspf=([a-z]+)`)
var dkimResultPattern = regexp.MustCompile(`(?i)\bdkim=([a-z]+)`)

// postmarkAuthRule is one source of the SPF+DKIM verdict: a header, and how to
// read a verdict out of its (single) value.
type postmarkAuthRule struct {
	// name labels the rule in diagnostics; it is also the lower-cased header
	// name it reads.
	name string
	// eval returns the verdict ("pass", "fail", or "none" when the value
	// carries no SPF/DKIM signal at all) and a closed-vocabulary detail.
	eval func(value string) (verdict, detail string)
}

// postmarkAuthRules is the ordered list of headers the verdict may be read from.
// derivePostmarkAuth consults the FIRST rule whose header is present and stops
// there: a later rule is only a fallback for the header's absence, never a way
// to overturn or replace a verdict an earlier header already gave.
//
// Calibration (2026-10-01, from Postmark's own documentation, NOT from a real
// payload -- see TestPostmarkRealRedactedPayloadDerivesPass):
//
//   - Postmark documents, in the inbound webhook and Parse an email pages, that
//     it scans inbound mail with SpamAssassin and passes the result in
//     X-Spam-Status, X-Spam-Score and X-Spam-Tests headers inside Headers[],
//     with X-Spam-Tests values such as "DKIM_SIGNED,DKIM_VALID,DKIM_VALID_AU,
//     SPF_PASS". It also documents a Received-SPF header.
//   - Postmark does NOT document adding an Authentication-Results header. The
//     rule this receiver shipped with read only that header, so against real
//     Postmark payloads it saw nothing, derived "none", and dropped every
//     message with a 200 and no visible error.
//   - Postmark also documents that the scan can be skipped, in which case the
//     SpamAssassin headers "will not be present or may be incomplete".
//
// So X-Spam-Tests is first. Authentication-Results stays only as the fallback
// for a payload with no X-Spam-Tests header at all; its provenance is
// unverified (the message's own sender can put one in the headers), which is
// why it never overrides X-Spam-Tests and why the first real payload must pin
// whether it should stay.
var postmarkAuthRules = []postmarkAuthRule{
	{name: authRuleSpamTests, eval: evalSpamTestsHeader},
	{name: authRuleAuthResults, eval: evalAuthenticationResultsHeader},
}

// derivePostmarkAuth collapses the payload's headers into one SPF+DKIM verdict
// and the content-free account of how it got there. verify.go's VerifySender
// treats only "pass" as acceptable -- this function never returns a "maybe".
//
// Trust note: the inbound Headers[] array is the message's raw headers plus the
// ones Postmark adds, so a sender can plant a header of any name, including
// X-Spam-Tests, in the mail they send. This function refuses to guess which of
// several same-named headers is the trustworthy one: when the header it reads
// appears more than once it answers "fail", which VerifySender turns into a
// rejection, rather than picking a side of an ambiguous input. (A planted
// header can only matter when Postmark's own is missing; the duplicate rule
// closes the case where both exist.)
func derivePostmarkAuth(headers []postmarkHeader) AuthDiagnostic {
	counts := make(map[string]int, len(headers))
	firstValue := make(map[string]string, len(postmarkAuthRules))
	for _, header := range headers {
		name := strings.ToLower(strings.TrimSpace(header.Name))
		if name == "" {
			continue
		}
		counts[name]++
		if counts[name] == 1 {
			firstValue[name] = header.Value
		}
	}

	diag := AuthDiagnostic{
		Verdict:     authVerdictNone,
		Rule:        authRuleNone,
		Detail:      authDetailNoUsableHeader,
		HeaderNames: sanitizedHeaderNames(counts),
	}
	for _, rule := range postmarkAuthRules {
		switch seen := counts[rule.name]; {
		case seen == 0:
			continue
		case seen > 1:
			diag.Verdict, diag.Rule, diag.Detail = authVerdictFail, rule.name, authDetailDuplicateHeader
			return diag
		}
		diag.Verdict, diag.Detail = rule.eval(firstValue[rule.name])
		diag.Rule = rule.name
		return diag
	}
	return diag
}

// evalSpamTestsHeader reads the SpamAssassin test list Postmark adds. It is
// "pass" only when the list carries BOTH SPF_PASS (the envelope-from SPF check
// passed) AND DKIM_VALID_AU (a valid DKIM signature from the author's -- the
// From header's -- domain). Plain DKIM_VALID is not enough: it only says some
// signature verified, not that it belongs to the domain the From address
// claims. Tokens are matched exactly, case-insensitively, so a look-alike such
// as "MY_SPF_PASS" never counts. Any other SPF_* or DKIM_* token (SPF_FAIL,
// SPF_SOFTFAIL, SPF_HELO_PASS, DKIM_INVALID, ...) means authentication was
// evaluated and did not pass; a list with no SPF_* or DKIM_* token at all
// carries no verdict ("none").
func evalSpamTestsHeader(value string) (verdict, detail string) {
	var spfPass, dkimAligned, signal bool
	tokens := strings.FieldsFunc(value, func(r rune) bool { return r == ',' || unicode.IsSpace(r) })
	for _, token := range tokens {
		token = strings.ToUpper(token)
		switch {
		case token == "SPF_PASS":
			spfPass, signal = true, true
		case token == "DKIM_VALID_AU":
			dkimAligned, signal = true, true
		case strings.HasPrefix(token, "SPF_"), strings.HasPrefix(token, "DKIM_"):
			signal = true
		}
	}
	switch {
	case spfPass && dkimAligned:
		return authVerdictPass, authDetailOK
	case !signal:
		return authVerdictNone, authDetailNoSignalTokens
	case !spfPass && !dkimAligned:
		return authVerdictFail, authDetailSPFDKIMNotPass
	case !spfPass:
		return authVerdictFail, authDetailSPFNotPass
	default:
		return authVerdictFail, authDetailDKIMNotAligned
	}
}

// evalAuthenticationResultsHeader reads an `Authentication-Results` header:
// "pass" only when BOTH its spf= and dkim= results are pass, "fail" when either
// token is present and the pair is not, "none" when neither token is present.
func evalAuthenticationResultsHeader(value string) (verdict, detail string) {
	spf := firstMatch(spfResultPattern, value)
	dkim := firstMatch(dkimResultPattern, value)
	switch {
	case spf == "" && dkim == "":
		return authVerdictNone, authDetailNoSignalTokens
	case strings.EqualFold(spf, "pass") && strings.EqualFold(dkim, "pass"):
		return authVerdictPass, authDetailOK
	default:
		return authVerdictFail, authDetailSPFOrDKIMFailed
	}
}

func firstMatch(pattern *regexp.Regexp, value string) string {
	match := pattern.FindStringSubmatch(value)
	if len(match) < 2 {
		return ""
	}
	return match[1]
}
