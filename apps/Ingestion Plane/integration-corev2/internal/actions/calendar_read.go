package actions

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/url"
	"strings"
	"sync"
	"time"
	// The runtime image is plain Alpine with no zoneinfo (checked on the running
	// integration-api container), so time.LoadLocation("Europe/Oslo") would fail
	// there without the embedded copy of the time zone database.
	_ "time/tzdata"
	"unicode"
	"unicode/utf8"
)

// Calendar read for the connected user's own Outlook (Microsoft Graph) and
// Google calendar: the operation calendar.events, which the composer picker and
// the agent's read tool (planned in F6) both call. One provider-neutral
// parameter set is validated once (parseCalendarRequest) and then composed per
// provider:
//
//	since       first moment to include: YYYY-MM-DD (the start of that day), a
//	            local date-time YYYY-MM-DDTHH:MM[:SS], or RFC 3339 with an offset
//	until       last moment to include: YYYY-MM-DD (the whole of that day, so the
//	            window ends at the next local midnight), or a date-time as above
//	query       free text; Google filters on its side (q), Outlook is filtered
//	            here in Go on the fetched page (see graphEventMatches)
//	maxResults  the size of the provider page, default 10, at most 100. With an
//	            Outlook query it defaults to 100 instead, because the filter only
//	            sees the page it fetched and a search over ten rows finds little
//	pageToken   the nextPageToken of the previous page, passed back unchanged
//
// Days and local date-times are Europe/Oslo (calendarZoneName). The window
// defaults to now through 14 days ahead; with a query and no window it is 30
// days back through 90 ahead; it is never longer than 366 days. Both providers
// answer in Oslo local time, so 09:00 local arrives as 09:00 and not as 07:00
// UTC.
//
// A nextPageToken is not the provider's own token. It wraps it together with
// the window and the query of the first page ("cal1." plus base64 JSON), because
// the window is otherwise defaulted from the clock again on every call: Google
// binds a page token to the query it was issued for, so a window that has moved
// on by a minute breaks the next page, and an Outlook page would lose its local
// filter. The token alone therefore continues a search; since, until and query
// sent beside a pageToken are ignored, and the result echoes the query and the
// window that are in effect. Everything inside the token is validated again
// exactly as if it had been passed as a parameter, because the token comes back
// from the caller, who may be an agent steered by event text.
//
// What is not verified against a live provider yet (no calendar was reachable
// when this was written: the only Microsoft connection on the dev stack lacks
// calendar.read until the F5 reconnect; the live proof is task F10): that Graph
// accepts $orderby=start/dateTime on calendarView, that its @odata.nextLink
// keeps the /me/calendarView path (a link in users('id') form would be refused
// and reported as moreAvailable without a token), and that it honours
// Prefer: outlook.body-content-type="text" there (documented, but a Graph that
// ignores it only costs HTML bodies, which the picker already strips). Whether
// Prefer: outlook.timezone takes IANA names is deliberately not relied on: the
// Windows name is sent.

const (
	calendarZoneName      = "Europe/Oslo"
	calendarGraphZoneName = "W. Europe Standard Time"
	// calendarGraphPrefer asks Graph for local start and end times and for a
	// plain-text event body. startDateTime and endDateTime are not affected by it:
	// Graph reads those by their own offset and takes none as UTC, so they are
	// always sent as UTC with a Z.
	calendarGraphPrefer = `outlook.timezone="` + calendarGraphZoneName + `", outlook.body-content-type="text"`

	calendarDefaultMaxResults = 10
	calendarSearchPageSize    = 100
	calendarMaxResultsCap     = 100
	calendarDefaultAheadDays  = 14
	calendarSearchBackDays    = 30
	calendarSearchAheadDays   = 90
	calendarMaxSpanDays       = 366
	calendarQueryMaxRunes     = 500

	calendarTokenPrefix      = "cal1."
	calendarTokenMaxBytes    = 8192
	calendarTokenMaxProvider = 3000

	calendarDateLayout = "2006-01-02"

	// body is selected beside bodyPreview because the composer picker builds an
	// Outlook event's attachment text from body.content. The local filter only
	// reads bodyPreview. F6's renderer truncates the body for the model.
	graphCalendarSelect  = "id,subject,start,end,location,organizer,attendees,bodyPreview,body,isAllDay,isCancelled,webLink,onlineMeeting,type,seriesMasterId"
	googleCalendarFields = "items(id,summary,description,location,start,end,attendees(displayName,email,responseStatus),organizer,htmlLink,status,hangoutLink),nextPageToken"
)

var (
	calendarZoneOnce sync.Once
	calendarZone     *time.Location
	calendarZoneErr  error
)

// calendarLocation is Europe/Oslo with its daylight saving rules.
func calendarLocation() (*time.Location, error) {
	calendarZoneOnce.Do(func() {
		calendarZone, calendarZoneErr = time.LoadLocation(calendarZoneName)
	})
	return calendarZone, calendarZoneErr
}

// clock is the service's notion of now (see Service.now).
func (s *Service) clock() time.Time {
	if s.now != nil {
		return s.now()
	}
	return time.Now()
}

type calendarRequest struct {
	Query string
	// Start and End are the window, in UTC.
	Start time.Time
	End   time.Time
	// Page is the provider's own page token (Google) or next link (Graph); empty
	// on a first page.
	Page     string
	PageSize string
	Local    *time.Location
}

// calendarPageToken is what a nextPageToken carries (see the header comment).
type calendarPageToken struct {
	Page  string `json:"p"`
	Start string `json:"s,omitempty"`
	End   string `json:"e,omitempty"`
	Query string `json:"q,omitempty"`
}

func (s *Service) parseCalendarRequest(params map[string]any, graph bool) (calendarRequest, error) {
	var req calendarRequest
	loc, err := calendarLocation()
	if err != nil {
		return req, fmt.Errorf("calendar time zone %s is unavailable: %w", calendarZoneName, err)
	}
	req.Local = loc

	rawToken, err := calendarPageTokenParam(params)
	if err != nil {
		return req, err
	}
	if rawToken != "" {
		token, err := decodeCalendarPageToken(rawToken)
		if err != nil {
			return req, err
		}
		req.Page, req.Query = token.Page, token.Query
		// decodeCalendarPageToken has already checked both bounds.
		start, _ := time.Parse(time.RFC3339, token.Start)
		end, _ := time.Parse(time.RFC3339, token.End)
		req.Start, req.End = start.UTC(), end.UTC()
	} else {
		if req.Query, err = mailTextParam(params, "query", calendarQueryMaxRunes); err != nil {
			return req, err
		}
		if graph {
			// Outlook is filtered here, by whitespace-separated terms. Quotes are
			// not part of a term, and a query of nothing but quotes is no query.
			req.Query = strings.Join(calendarQueryTerms(req.Query), " ")
		}
		since, hasSince, err := calendarBoundParam(params, "since", false, loc)
		if err != nil {
			return req, err
		}
		until, hasUntil, err := calendarBoundParam(params, "until", true, loc)
		if err != nil {
			return req, err
		}
		if req.Start, req.End, err = calendarResolveWindow(s.clock(), since, until, hasSince, hasUntil, req.Query != ""); err != nil {
			return req, err
		}
	}

	fallback := calendarDefaultMaxResults
	if graph && req.Query != "" {
		fallback = calendarSearchPageSize
	}
	req.PageSize = limitParam(params, "maxResults", fallback, calendarMaxResultsCap)
	return req, nil
}

// calendarBoundParam reads since or until. A bare date is a whole local day: it
// is that day's first moment for since, and for until the first moment of the
// next day, so the day itself is included.
func calendarBoundParam(params map[string]any, key string, endOfDay bool, loc *time.Location) (time.Time, bool, error) {
	raw, err := mailStringParam(params, key)
	if err != nil || raw == "" {
		return time.Time{}, false, err
	}
	invalid := fmt.Errorf("%s must be a date (YYYY-MM-DD) or a date and time (YYYY-MM-DDTHH:MM in local time, or RFC 3339)", key)
	if day, err := time.Parse(calendarDateLayout, raw); err == nil {
		if !calendarYearInRange(day.Year()) {
			return time.Time{}, false, invalid
		}
		offset := 0
		if endOfDay {
			offset = 1
		}
		return time.Date(day.Year(), day.Month(), day.Day()+offset, 0, 0, 0, 0, loc).UTC(), true, nil
	}
	if moment, err := time.Parse(time.RFC3339, raw); err == nil {
		if !calendarYearInRange(moment.Year()) {
			return time.Time{}, false, invalid
		}
		return moment.UTC(), true, nil
	}
	for _, layout := range []string{"2006-01-02T15:04:05", "2006-01-02T15:04"} {
		if moment, err := time.ParseInLocation(layout, raw, loc); err == nil {
			if !calendarYearInRange(moment.Year()) {
				return time.Time{}, false, invalid
			}
			return moment.UTC(), true, nil
		}
	}
	return time.Time{}, false, invalid
}

func calendarYearInRange(year int) bool {
	return year >= 1970 && year <= 2200
}

// calendarResolveWindow settles the window from what was given and the clock.
// Given only since, the window runs a default span from it; given only until,
// it runs from the default start (now, or 30 days back for a search) to it, or
// back a default span from it when that start is not before it.
func calendarResolveWindow(now, since, until time.Time, hasSince, hasUntil, hasQuery bool) (time.Time, time.Time, error) {
	const day = 24 * time.Hour
	back, ahead := 0, calendarDefaultAheadDays
	if hasQuery {
		back, ahead = calendarSearchBackDays, calendarSearchAheadDays
	}
	span := time.Duration(back+ahead) * day
	start, end := since, until
	switch {
	case !hasSince && !hasUntil:
		start, end = now.Add(-time.Duration(back)*day), now.Add(time.Duration(ahead)*day)
	case hasSince && !hasUntil:
		end = start.Add(span)
	case !hasSince && hasUntil:
		if start = now.Add(-time.Duration(back) * day); !start.Before(end) {
			start = end.Add(-span)
		}
	}
	if !end.After(start) {
		return time.Time{}, time.Time{}, fmt.Errorf("until must be after since")
	}
	if end.Sub(start) > calendarMaxSpanDays*day {
		return time.Time{}, time.Time{}, fmt.Errorf("the window is longer than %d days; narrow since and until", calendarMaxSpanDays)
	}
	return start.UTC(), end.UTC(), nil
}

func calendarPageTokenParam(params map[string]any) (string, error) {
	raw, err := mailStringParam(params, "pageToken")
	if err != nil || raw == "" {
		return "", err
	}
	if len(raw) > calendarTokenMaxBytes {
		return "", fmt.Errorf("pageToken is too long")
	}
	if calendarHasControl(raw) {
		return "", fmt.Errorf("pageToken contains a control character")
	}
	return raw, nil
}

func calendarHasControl(value string) bool {
	for _, r := range value {
		if unicode.IsControl(r) {
			return true
		}
	}
	return false
}

func encodeCalendarPageToken(token calendarPageToken) string {
	// A struct of strings always marshals.
	data, _ := json.Marshal(token)
	return calendarTokenPrefix + base64.RawURLEncoding.EncodeToString(data)
}

// decodeCalendarPageToken admits a token only if it was built the way
// encodeCalendarPageToken builds one and every part of it passes the checks the
// matching request parameter would have passed.
func decodeCalendarPageToken(raw string) (calendarPageToken, error) {
	invalid := fmt.Errorf("pageToken is not a calendar page token from this connection")
	var token calendarPageToken
	if !strings.HasPrefix(raw, calendarTokenPrefix) {
		return token, invalid
	}
	data, err := base64.RawURLEncoding.DecodeString(strings.TrimPrefix(raw, calendarTokenPrefix))
	if err != nil {
		return token, invalid
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&token); err != nil {
		return token, invalid
	}
	if _, err := decoder.Token(); err != io.EOF {
		return token, invalid
	}
	if token.Page == "" || len(token.Page) > calendarTokenMaxProvider || calendarHasControl(token.Page) {
		return token, invalid
	}
	start, startErr := time.Parse(time.RFC3339, token.Start)
	end, endErr := time.Parse(time.RFC3339, token.End)
	if startErr != nil || endErr != nil || !end.After(start) || end.Sub(start) > calendarMaxSpanDays*24*time.Hour {
		return token, invalid
	}
	if utf8.RuneCountInString(token.Query) > calendarQueryMaxRunes || calendarHasControl(token.Query) {
		return token, invalid
	}
	return token, nil
}

// calendarNextToken wraps the provider's own next-page value for the caller. It
// returns false when the provider's value is too long to be wrapped.
func calendarNextToken(req calendarRequest, providerPage string) (string, bool) {
	if providerPage == "" || len(providerPage) > calendarTokenMaxProvider || calendarHasControl(providerPage) {
		return "", false
	}
	return encodeCalendarPageToken(calendarPageToken{
		Page:  providerPage,
		Start: req.Start.UTC().Format(time.RFC3339),
		End:   req.End.UTC().Format(time.RFC3339),
		Query: req.Query,
	}), true
}

// calendarAnnotate adds what the caller needs to know about the request that
// was really made: the window (it is defaulted, so it is not otherwise visible)
// and the query that was applied.
func calendarAnnotate(result map[string]any, req calendarRequest) {
	result["window"] = map[string]any{
		"start":    req.Start.In(req.Local).Format(time.RFC3339),
		"end":      req.End.In(req.Local).Format(time.RFC3339),
		"timeZone": calendarZoneName,
	}
	if req.Query != "" {
		result["query"] = req.Query
	}
}

// --- Outlook (Microsoft Graph) ------------------------------------------------

func (s *Service) microsoftCalendarEvents(ctx context.Context, token, base string, params map[string]any) (any, error) {
	req, err := s.parseCalendarRequest(params, true)
	if err != nil {
		return nil, err
	}
	var endpoint string
	if req.Page != "" {
		// The link carries the whole previous query (window, $select, $top and
		// $skip), so it is replayed as is. The Prefer header is not part of it
		// and is sent again below.
		if endpoint, err = s.graphCalendarPageURL(req.Page); err != nil {
			return nil, err
		}
	} else {
		endpoint = graphCalendarURL(base, req)
	}
	result, err := s.getBearerMap(ctx, token, endpoint, map[string]string{"Prefer": calendarGraphPrefer})
	if err != nil {
		return nil, err
	}
	s.finishGraphCalendar(result, req)
	return result, nil
}

// graphCalendarURL builds the first-page request: the occurrences in the window,
// oldest first, with no $search (events do not support it) and no $filter.
func graphCalendarURL(base string, req calendarRequest) string {
	values := url.Values{
		"startDateTime": {req.Start.UTC().Format(time.RFC3339)},
		"endDateTime":   {req.End.UTC().Format(time.RFC3339)},
		"$orderby":      {"start/dateTime"},
		"$top":          {req.PageSize},
		"$select":       {graphCalendarSelect},
	}
	return base + "/me/calendarView?" + values.Encode()
}

// graphCalendarPageURL admits a page link only if it is exactly the kind of
// link Graph hands out for the calendar view on the configured Graph host. The
// link comes back from the caller, who may be an agent steered by event text,
// and the request that follows carries the user's bearer token, so a link to
// another host, a plaintext link, another user's calendar, a named calendar or
// any other resource under /me is refused before any request is made.
func (s *Service) graphCalendarPageURL(raw string) (string, error) {
	invalid := fmt.Errorf("pageToken is not a calendar page token from this connection")
	link, err := url.Parse(raw)
	if err != nil || link.Scheme != "https" || link.Opaque != "" || link.User != nil || link.Fragment != "" || link.Host == "" {
		return "", invalid
	}
	configured, err := url.Parse(strings.TrimRight(s.cfg.MicrosoftGraphBaseURL, "/"))
	if err != nil || configured.Host == "" || !strings.EqualFold(link.Host, configured.Host) {
		return "", invalid
	}
	if !strings.EqualFold(link.Path, strings.TrimRight(configured.Path, "/")+"/v1.0/me/calendarView") {
		return "", invalid
	}
	return raw, nil
}

// finishGraphCalendar applies the local search filter, replaces Graph's
// @odata.nextLink by a page token, and states the window and query in effect.
func (s *Service) finishGraphCalendar(result map[string]any, req calendarRequest) {
	if terms := calendarQueryTerms(req.Query); len(terms) > 0 {
		if rows, ok := result["value"].([]any); ok {
			kept := make([]any, 0, len(rows))
			for _, row := range rows {
				if event, ok := row.(map[string]any); ok && graphEventMatches(event, terms) {
					kept = append(kept, row)
				}
			}
			result["value"] = kept
			result["scanned"] = len(rows)
			result["filteredOut"] = len(rows) - len(kept)
		}
	}
	next := stringFromAny(result["@odata.nextLink"])
	delete(result, "@odata.nextLink")
	if next != "" {
		// More rows exist even when the link cannot be handed on, and the caller
		// should be able to tell a list that ended from one that was cut short.
		result["moreAvailable"] = true
		if _, err := s.graphCalendarPageURL(next); err == nil {
			if wrapped, ok := calendarNextToken(req, next); ok {
				result["nextPageToken"] = wrapped
			}
		}
	}
	calendarAnnotate(result, req)
}

// calendarQueryTerms splits a search into lower-case terms. Quotes are dropped.
func calendarQueryTerms(query string) []string {
	return strings.Fields(strings.ToLower(strings.ReplaceAll(query, `"`, " ")))
}

// graphEventMatches is the local search: an event matches when every term is
// contained, case-insensitively, somewhere in its subject, location, organizer
// (name or address), attendees (names or addresses) or body preview. It does
// not depend on Graph supporting any search syntax on events.
func graphEventMatches(event map[string]any, terms []string) bool {
	text := graphEventSearchText(event)
	for _, term := range terms {
		if !strings.Contains(text, term) {
			return false
		}
	}
	return true
}

func graphEventSearchText(event map[string]any) string {
	parts := []string{}
	add := func(value any) {
		if text, ok := value.(string); ok && strings.TrimSpace(text) != "" {
			parts = append(parts, strings.ToLower(text))
		}
	}
	addAddress := func(entry any) {
		if holder, ok := entry.(map[string]any); ok {
			if email, ok := holder["emailAddress"].(map[string]any); ok {
				add(email["name"])
				add(email["address"])
			}
		}
	}
	add(event["subject"])
	add(event["bodyPreview"])
	if location, ok := event["location"].(map[string]any); ok {
		add(location["displayName"])
	}
	addAddress(event["organizer"])
	if attendees, ok := event["attendees"].([]any); ok {
		for _, attendee := range attendees {
			addAddress(attendee)
		}
	}
	return strings.Join(parts, "\n")
}

// --- Google Calendar ----------------------------------------------------------

func (s *Service) googleCalendarEvents(ctx context.Context, token, base string, params map[string]any) (any, error) {
	req, err := s.parseCalendarRequest(params, false)
	if err != nil {
		return nil, err
	}
	values := url.Values{
		"timeMin":      {req.Start.UTC().Format(time.RFC3339)},
		"timeMax":      {req.End.UTC().Format(time.RFC3339)},
		"singleEvents": {"true"},
		"orderBy":      {"startTime"},
		"maxResults":   {req.PageSize},
		"timeZone":     {calendarZoneName},
		"fields":       {googleCalendarFields},
	}
	if req.Query != "" {
		values.Set("q", req.Query)
	}
	if req.Page != "" {
		values.Set("pageToken", req.Page)
	}
	result, err := s.getBearerMap(ctx, token, base+"/calendar/v3/calendars/primary/events?"+values.Encode(), nil)
	if err != nil {
		return nil, err
	}
	next := stringFromAny(result["nextPageToken"])
	delete(result, "nextPageToken")
	if next != "" {
		if wrapped, ok := calendarNextToken(req, next); ok {
			result["nextPageToken"] = wrapped
		} else {
			result["moreAvailable"] = true
		}
	}
	// With a fields mask Google answers {} when nothing matches; the picker
	// reads result.items.
	if _, ok := result["items"].([]any); !ok {
		result["items"] = []any{}
	}
	calendarAnnotate(result, req)
	return result, nil
}
