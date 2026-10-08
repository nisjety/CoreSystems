package actions

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"regexp"
	"strings"
	"sync"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/integration-corev2/internal/htmlstrip"
)

// Mail read for the connected user's own Outlook (Microsoft Graph) and Gmail
// mailbox: the operations mail.messages, mail.messages.get, gmail.messages and
// gmail.messages.get.
//
// One provider-neutral parameter set is validated once (parseMailQuery) and
// then composed per provider, so neither provider's query language is ever
// assembled from unvalidated text:
//
//	query       free text in the provider's own search syntax
//	from        a sender address or display name
//	since       YYYY-MM-DD, first day to include
//	until       YYYY-MM-DD, last day to include (inclusive)
//	folder      inbox | sent | drafts | archive | deleted
//	unread      only unread messages
//	maxResults  default 10, at most 100 (the picker's cap). The agent's own
//	            tool (planned in F6, not built yet) is to clamp to 25 before it
//	            calls; this layer cannot tell the two callers apart.
//	pageToken   the nextPageToken of the previous page, passed back unchanged
//	headers     Gmail only: add subject, from, to, cc, date, snippet and
//	            labelIds to each listed message (bounded fan-out, see below)
//
// Everything is additive: a call without these params produces the request it
// always did (apart from the Outlook list now being ordered newest first and
// selecting a few more fields), which matters because the composer picker and
// execution-core's gmail.send read-back both call these operations.
//
// What is not verified against a live provider yet (no mailbox was available
// when this was written; the live proof is task F10): that Graph accepts
// `received>=` and `received<=` date restrictions inside $search, that it
// really refuses $filter and $orderby next to $search on messages, and that
// $expand=attachments($select=...) is returned when a $select is also given.
// Each of those is isolated in one helper below so a real response can change
// one line.

const (
	mailDefaultMaxResults       = 10
	mailMaxResultsCap           = 100
	mailHeaderFanOutCap         = 25
	mailHeaderFanOutConcurrency = 5
	mailQueryMaxRunes           = 500
	mailFromMaxRunes            = 320
	mailPageTokenMaxBytes       = 4096
	mailDateLayout              = "2006-01-02"
	gmailDateLayout             = "2006/01/02"
	gmailMaxAttachmentsListed   = 50

	// graphMailFloorDate leads the $filter when only "unread" is asked for:
	// Graph rejects an $orderby whose property does not lead the $filter
	// (InefficientFilter), so the filter needs a harmless receivedDateTime
	// bound to put in front of isRead.
	graphMailFloorDate = "1900-01-01T00:00:00Z"

	graphMailListSelect = "id,subject,from,receivedDateTime,webLink,bodyPreview,hasAttachments,isRead,conversationId,parentFolderId"
	graphMailGetSelect  = "id,subject,from,receivedDateTime,webLink,body,toRecipients,ccRecipients,hasAttachments,conversationId,internetMessageId"
	// Attachment metadata only: without the inner $select Graph inlines every
	// file attachment's bytes (contentBytes) into the message.
	graphMailAttachmentExpand = "attachments($select=id,name,contentType,size,isInline)"
)

// graphMailFolders maps the neutral folder names to Graph's well-known folder
// names, which Graph accepts in place of a folder id.
var graphMailFolders = map[string]string{
	"inbox":   "inbox",
	"sent":    "sentitems",
	"drafts":  "drafts",
	"archive": "archive",
	"deleted": "deleteditems",
}

// gmailMailFolderLabels maps a folder to the Gmail label it lives under.
// Archive has no label: it is everything that is no longer in the inbox,
// expressed in the search string by gmailArchiveTerms.
var gmailMailFolderLabels = map[string]string{
	"inbox":   "INBOX",
	"sent":    "SENT",
	"drafts":  "DRAFT",
	"deleted": "TRASH",
}

const gmailArchiveTerms = "-in:inbox -in:sent -in:drafts -in:chats"

var mailFolderAliases = map[string]string{
	"inbox":        "inbox",
	"sent":         "sent",
	"sentitems":    "sent",
	"drafts":       "drafts",
	"draft":        "drafts",
	"archive":      "archive",
	"deleted":      "deleted",
	"deleteditems": "deleted",
	"trash":        "deleted",
}

// mailSafeOperand matches a sender that can stand after `from:` with no
// quoting: an address or a single word. Anything else (a display name, or text
// that tries to start another search operator) is quoted as one phrase.
var mailSafeOperand = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._%+'@-]*$`)

type mailQuery struct {
	Query     string
	From      string
	Since     time.Time
	Until     time.Time
	Folder    string
	Unread    bool
	PageToken string
}

func parseMailQuery(params map[string]any) (mailQuery, error) {
	var q mailQuery
	var err error
	if q.Query, err = mailTextParam(params, "query", mailQueryMaxRunes); err != nil {
		return q, err
	}
	rawFrom, err := mailTextParam(params, "from", mailFromMaxRunes)
	if err != nil {
		return q, err
	}
	if rawFrom != "" {
		if q.From = mailCleanOperand(rawFrom); q.From == "" {
			return q, fmt.Errorf("from has no usable address or name")
		}
	}
	if q.Since, err = mailDateParam(params, "since"); err != nil {
		return q, err
	}
	if q.Until, err = mailDateParam(params, "until"); err != nil {
		return q, err
	}
	if !q.Since.IsZero() && !q.Until.IsZero() && q.Since.After(q.Until) {
		return q, fmt.Errorf("since must not be after until")
	}
	if q.Folder, err = mailFolderParam(params); err != nil {
		return q, err
	}
	if q.Unread, err = mailBoolParam(params, "unread"); err != nil {
		return q, err
	}
	if q.PageToken, err = mailPageTokenParam(params); err != nil {
		return q, err
	}
	return q, nil
}

// mailStringParam reads an optional string param. A value of another type is an
// error rather than silently ignored: an ignored filter would widen a search
// the caller thought was narrowed.
func mailStringParam(params map[string]any, key string) (string, error) {
	value, ok := params[key]
	if !ok || value == nil {
		return "", nil
	}
	text, ok := value.(string)
	if !ok {
		return "", fmt.Errorf("%s must be text", key)
	}
	return strings.TrimSpace(text), nil
}

// mailTextParam reads free text and removes the characters that could break a
// provider query line or smuggle a second one.
func mailTextParam(params map[string]any, key string, maxRunes int) (string, error) {
	raw, err := mailStringParam(params, key)
	if err != nil || raw == "" {
		return "", err
	}
	if utf8.RuneCountInString(raw) > maxRunes {
		return "", fmt.Errorf("%s is too long (at most %d characters)", key, maxRunes)
	}
	return mailCleanText(raw), nil
}

// mailCleanText turns every control character and Unicode line or paragraph
// separator into a space, drops invisible formatting characters (zero-width
// joiners, bidirectional overrides), and collapses runs of whitespace.
func mailCleanText(value string) string {
	var out strings.Builder
	out.Grow(len(value))
	for _, r := range value {
		switch {
		case unicode.IsControl(r), r == ' ', r == ' ':
			out.WriteByte(' ')
		case unicode.Is(unicode.Cf, r):
			// dropped
		default:
			out.WriteRune(r)
		}
	}
	return strings.Join(strings.Fields(out.String()), " ")
}

// mailCleanOperand reduces a sender to text that is safe inside a quoted
// phrase: it is cleaned like any free text and also loses the characters that
// would end the phrase or open a group.
func mailCleanOperand(value string) string {
	replacer := strings.NewReplacer(`"`, " ", `\`, " ", "(", " ", ")", " ", "{", " ", "}", " ")
	return strings.Join(strings.Fields(replacer.Replace(mailCleanText(value))), " ")
}

// mailOperand renders a cleaned sender for a search expression.
func mailOperand(cleaned string) string {
	if mailSafeOperand.MatchString(cleaned) {
		return cleaned
	}
	return `"` + cleaned + `"`
}

// mailGroupFreeText parenthesises a free-text query that contains an OR, when
// other restrictions follow it: in both providers' grammars an unparenthesised
// OR would otherwise bind the sender or date restriction to only one
// alternative.
func mailGroupFreeText(query string, restrictionsFollow bool) string {
	if restrictionsFollow && strings.Contains(" "+query+" ", " OR ") {
		return "(" + query + ")"
	}
	return query
}

func mailDateParam(params map[string]any, key string) (time.Time, error) {
	raw, err := mailStringParam(params, key)
	if err != nil || raw == "" {
		return time.Time{}, err
	}
	parsed, parseErr := time.Parse(mailDateLayout, raw)
	if parseErr != nil || parsed.Format(mailDateLayout) != raw || parsed.Year() < 1970 || parsed.Year() > 2200 {
		return time.Time{}, fmt.Errorf("%s must be a date in YYYY-MM-DD form", key)
	}
	return parsed, nil
}

func mailFolderParam(params map[string]any) (string, error) {
	raw, err := mailStringParam(params, "folder")
	if err != nil || raw == "" {
		return "", err
	}
	if folder, ok := mailFolderAliases[strings.ToLower(raw)]; ok {
		return folder, nil
	}
	return "", fmt.Errorf("folder must be one of inbox, sent, drafts, archive, deleted")
}

// mailBoolParam accepts a JSON boolean and the strings the legacy GET routes
// deliver query parameters as.
func mailBoolParam(params map[string]any, key string) (bool, error) {
	value, ok := params[key]
	if !ok || value == nil {
		return false, nil
	}
	switch typed := value.(type) {
	case bool:
		return typed, nil
	case string:
		switch strings.ToLower(strings.TrimSpace(typed)) {
		case "true", "1", "yes":
			return true, nil
		case "false", "0", "no", "":
			return false, nil
		}
	}
	return false, fmt.Errorf("%s must be true or false", key)
}

func mailPageTokenParam(params map[string]any) (string, error) {
	raw, err := mailStringParam(params, "pageToken")
	if err != nil || raw == "" {
		return "", err
	}
	if len(raw) > mailPageTokenMaxBytes {
		return "", fmt.Errorf("pageToken is too long")
	}
	for _, r := range raw {
		if unicode.IsControl(r) {
			return "", fmt.Errorf("pageToken contains a control character")
		}
	}
	return raw, nil
}

// mailInt64 reads a JSON number (float64 after decoding) or an integer.
func mailInt64(value any) int64 {
	switch typed := value.(type) {
	case float64:
		return int64(typed)
	case int:
		return int64(typed)
	case int64:
		return typed
	}
	return 0
}

// mailItemError is the short, fixed-vocabulary reason one listed message could
// not be enriched. It never carries the provider's own error text, which can
// echo request content.
func mailItemError(err error) string {
	switch {
	case errors.Is(err, context.DeadlineExceeded):
		return "timed out"
	case errors.Is(err, context.Canceled):
		return "canceled"
	}
	if rejection, ok := AsProviderAuthError(err); ok {
		return rejection.Error()
	}
	if message := err.Error(); strings.HasPrefix(message, "provider action endpoint returned status") {
		return message
	}
	return "metadata fetch failed"
}

// --- Outlook (Microsoft Graph) ------------------------------------------------

func (s *Service) microsoftMailMessages(ctx context.Context, token, base string, params map[string]any) (any, error) {
	q, err := parseMailQuery(params)
	if err != nil {
		return nil, err
	}
	var endpoint string
	if q.PageToken != "" {
		// The link carries the whole previous query ($select, $top, $skiptoken
		// and the rest), so it is replayed as is and nothing else is applied.
		if endpoint, err = s.graphMailPageURL(q.PageToken); err != nil {
			return nil, err
		}
	} else {
		endpoint = graphMailListURL(base, q, limitParam(params, "maxResults", mailDefaultMaxResults, mailMaxResultsCap))
	}
	result, err := s.getBearer(ctx, token, endpoint, nil)
	if err != nil {
		return nil, err
	}
	return s.finishGraphMailList(result, q), nil
}

func (s *Service) microsoftMailMessageGet(ctx context.Context, token, base string, params map[string]any) (any, error) {
	messageID, err := requiredStringParam(params, "messageId")
	if err != nil {
		return nil, err
	}
	values := url.Values{
		"$select": {graphMailGetSelect},
		"$expand": {graphMailAttachmentExpand},
	}
	result, err := s.getBearerMap(ctx, token, base+"/me/messages/"+url.PathEscape(messageID)+"?"+values.Encode(), nil)
	if err != nil {
		return nil, err
	}
	if bodyField, ok := result["body"].(map[string]any); ok {
		content := stringFromAny(bodyField["content"])
		if strings.EqualFold(stringFromAny(bodyField["contentType"]), "html") {
			result["bodyText"] = htmlstrip.StripHTMLTags(content)
		} else {
			result["bodyText"] = content
		}
	}
	return result, nil
}

// graphMailListURL builds the list request. A free-text or sender search uses
// $search with no $filter and no $orderby (Graph does not combine them on
// messages; results come back newest first by sent date). Everything else uses
// $filter plus $orderby receivedDateTime desc.
func graphMailListURL(base string, q mailQuery, top string) string {
	endpoint := base + "/me/messages"
	if folder, ok := graphMailFolders[q.Folder]; ok {
		endpoint = base + "/me/mailFolders/" + folder + "/messages"
	}
	values := url.Values{"$top": {top}, "$select": {graphMailListSelect}}
	if graphMailUsesSearch(q) {
		values.Set("$search", `"`+searchPhraseEscape(graphMailSearchExpression(q))+`"`)
	} else {
		if filter := graphMailFilter(q); filter != "" {
			values.Set("$filter", filter)
		}
		values.Set("$orderby", "receivedDateTime desc")
	}
	// Spaces go out as %20, not Encode's "+": a literal plus is already %2B, so
	// this is the same query for any decoder, and it does not rely on Graph
	// reading "+" as a space inside $filter and $search.
	return endpoint + "?" + strings.ReplaceAll(values.Encode(), "+", "%20")
}

// graphMailUsesSearch: only a free-text or sender restriction needs $search.
// Dates and unread alone are served by $filter, which is exact.
func graphMailUsesSearch(q mailQuery) bool {
	return q.From != "" || graphKQLFreeText(q.Query) != ""
}

// graphMailSearchExpression is the KQL inside $search's double quotes, before
// the quotes are escaped. Date bounds are whole days; `until` is inclusive.
func graphMailSearchExpression(q mailQuery) string {
	restrictions := []string{}
	if q.From != "" {
		restrictions = append(restrictions, "from:"+mailOperand(q.From))
	}
	if !q.Since.IsZero() {
		restrictions = append(restrictions, "received>="+q.Since.Format(mailDateLayout))
	}
	if !q.Until.IsZero() {
		restrictions = append(restrictions, "received<="+q.Until.Format(mailDateLayout))
	}
	parts := []string{}
	if query := graphKQLFreeText(q.Query); query != "" {
		parts = append(parts, mailGroupFreeText(query, len(restrictions) > 0))
	}
	return strings.Join(append(parts, restrictions...), " ")
}

// graphKQLFreeText keeps a half-typed query from being sent as a syntax error.
// The composer sends the search box on every debounced keystroke, so
// `"quarterly rep` or `invoice (final` reaches Graph as an unterminated phrase
// or group. Quotes are dropped when they do not pair up, and parentheses when
// they do not balance; a well-formed query is untouched.
func graphKQLFreeText(query string) string {
	if strings.Count(query, `"`)%2 == 1 {
		query = strings.ReplaceAll(query, `"`, " ")
	}
	depth := 0
	balanced := true
	for _, r := range query {
		switch r {
		case '(':
			depth++
		case ')':
			if depth--; depth < 0 {
				balanced = false
			}
		}
	}
	if depth != 0 || !balanced {
		query = strings.NewReplacer("(", " ", ")", " ").Replace(query)
	}
	return strings.Join(strings.Fields(query), " ")
}

// graphMailFilter is the $filter of the non-search path. The property that is
// ordered on leads the filter, with a floor date when nothing else bounds it,
// because Graph rejects any other arrangement as InefficientFilter.
func graphMailFilter(q mailQuery) string {
	clauses := []string{}
	if !q.Since.IsZero() {
		clauses = append(clauses, "receivedDateTime ge "+q.Since.Format(mailDateLayout)+"T00:00:00Z")
	}
	if !q.Until.IsZero() {
		clauses = append(clauses, "receivedDateTime lt "+q.Until.AddDate(0, 0, 1).Format(mailDateLayout)+"T00:00:00Z")
	}
	if q.Unread {
		if len(clauses) == 0 {
			clauses = append(clauses, "receivedDateTime ge "+graphMailFloorDate)
		}
		clauses = append(clauses, "isRead eq false")
	}
	return strings.Join(clauses, " and ")
}

// graphMailPageURL admits a page token only if it is exactly the kind of link
// Graph hands out for a message list on the configured Graph host. The token
// comes back from the caller, who may be an agent steered by mail content, and
// the request that follows carries the user's bearer token, so a link to
// another host, a plaintext link, another user's mailbox or a non-mail
// resource under /me (OneDrive, say, which this operation's capability does not
// cover) is refused before any request is made.
func (s *Service) graphMailPageURL(raw string) (string, error) {
	invalid := fmt.Errorf("pageToken is not a mail page token from this connection")
	link, err := url.Parse(raw)
	if err != nil || link.Scheme != "https" || link.Opaque != "" || link.User != nil || link.Fragment != "" || link.Host == "" {
		return "", invalid
	}
	configured, err := url.Parse(strings.TrimRight(s.cfg.MicrosoftGraphBaseURL, "/"))
	if err != nil || configured.Host == "" || !strings.EqualFold(link.Host, configured.Host) {
		return "", invalid
	}
	prefix := strings.TrimRight(configured.Path, "/") + "/v1.0/me/"
	if !strings.HasPrefix(link.Path, prefix) {
		return "", invalid
	}
	segments := strings.Split(strings.TrimPrefix(link.Path, prefix), "/")
	for _, segment := range segments {
		if segment == "" || segment == "." || segment == ".." {
			return "", invalid
		}
	}
	if segments[len(segments)-1] != "messages" {
		return "", invalid
	}
	if len(segments) > 1 && !strings.HasPrefix(segments[0], "mailFolders") {
		return "", invalid
	}
	return raw, nil
}

// finishGraphMailList adds nextPageToken beside the Graph `value`, and applies
// the unread restriction locally: $search cannot take a $filter, so on the
// search path (and on any replayed page whose origin is unknown) read messages
// are dropped here. On the filter path every row is already unread and this
// changes nothing.
func (s *Service) finishGraphMailList(result any, q mailQuery) any {
	list, ok := result.(map[string]any)
	if !ok {
		return result
	}
	if next := stringFromAny(list["@odata.nextLink"]); next != "" {
		if _, err := s.graphMailPageURL(next); err == nil {
			list["nextPageToken"] = next
		}
	}
	if q.Unread {
		if rows, ok := list["value"].([]any); ok {
			kept := make([]any, 0, len(rows))
			for _, row := range rows {
				if message, ok := row.(map[string]any); ok && message["isRead"] == true {
					continue
				}
				kept = append(kept, row)
			}
			if dropped := len(rows) - len(kept); dropped > 0 {
				list["value"] = kept
				list["filteredOutRead"] = dropped
			}
		}
	}
	return list
}

// --- Gmail --------------------------------------------------------------------

func (s *Service) googleGmailMessages(ctx context.Context, token, base string, params map[string]any) (any, error) {
	q, err := parseMailQuery(params)
	if err != nil {
		return nil, err
	}
	withHeaders, err := mailBoolParam(params, "headers")
	if err != nil {
		return nil, err
	}
	// With headers every listed message costs one more call, so the page is
	// clamped to what the fan-out will enrich: nothing in the page is left
	// without its subject, and nextPageToken carries the rest.
	pageCap := mailMaxResultsCap
	if withHeaders {
		pageCap = mailHeaderFanOutCap
	}
	values := url.Values{"maxResults": {limitParam(params, "maxResults", mailDefaultMaxResults, pageCap)}}
	if search := gmailSearchExpression(q); search != "" {
		values.Set("q", search)
	}
	for _, label := range gmailLabelIDs(q) {
		values.Add("labelIds", label)
	}
	if q.Folder == "deleted" {
		// Trash is left out of a listing unless asked for.
		values.Set("includeSpamTrash", "true")
	}
	if q.PageToken != "" {
		values.Set("pageToken", q.PageToken)
	}
	result, err := s.getBearer(ctx, token, base+"/gmail/v1/users/me/messages?"+values.Encode(), nil)
	if err != nil {
		return nil, err
	}
	if withHeaders {
		if list, ok := result.(map[string]any); ok {
			s.gmailAttachHeaders(ctx, token, base, list)
		}
	}
	return result, nil
}

func (s *Service) googleGmailMessageGet(ctx context.Context, token, base string, params map[string]any) (any, error) {
	messageID, err := requiredStringParam(params, "id")
	if err != nil {
		return nil, err
	}
	values := url.Values{"format": {"full"}}
	raw, err := s.getBearerMap(ctx, token, base+"/gmail/v1/users/me/messages/"+url.PathEscape(messageID)+"?"+values.Encode(), nil)
	if err != nil {
		return nil, err
	}
	payload, _ := raw["payload"].(map[string]any)
	headers := gmailHeaderMap(payload)
	text, html := extractGmailMessageBodies(payload)
	bodyText := text
	if bodyText == "" && html != "" {
		// Gmail's own listing comment already notes the list endpoint
		// never carries a body regardless of params — that is a search
		// limitation, not a signal that no body exists here. format=full
		// always returns one MIME tree; text/plain wins when present,
		// otherwise the text/html leaf is rendered down to text.
		bodyText = htmlstrip.StripHTMLTags(html)
	}
	return map[string]any{
		"id":          raw["id"],
		"threadId":    raw["threadId"],
		"subject":     headers["subject"],
		"from":        headers["from"],
		"to":          headers["to"],
		"cc":          headers["cc"],
		"date":        headers["date"],
		"snippet":     raw["snippet"],
		"bodyText":    bodyText,
		"attachments": gmailAttachmentList(payload),
	}, nil
}

// gmailSearchExpression is the `q` of a Gmail list. The free-text query is the
// user's own Gmail search and is passed as one value (never a second request
// parameter); the sender is a single `from:` operand and the dates are whole
// days, with `before:` one day past the inclusive `until` because Gmail's
// `before:` excludes its day.
func gmailSearchExpression(q mailQuery) string {
	restrictions := []string{}
	if q.From != "" {
		restrictions = append(restrictions, "from:"+mailOperand(q.From))
	}
	if !q.Since.IsZero() {
		restrictions = append(restrictions, "after:"+q.Since.Format(gmailDateLayout))
	}
	if !q.Until.IsZero() {
		restrictions = append(restrictions, "before:"+q.Until.AddDate(0, 0, 1).Format(gmailDateLayout))
	}
	if q.Folder == "archive" {
		restrictions = append(restrictions, gmailArchiveTerms)
	}
	parts := []string{}
	if q.Query != "" {
		parts = append(parts, mailGroupFreeText(q.Query, len(restrictions) > 0))
	}
	return strings.Join(append(parts, restrictions...), " ")
}

// gmailLabelIDs are the labels a listed message must carry (all of them).
func gmailLabelIDs(q mailQuery) []string {
	labels := []string{}
	if label, ok := gmailMailFolderLabels[q.Folder]; ok {
		labels = append(labels, label)
	}
	if q.Unread {
		labels = append(labels, "UNREAD")
	}
	return labels
}

// gmailAttachHeaders replaces each listed message (at most mailHeaderFanOutCap
// of them) with its id, threadId and headers read through
// messages.get?format=metadata, five at a time. One message failing becomes
// {id, error} in its place; it never fails the list. Messages past the cap stay
// as the bare ids Gmail returned.
func (s *Service) gmailAttachHeaders(ctx context.Context, token, base string, list map[string]any) {
	listed, _ := list["messages"].([]any)
	if len(listed) == 0 {
		return
	}
	enriched := make([]any, len(listed))
	copy(enriched, listed)
	limit := len(listed)
	if limit > mailHeaderFanOutCap {
		limit = mailHeaderFanOutCap
	}
	slots := make(chan struct{}, mailHeaderFanOutConcurrency)
	var wg sync.WaitGroup
	for index := 0; index < limit; index++ {
		item, _ := listed[index].(map[string]any)
		id := stringFromAny(item["id"])
		if id == "" {
			continue
		}
		select {
		case slots <- struct{}{}:
		case <-ctx.Done():
			enriched[index] = map[string]any{"id": id, "error": mailItemError(ctx.Err())}
			continue
		}
		wg.Add(1)
		go func(index int, id string, item map[string]any) {
			defer wg.Done()
			defer func() { <-slots }()
			enriched[index] = s.gmailMessageSummary(ctx, token, base, id, item)
		}(index, id, item)
	}
	wg.Wait()
	list["messages"] = enriched
}

func (s *Service) gmailMessageSummary(ctx context.Context, token, base, id string, listed map[string]any) map[string]any {
	values := url.Values{"format": {"metadata"}}
	for _, header := range []string{"Subject", "From", "To", "Cc", "Date"} {
		values.Add("metadataHeaders", header)
	}
	raw, err := s.getBearerMap(ctx, token, base+"/gmail/v1/users/me/messages/"+url.PathEscape(id)+"?"+values.Encode(), nil)
	if err != nil {
		return map[string]any{"id": id, "error": mailItemError(err)}
	}
	payload, _ := raw["payload"].(map[string]any)
	headers := gmailHeaderMap(payload)
	threadID := listed["threadId"]
	if threadID == nil {
		threadID = raw["threadId"]
	}
	labels, ok := raw["labelIds"].([]any)
	if !ok {
		labels = []any{}
	}
	summary := map[string]any{
		"id":       id,
		"threadId": threadID,
		"subject":  headers["subject"],
		"from":     headers["from"],
		"to":       headers["to"],
		"date":     headers["date"],
		"snippet":  raw["snippet"],
		"labelIds": labels,
	}
	if cc := headers["cc"]; cc != "" {
		summary["cc"] = cc
	}
	return summary
}

// gmailAttachmentList lists the attachments of a full-format message: every
// MIME part that has a filename. A part with only an attachmentId and no
// filename is a large body part, not an attachment. The content is a separate
// operation and is never read here.
func gmailAttachmentList(payload map[string]any) []any {
	attachments := []any{}
	var walk func(part map[string]any, depth int)
	walk = func(part map[string]any, depth int) {
		if part == nil || depth > 8 || len(attachments) >= gmailMaxAttachmentsListed {
			return
		}
		if filename := stringFromAny(part["filename"]); filename != "" {
			bodyField, _ := part["body"].(map[string]any)
			attachments = append(attachments, map[string]any{
				"filename":     filename,
				"mimeType":     stringFromAny(part["mimeType"]),
				"size":         mailInt64(bodyField["size"]),
				"attachmentId": stringFromAny(bodyField["attachmentId"]),
			})
		}
		children, _ := part["parts"].([]any)
		for _, child := range children {
			if childPart, ok := child.(map[string]any); ok {
				walk(childPart, depth+1)
			}
		}
	}
	walk(payload, 0)
	return attachments
}
