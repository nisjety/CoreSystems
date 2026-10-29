package publicsignals

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode"
)

// One serial process-wide gate covers metadata and index requests alike. Never
// parallelize against the free public index server or bulk-fetch archive data.
var commonCrawlGate = &requestGate{interval: 2 * time.Second, slot: make(chan struct{}, 1)}

type requestGate struct {
	interval time.Duration
	slot     chan struct{}
	last     time.Time
}

func (g *requestGate) acquire(ctx context.Context) (func(), bool) {
	select {
	case g.slot <- struct{}{}:
	case <-ctx.Done():
		return nil, false
	}
	if delay := time.Until(g.last.Add(g.interval)); delay > 0 {
		timer := time.NewTimer(delay)
		select {
		case <-timer.C:
		case <-ctx.Done():
			timer.Stop()
			<-g.slot
			return nil, false
		}
	}
	if ctx.Err() != nil {
		<-g.slot
		return nil, false
	}
	g.last = time.Now()
	return func() { <-g.slot }, true
}
func (s *Service) archiveRequest(ctx context.Context, path string, max int64) ([]byte, string, int) {
	release, ok := s.gate.acquire(ctx)
	if !ok {
		return nil, contextReason(ctx), 0
	}
	defer release()
	req, err := http.NewRequestWithContext(ctx, "GET", "https://index.commoncrawl.org"+path, nil)
	if err != nil {
		return nil, "source_unavailable", 0
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "Integration-Core-PublicSignals/1.0")
	return s.requestResponse(req, max)
}

var indexPattern = regexp.MustCompile(`^CC-MAIN-([0-9]{4})-([0-9]{2})$`)

func validIndex(id string, now time.Time) bool {
	parts := indexPattern.FindStringSubmatch(id)
	if parts == nil {
		return false
	}
	year, _ := strconv.Atoi(parts[1])
	week, _ := strconv.Atoi(parts[2])
	return year >= 2008 && year <= now.UTC().Year() && week >= 1 && week <= 53
}

// Only public collection metadata is cached. The context-aware lock coalesces
// concurrent refreshes; expired indexes are never used after a failed refresh.
func (s *Service) archiveIndex(ctx context.Context) (string, string) {
	select {
	case s.indexSlot <- struct{}{}:
	case <-ctx.Done():
		return "", contextReason(ctx)
	}
	defer func() { <-s.indexSlot }()
	if s.latestIndex != "" && s.now().Before(s.indexExpires) {
		return s.latestIndex, ""
	}
	raw, reason, _ := s.archiveRequest(ctx, "/collinfo.json", 1<<20)
	if reason != "" {
		if reason == "no_data" {
			reason = "source_unavailable"
		}
		return "", reason
	}
	var collections []struct {
		ID string `json:"id"`
	}
	if json.Unmarshal(raw, &collections) != nil || len(collections) > 512 {
		return "", "source_unavailable"
	}
	index, previous := "", ""
	for _, collection := range collections {
		if validIndex(collection.ID, s.now()) && collection.ID > index {
			previous, index = index, collection.ID
		} else if validIndex(collection.ID, s.now()) && collection.ID < index && collection.ID > previous {
			previous = collection.ID
		}
	}
	if index == "" {
		return "", "source_unavailable"
	}
	s.latestIndex, s.previousIndex, s.indexExpires = index, previous, s.now().Add(30*time.Minute)
	return index, ""
}

func (s *Service) archiveQuery(ctx context.Context, path string) ([]byte, string, int) {
	raw, reason, status := s.archiveRequest(ctx, path, maxResponse)
	if status != 502 && status != 504 {
		return raw, reason, status
	}
	// Retry only an explicit transient gateway response, once, through the same
	// serialized pacing gate. Never retry throttles, metadata, or malformed data.
	if deadline, ok := ctx.Deadline(); ok && time.Until(deadline) <= s.gate.interval {
		return raw, reason, status
	}
	if ctx.Err() != nil {
		return nil, contextReason(ctx), status
	}
	raw, reason, status = s.archiveRequest(ctx, path, maxResponse)
	return raw, reason, status
}

func (s *Service) archive(ctx context.Context, site string) CommonCrawl {
	return s.archiveWithIndex(ctx, site, nil)
}

func (s *Service) archiveWithIndex(ctx context.Context, site string, selected func(string)) CommonCrawl {
	index, reason := s.archiveIndex(ctx)
	if reason != "" {
		return unavailableArchive(reason)
	}
	if selected != nil {
		selected(index)
	}
	result := s.archiveCurrentHosts(ctx, site, index)
	if result.Status == "connected" || result.Reason != "timeout" || ctx.Err() != nil {
		return result
	}
	select {
	case s.indexSlot <- struct{}{}:
	case <-ctx.Done():
		return result
	}
	previous := s.previousIndex
	<-s.indexSlot
	// At most one older archive query, and only following a transient timeout.
	// Its empty response cannot erase uncertainty about the newer archive.
	if previous != "" && previous < index {
		older, _ := s.archiveHostBound(ctx, site, previous, 10*time.Second)
		if older.Status == "connected" {
			return older
		}
	}
	return result
}

func (s *Service) archiveCurrentHosts(ctx context.Context, site, index string) CommonCrawl {
	result, discover := s.archiveHost(ctx, site, index)
	if !discover || ctx.Err() != nil {
		return result
	}
	if alternate := cruxAlternateSite(site); alternate != "" {
		next, _ := s.archiveHost(ctx, alternate, index)
		// A successful alternate carries its own host provenance. If one host
		// failed, the other host's empty response cannot prove absent coverage.
		if next.Status == "connected" || result.Reason == "no_data" || next.Reason != "no_data" {
			return next
		}
	}
	return result
}

func (s *Service) archiveHost(parent context.Context, site, index string) (CommonCrawl, bool) {
	return s.archiveHostBound(parent, site, index, 8*time.Second)
}

func (s *Service) archiveHostBound(parent context.Context, site, index string, budget time.Duration) (CommonCrawl, bool) {
	// Bound each candidate so a slow host query leaves time for its counterpart.
	ctx, cancel := context.WithTimeout(parent, budget)
	defer cancel()
	failure := func(reason string) CommonCrawl {
		result := unavailableArchive(reason)
		result.Index = index
		return result
	}
	u, _ := url.Parse(site)
	query := url.Values{"url": {u.Hostname()}, "matchType": {"host"}, "output": {"json"}, "limit": {"20"}, "fl": {"url,timestamp,status"}}
	raw, reason, status := s.archiveQuery(ctx, "/"+index+"-index?"+query.Encode())
	if reason != "" {
		return failure(reason), reason == "no_data" || reason == "timeout" || status == 500 || status == 502
	}
	result := CommonCrawl{Status: "connected", ReportingHost: u.Hostname(), Index: index, Captures: []Capture{}, Limit: 20}
	alternateHost := ""
	if alternate := cruxAlternateSite(site); alternate != "" {
		alternateURL, _ := url.Parse(alternate)
		alternateHost = alternateURL.Hostname()
	}
	byHost := map[string][]Capture{}
	scanner := bufio.NewScanner(bytes.NewReader(raw))
	scanner.Buffer(make([]byte, 4096), maxResponse)
	seen := map[string]bool{}
	lines := 0
	for scanner.Scan() {
		line := bytes.TrimSpace(scanner.Bytes())
		if len(line) == 0 {
			continue
		}
		lines++
		if lines > 20 {
			result.Truncated = true
			break
		}
		capture, ok := parseCapture(line, u.Hostname(), s.now())
		captureHost := u.Hostname()
		// Common Crawl's canonical host index can return captures for the exact
		// www counterpart in the same query. Validate those captures against
		// their actual host; never widen this to other subdomains.
		if !ok && alternateHost != "" {
			capture, ok = parseCapture(line, alternateHost, s.now())
			captureHost = alternateHost
		}
		if !ok {
			return failure("source_unavailable"), false
		}
		key := capture.URL + "\n" + capture.CapturedAt + "\n" + strconv.Itoa(capture.HTTPStatus)
		if !seen[key] {
			byHost[captureHost] = append(byHost[captureHost], capture)
			seen[key] = true
		}
	}
	if scanner.Err() != nil {
		return failure("source_unavailable"), false
	}
	result.Captures = byHost[u.Hostname()]
	if len(result.Captures) == 0 && len(byHost[alternateHost]) > 0 {
		result.ReportingHost = alternateHost
		result.Captures = byHost[alternateHost]
	}
	// A single report describes one measured host. Prefer the requested host
	// when both appear and disclose that the bounded result omitted records.
	result.Truncated = result.Truncated || len(byHost) > 1
	if len(result.Captures) == 0 {
		result = unavailableArchive("no_data")
		result.Index = index
		return result, true
	}
	// The provider limit cannot establish whether more records exist. Surface
	// possible truncation rather than claiming complete archive coverage.
	result.Truncated = result.Truncated || lines >= 20
	return result, false
}
func parseCapture(raw []byte, host string, now time.Time) (Capture, bool) {
	var row struct{ URL, Timestamp, Status string }
	if json.Unmarshal(raw, &row) != nil || len(row.URL) > 2048 || len(row.Timestamp) != 14 || len(row.Status) != 3 {
		return Capture{}, false
	}
	u, err := url.Parse(row.URL)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Host != host || u.User != nil || u.Port() != "" || u.Fragment != "" || u.Opaque != "" || u.String() != row.URL || strings.ContainsAny(row.URL, "\\\r\n\t ") || !publicArchivePath(u.EscapedPath()) {
		return Capture{}, false
	}
	when, err := time.Parse("20060102150405", row.Timestamp)
	if err != nil || when.Format("20060102150405") != row.Timestamp || when.After(now) || when.Year() < 2008 {
		return Capture{}, false
	}
	status, err := strconv.Atoi(row.Status)
	if err != nil || status < 100 || status > 599 || strconv.Itoa(status) != row.Status {
		return Capture{}, false
	}
	// Query values can contain personal data or credentials. Archive research
	// exposes only the path, never query values, raw CDX metadata or WARC bodies.
	u.RawQuery = ""
	u.ForceQuery = false
	if u.Path == "" {
		u.Path = "/"
	}
	return Capture{URL: u.String(), CapturedAt: when.UTC().Format(time.RFC3339), HTTPStatus: status}, true
}

// Public archive research excludes account, authentication, transaction and
// administrative paths, including repeatedly percent-encoded route segments.
// This is a conservative privacy boundary, not a claim every remaining page is
// free of personal information. Raw historical content is never downloaded.
func publicArchivePath(path string) bool {
	for range 4 {
		if strings.ContainsRune(path, '\\') || strings.ContainsFunc(path, unicode.IsControl) {
			return false
		}
		decoded, err := url.PathUnescape(path)
		if err != nil {
			return false
		}
		if decoded == path {
			break
		}
		path = decoded
	}
	if decoded, err := url.PathUnescape(path); err != nil || decoded != path || strings.ContainsRune(path, '\\') || strings.ContainsFunc(path, unicode.IsControl) {
		return false
	}
	for _, segment := range strings.Split(strings.ToLower(path), "/") {
		segment = strings.SplitN(segment, ";", 2)[0]
		switch segment {
		case ".", "..", "auth", "oauth", "login", "logout", "signin", "sign-in", "signup", "sign-up", "register", "callback", "magic-link", "account", "accounts", "profile", "settings", "admin", "api", "dashboard", "wp-admin", "wp-login.php", "session", "sessions", "password", "reset", "reset-password", "recover", "verify", "verification", "invite", "invitations", "checkout", "cart", "payment", "payments", "billing", "token", "tokens", "secret", "secrets", "unsubscribe":
			return false
		}
	}
	return true
}
