package mailtask

import (
	"encoding/base64"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
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
	OriginalRecipient string `json:"OriginalRecipient"`
	To                string `json:"To"`
	Subject           string `json:"Subject"`
	TextBody          string `json:"TextBody"`
	HtmlBody          string `json:"HtmlBody"`
	Headers           []struct {
		Name  string `json:"Name"`
		Value string `json:"Value"`
	} `json:"Headers"`
	Attachments []struct {
		Name        string `json:"Name"`
		Content     string `json:"Content"` // base64
		ContentType string `json:"ContentType"`
	} `json:"Attachments"`
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

	return InboundMailTaskEvent{
		Provider:          "postmark",
		ProviderMessageID: strings.TrimSpace(raw.MessageID),
		Alias:             strings.ToLower(alias),
		ToDomain:          strings.ToLower(domain),
		FromEmail:         fromEmail,
		FromName:          strings.TrimSpace(raw.FromFull.Name),
		AuthResult:        derivePostmarkAuthResult(raw.Headers),
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

var authResultsHeaderName = "authentication-results"
var spfResultPattern = regexp.MustCompile(`(?i)\bspf=([a-z]+)`)
var dkimResultPattern = regexp.MustCompile(`(?i)\bdkim=([a-z]+)`)

// derivePostmarkAuthResult reads the forwarded `Authentication-Results`
// header and collapses SPF+DKIM into one verdict: "pass" only when BOTH
// explicitly say pass, "fail" when either explicitly says anything else,
// "none" when the header is missing or neither token is present. verify.go's
// VerifySender treats only "pass" as acceptable -- this function never
// returns a "maybe".
//
// Trust note: this header is the ONE input to the anti-spoofing gate that
// does not come from Postmark's own structured fields (MessageID, To,
// FromFull, ...) -- it is a free-text header line, and the inbound Headers[]
// array is (per Postmark's own documented contract) the message's raw
// headers, which can in principle include more than one header of this name
// if the originating message already carried one before Postmark's own
// receiving MTA added its own trace header. A forger who cannot pass real
// SPF/DKIM for the alias owner's domain could otherwise try to inject their
// own "Authentication-Results: ...; spf=pass; dkim=pass" line into the
// message they send. This function refuses to guess which of several
// same-named headers is the trustworthy one it never returns "pass" when
// more than one is present, which fails closed into VerifySender's rejection
// path rather than picking a side of an ambiguous input.
func derivePostmarkAuthResult(headers []struct {
	Name  string `json:"Name"`
	Value string `json:"Value"`
}) string {
	var value string
	matches := 0
	for _, header := range headers {
		if strings.EqualFold(strings.TrimSpace(header.Name), authResultsHeaderName) {
			matches++
			if matches == 1 {
				value = header.Value
			}
		}
	}
	if matches > 1 {
		return "fail"
	}
	if value == "" {
		return "none"
	}
	spf := firstMatch(spfResultPattern, value)
	dkim := firstMatch(dkimResultPattern, value)
	if spf == "" && dkim == "" {
		return "none"
	}
	if strings.EqualFold(spf, "pass") && strings.EqualFold(dkim, "pass") {
		return "pass"
	}
	return "fail"
}

func firstMatch(pattern *regexp.Regexp, value string) string {
	match := pattern.FindStringSubmatch(value)
	if len(match) < 2 {
		return ""
	}
	return match[1]
}
