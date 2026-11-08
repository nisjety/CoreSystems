package activities

import (
	"context"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// frontierBatchMax is the most items one enqueue_batch call takes. It mirrors
// ENQUEUE_BATCH_MAX in quarry-edge's queue_routes.rs.
const frontierBatchMax = 500

// FrontierBatchItem is one child link of a crawl page (WS14.S3).
type FrontierBatchItem struct {
	RequestID string `json:"request_id"`
	URL       string `json:"url"`
	Depth     uint32 `json:"depth"`
}

// FrontierEnqueueBatchInput is the children of one page, for one queue.
type FrontierEnqueueBatchInput struct {
	Queue string
	OrgID string
	Items []FrontierBatchItem
}

// FrontierEnqueueBatch enqueues a page's children (WS14.S3). It sends at most
// 500 items per call, in order, and returns how many the queue took. A
// re-sent item keeps its request id, so the queue does not take it twice.
func (a *Activities) FrontierEnqueueBatch(ctx context.Context, in FrontierEnqueueBatchInput) (uint32, error) {
	const op = "activities.FrontierEnqueueBatch"
	var accepted uint32
	for start := 0; start < len(in.Items); start += frontierBatchMax {
		end := start + frontierBatchMax
		if end > len(in.Items) {
			end = len(in.Items)
		}
		body, err := json.Marshal(map[string]any{"org_id": in.OrgID, "items": in.Items[start:end]})
		if err != nil {
			return accepted, errs.New(errs.CategoryValidation, op, err).Temporal()
		}
		var envelope quarrycontracts.RESTEnvelope[struct {
			Accepted uint32 `json:"accepted"`
		}]
		if err := a.frontierCall(ctx, http.MethodPost, "/v1/internal/queues/"+in.Queue+"/enqueue_batch", in.OrgID, in.Queue, body, &envelope); err != nil {
			return accepted, err
		}
		if envelope.Data != nil {
			accepted += envelope.Data.Accepted
		}
	}
	return accepted, nil
}

// E3.7.6 canonical frontier identity (WS14). The Rust helper
// quarry_transform::canonical_url is the authority for a canonical URL. The functions
// below mirror its conservative folding for the plain ASCII subset and fail closed on
// anything else, so a URL the mirror refuses keeps the request id it has today.
// Nothing calls them yet: the enqueue sites (workflows.go durableFrontierRequestID and
// the URL they enqueue, crawl_scope.go) and the edge's seen route are the wiring. They
// switch together behind the canonical-frontier flag (see the E3.7.6 status file).

// frontierCanonicalMaxBytes mirrors quarry_transform::canonical_url::MAX_INPUT_BYTES.
const frontierCanonicalMaxBytes = 8 * 1024

// Bytes a canonical path or query may hold without percent-encoding, beside the
// unreserved set. Both are subsets of what the Rust helper leaves unencoded in that
// component, so an accepted URL canonicalises to the same string on both sides. The
// query leaves out `'`, which the Rust helper encodes for http and https.
const (
	frontierPathExtra  = "!$&'()*+,;=:@/%"
	frontierQueryExtra = "!$&()*+,;=:@/?%"
)

// CanonicalFrontierURL returns the conservative canonical form of a crawl URL and true,
// or "" and false when the mirror does not cover the URL. It folds the scheme and host
// case, the default port (80 for http, 443 for https), the fragment, one trailing dot of
// the host, dot segments, an empty path (to "/"), a `?` with no query, and the escapes of
// unreserved bytes (decoded; the other escapes keep their hex, upper-cased). It keeps
// apart what a server may serve differently: a trailing slash, www., index.html, query
// order and tracking parameters.
func CanonicalFrontierURL(raw string) (string, bool) {
	if raw == "" || len(raw) > frontierCanonicalMaxBytes || !plainFrontierASCII(raw) {
		return "", false
	}
	scheme, rest, found := strings.Cut(raw, "://")
	if !found {
		return "", false
	}
	scheme = strings.ToLower(scheme)
	if scheme != "http" && scheme != "https" {
		return "", false
	}
	// The fragment is not part of the resource identity.
	if i := strings.IndexByte(rest, '#'); i >= 0 {
		rest = rest[:i]
	}
	authority, pathQuery := rest, ""
	if i := strings.IndexAny(rest, "/?"); i >= 0 {
		authority, pathQuery = rest[:i], rest[i:]
	}
	host, port, ok := canonicalFrontierAuthority(scheme, authority)
	if !ok {
		return "", false
	}
	path, query, hasQuery := pathQuery, "", false
	if i := strings.IndexByte(pathQuery, '?'); i >= 0 {
		path, query, hasQuery = pathQuery[:i], pathQuery[i+1:], true
	}
	if !frontierCharsAllowed(path, frontierPathExtra) || !frontierCharsAllowed(query, frontierQueryExtra) {
		return "", false
	}
	out := scheme + "://" + host
	if port != "" {
		out += ":" + port
	}
	out += removeFrontierDotSegments(normalizeFrontierPercent(path))
	if hasQuery && query != "" {
		out += "?" + normalizeFrontierPercent(query)
	}
	return out, true
}

// canonicalFrontierAuthority folds the host and port of an authority. The scheme's
// default port is dropped. Userinfo, IPv6 literals and percent-escapes are refused.
func canonicalFrontierAuthority(scheme, authority string) (host, port string, ok bool) {
	if authority == "" || strings.ContainsAny(authority, "@[]%\\") || strings.Count(authority, ":") > 1 {
		return "", "", false
	}
	host = authority
	if i := strings.IndexByte(authority, ':'); i >= 0 {
		host = authority[:i]
		digits := authority[i+1:]
		if digits == "" || len(digits) > 5 || strings.Trim(digits, "0123456789") != "" {
			return "", "", false
		}
		n, err := strconv.Atoi(digits)
		if err != nil || n > 65535 {
			return "", "", false
		}
		defaultPort := (scheme == "http" && n == 80) || (scheme == "https" && n == 443)
		if !defaultPort {
			port = strconv.Itoa(n)
		}
	}
	host = strings.TrimSuffix(strings.ToLower(host), ".")
	if !frontierHostOK(host) {
		return "", "", false
	}
	return host, port, true
}

// frontierHostOK accepts a lower-case host of LDH labels that WHATWG reads as a domain
// and not as an IPv4 address: no empty label, no label that starts or ends with `-`, no
// `xn--` label, and a last label that is neither all digits nor `0x` prefixed.
func frontierHostOK(host string) bool {
	if host == "" || len(host) > 253 {
		return false
	}
	labels := strings.Split(host, ".")
	for _, label := range labels {
		if label == "" || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' || strings.HasPrefix(label, "xn--") {
			return false
		}
		for i := 0; i < len(label); i++ {
			c := label[i]
			if (c < 'a' || c > 'z') && (c < '0' || c > '9') && c != '-' {
				return false
			}
		}
	}
	last := labels[len(labels)-1]
	return strings.Trim(last, "0123456789") != "" && !strings.HasPrefix(last, "0x")
}

// plainFrontierASCII reports whether every byte is printable ASCII with no space.
func plainFrontierASCII(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] <= ' ' || s[i] >= 0x7f {
			return false
		}
	}
	return true
}

// frontierCharsAllowed reports whether every byte is unreserved or listed in extra.
func frontierCharsAllowed(s, extra string) bool {
	for i := 0; i < len(s); i++ {
		if !frontierUnreserved(s[i]) && strings.IndexByte(extra, s[i]) < 0 {
			return false
		}
	}
	return true
}

// frontierUnreserved is the RFC 3986 unreserved set: ALPHA, DIGIT, `-`, `.`, `_` and `~`.
func frontierUnreserved(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') ||
		c == '-' || c == '.' || c == '_' || c == '~'
}

// normalizeFrontierPercent decodes a %XX escape whose byte is unreserved and upper-cases
// the hex of every other escape, as the Rust helper does. A `%` that is not followed by
// two hex digits is kept as it is.
func normalizeFrontierPercent(in string) string {
	var b strings.Builder
	b.Grow(len(in))
	for i := 0; i < len(in); i++ {
		if in[i] == '%' && i+2 < len(in) {
			hi, lo := frontierHexValue(in[i+1]), frontierHexValue(in[i+2])
			if hi >= 0 && lo >= 0 {
				if v := byte(hi<<4 | lo); frontierUnreserved(v) {
					b.WriteByte(v)
				} else {
					b.WriteByte('%')
					b.WriteByte(frontierUpperHex(hi))
					b.WriteByte(frontierUpperHex(lo))
				}
				i += 2
				continue
			}
		}
		b.WriteByte(in[i])
	}
	return b.String()
}

// frontierHexValue is the value of one hex digit, or -1.
func frontierHexValue(c byte) int {
	switch {
	case c >= '0' && c <= '9':
		return int(c - '0')
	case c >= 'a' && c <= 'f':
		return int(c-'a') + 10
	case c >= 'A' && c <= 'F':
		return int(c-'A') + 10
	}
	return -1
}

// frontierUpperHex is the upper-case hex digit of a value from 0 to 15.
func frontierUpperHex(n int) byte {
	if n < 10 {
		return byte('0' + n)
	}
	return byte('A' + n - 10)
}

// removeFrontierDotSegments resolves `.` and `..` in an absolute path, as the Rust helper
// does for http and https: a `..` at the root is dropped, and a final `.` or `..` leaves a
// trailing slash. An empty path is "/".
func removeFrontierDotSegments(path string) string {
	if path == "" {
		return "/"
	}
	segments := strings.Split(strings.TrimPrefix(path, "/"), "/")
	out := make([]string, 0, len(segments))
	for i, seg := range segments {
		last := i == len(segments)-1
		switch seg {
		case ".":
			if last {
				out = append(out, "")
			}
		case "..":
			if len(out) > 0 {
				out = out[:len(out)-1]
			}
			if last {
				out = append(out, "")
			}
		default:
			out = append(out, seg)
		}
	}
	return "/" + strings.Join(out, "/")
}

// FrontierCanonicalRequestID is the request id of a crawl URL under the canonical
// frontier: the id of its canonical form when the mirror covers the URL, else the id of
// the URL as given. For a URL that is already canonical it is FrontierRequestID, so the
// ids of a run do not change for URLs that were canonical already.
func FrontierCanonicalRequestID(runID, raw string) string {
	if canonical, ok := CanonicalFrontierURL(raw); ok {
		return FrontierRequestID(runID, canonical)
	}
	return FrontierRequestID(runID, raw)
}

// FrontierSeenRequestIDs lists the request ids a fetch leaves marked seen (E3.7.6): the
// requested URL's id, then the final URL's id when the fetch landed on another canonical
// resource. It mirrors quarry_transform::canonical_url::Reconciliation::seen_request_ids.
// An empty final URL (the edge reported none) gives the requested id alone.
func FrontierSeenRequestIDs(runID, requested, final string) []string {
	first := FrontierCanonicalRequestID(runID, requested)
	if strings.TrimSpace(final) == "" {
		return []string{first}
	}
	if second := FrontierCanonicalRequestID(runID, final); second != first {
		return []string{first, second}
	}
	return []string{first}
}

// FrontierMarkSeenInput names request ids to record as already crawled in one queue,
// without enqueuing them: the final URL of a redirect (E3.7.6).
type FrontierMarkSeenInput struct {
	Queue      string   `json:"queue"`
	OrgID      string   `json:"org_id"`
	RequestIDs []string `json:"request_ids"`
}

// FrontierMarkSeen records request ids as seen in one queue, so a later enqueue of the
// same id is refused. Blank ids are skipped. It sends at most 500 ids per call and returns
// how many the queue recorded. Nothing calls it yet: it needs the edge route
// POST /v1/internal/queues/{queue}/seen, which is not built.
func (a *Activities) FrontierMarkSeen(ctx context.Context, in FrontierMarkSeenInput) (uint32, error) {
	const op = "activities.FrontierMarkSeen"
	ids := make([]string, 0, len(in.RequestIDs))
	for _, id := range in.RequestIDs {
		if strings.TrimSpace(id) != "" {
			ids = append(ids, id)
		}
	}
	var marked uint32
	for start := 0; start < len(ids); start += frontierBatchMax {
		end := start + frontierBatchMax
		if end > len(ids) {
			end = len(ids)
		}
		body, err := json.Marshal(map[string]any{"org_id": in.OrgID, "request_ids": ids[start:end]})
		if err != nil {
			return marked, errs.New(errs.CategoryValidation, op, err).Temporal()
		}
		var envelope quarrycontracts.RESTEnvelope[struct {
			Accepted uint32 `json:"accepted"`
		}]
		if err := a.frontierCall(ctx, http.MethodPost, "/v1/internal/queues/"+in.Queue+"/seen", in.OrgID, in.Queue, body, &envelope); err != nil {
			return marked, err
		}
		if envelope.Data != nil {
			marked += envelope.Data.Accepted
		}
	}
	return marked, nil
}
