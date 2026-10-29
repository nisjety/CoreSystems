package demo

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/triodelab/anbud/internal/match"
	"golang.org/x/text/collate"
	"golang.org/x/text/language"
)

var ErrMatchCursor = errors.New("invalid matching cursor")
var ErrMatchSnapshotExpired = errors.New("matching snapshot expired")

const maxMatchSnapshotBytes = 32 << 20
const maxMatchSnapshots = 8

type matchPageSnapshot struct {
	profile            match.Profile
	id, key, org, user string
	generation         int64
	created, expires   time.Time
	data               Matches
	rows               []Result
	trackedKey         string
	bytes              int
}
type matchPageCursor struct {
	ID     string `json:"id"`
	Offset int    `json:"offset"`
}

func validMatchFilters(r Request) bool {
	f := r.Filters
	return len(f.Query) <= 200 && len(f.Buyer) <= 200 && len(f.Region) <= 8 && len(f.CPV) <= 10 &&
		(r.Sort == "" || r.Sort == "relevance" || r.Sort == "deadline" || r.Sort == "title") &&
		(f.Status == "" || f.Status == "all" || f.Status == "active" || f.Status == "expired") &&
		(f.Deadline == "" || f.Deadline == "all" || f.Deadline == "soon") &&
		(f.Verdict == "" || f.Verdict == "all" || f.Verdict == "pass" || f.Verdict == "conditional" || f.Verdict == "fail")
}
func digest(value any) string {
	body, _ := json.Marshal(value)
	return fmt.Sprintf("%x", sha256.Sum256(body))
}
func matchingEvidenceKey(e map[string]match.Evidence, d *DocumentsStatus) string {
	return digest(struct {
		E map[string]match.Evidence
		D *DocumentsStatus
	}{e, d})
}
func matchSearchKey(r Request) string {
	r.Cursor = ""
	r.Scan = 0
	r.User = ""
	r.Org = ""
	r.Profile.SEOKeywords = nil
	r.Profile.UserKeywords = nil
	if r.Limit <= 0 {
		r.Limit = DefaultLimit
	}
	if r.Sort == "" {
		r.Sort = "relevance"
	}
	return digest(r)
}
func trackedResultsKey(rows []Result) string {
	ids := make([]string, 0, len(rows))
	for _, r := range rows {
		ids = append(ids, r.NoticeID)
	}
	sort.Strings(ids)
	return digest(ids)
}
func (s *Service) matchingGeneration(ctx context.Context) (int64, error) {
	if source, ok := s.notices.(interface {
		CatalogueGeneration(context.Context) (int64, error)
	}); ok {
		return source.CatalogueGeneration(ctx)
	}
	return 0, nil
}
func encodeMatchPage(id string, offset int) string {
	body, _ := json.Marshal(matchPageCursor{id, offset})
	return base64.RawURLEncoding.EncodeToString(body)
}
func decodeMatchPage(cursor string) (matchPageCursor, error) {
	var c matchPageCursor
	if len(cursor) > 512 {
		return c, ErrMatchCursor
	}
	body, err := base64.RawURLEncoding.DecodeString(cursor)
	if err != nil || json.Unmarshal(body, &c) != nil || len(c.ID) != 32 || c.Offset < 0 {
		return c, ErrMatchCursor
	}
	return c, nil
}

// Matches preserves the legacy response for embedded callers. The paginated feed
// keeps a bounded, short-lived immutable result snapshot scoped to company + user.
func (s *Service) Matches(ctx context.Context, r Request) (Matches, error) {
	if !r.Paginate || r.TrackedOnly {
		return s.computeMatches(ctx, r)
	}
	if !validMatchFilters(r) {
		return Matches{}, ErrMatchCursor
	}
	size := r.Limit
	if size <= 0 {
		size = DefaultLimit
	}
	if size > MaxLimit {
		return Matches{}, ErrMatchCursor
	}
	if r.Cursor != "" {
		cursor, err := decodeMatchPage(r.Cursor)
		if err != nil {
			return Matches{}, err
		}
		s.matchPagesMu.Lock()
		snapshot := s.matchPages[cursor.ID]
		s.matchPagesMu.Unlock()
		if snapshot == nil {
			return Matches{}, ErrMatchSnapshotExpired
		}
		if snapshot.org != r.Org || snapshot.user != r.User || snapshot.key != matchSearchKey(r) || cursor.Offset%size != 0 {
			return Matches{}, ErrMatchCursor
		}
		generation, err := s.matchingGeneration(ctx)
		if err != nil {
			return Matches{}, err
		}
		if generation != snapshot.generation || !s.now().Before(snapshot.expires) {
			s.forgetMatchPage(snapshot.id)
			return Matches{}, ErrMatchSnapshotExpired
		}
		// Uploaded evidence and saved pipeline membership remain live, never stale
		// simply because the public source generation has not changed.
		if s.evidence != nil && r.Org != "" {
			candidates := make([]candidate, len(snapshot.data.assessedIDs))
			for i, id := range snapshot.data.assessedIDs {
				candidates[i].id = id
			}
			evidence, status := s.documents(ctx, r.Org, candidates)
			if matchingEvidenceKey(evidence, status) != snapshot.data.evidenceKey {
				s.forgetMatchPage(snapshot.id)
				return Matches{}, ErrMatchSnapshotExpired
			}
		}
		company, _ := s.company(ctx, r.Profile)
		if digest(company) != digest(snapshot.data.Company) {
			s.forgetMatchPage(snapshot.id)
			return Matches{}, ErrMatchSnapshotExpired
		}
		_, profile := s.companyForTracked(snapshot.profile, company)
		tracked, err := s.trackedResults(ctx, r, profile, s.now())
		if err != nil {
			return Matches{}, err
		}
		if trackedResultsKey(tracked) != snapshot.trackedKey {
			s.forgetMatchPage(snapshot.id)
			return Matches{}, ErrMatchSnapshotExpired
		}
		current, err := s.matchingGeneration(ctx)
		if err != nil {
			return Matches{}, err
		}
		if current != generation {
			s.forgetMatchPage(snapshot.id)
			return Matches{}, ErrMatchSnapshotExpired
		}
		return snapshot.page(cursor.Offset, size, tracked)
	}
	generation, err := s.matchingGeneration(ctx)
	if err != nil {
		return Matches{}, err
	}
	data, err := s.computeMatches(ctx, r)
	if err != nil {
		return Matches{}, err
	}
	current, err := s.matchingGeneration(ctx)
	if err != nil {
		return Matches{}, err
	}
	if current != generation {
		return Matches{}, ErrMatchSnapshotExpired
	}
	trackedIDs := map[string]bool{}
	for _, row := range data.TrackedResults {
		trackedIDs[row.NoticeID] = true
	}
	inbox := make([]Result, 0, len(data.Results))
	for _, row := range data.Results {
		if !trackedIDs[row.NoticeID] {
			inbox = append(inbox, row)
		}
	}
	data.InboxTotal = len(inbox)
	rows := filterMatchRows(inbox, r)
	if r.Sort == "title" {
		comparator := collate.New(language.Danish)
		sort.SliceStable(rows, func(i, j int) bool {
			cmp := comparator.CompareString(strings.ToLower(rows[i].Title), strings.ToLower(rows[j].Title))
			if cmp == 0 {
				return rows[i].NoticeID < rows[j].NoticeID
			}
			return cmp < 0
		})
	}
	if r.Sort == "deadline" {
		sort.SliceStable(rows, func(i, j int) bool {
			a, b := rows[i].Deadline, rows[j].Deadline
			if a == nil || b == nil {
				return a != nil && b == nil
			}
			if a.At.Equal(b.At) {
				return rows[i].NoticeID < rows[j].NoticeID
			}
			return a.At.Before(b.At)
		})
	}
	token := make([]byte, 24)
	if _, err := rand.Read(token); err != nil {
		return Matches{}, err
	}
	now := s.now()
	expires := data.validUntil
	if loc, err := time.LoadLocation("Europe/Oslo"); err == nil {
		at := now.In(loc)
		midnight := time.Date(at.Year(), at.Month(), at.Day()+1, 0, 0, 0, 0, loc)
		if midnight.Before(expires) {
			expires = midnight
		}
	}
	data.Results = nil
	encoded, _ := json.Marshal(rows)
	encodedData, _ := json.Marshal(data)
	snapshot := &matchPageSnapshot{id: base64.RawURLEncoding.EncodeToString(token), key: matchSearchKey(r), org: r.Org, user: r.User, generation: generation, created: now, expires: expires, data: data, rows: rows, trackedKey: trackedResultsKey(data.TrackedResults), bytes: len(encoded) + len(encodedData), profile: r.Profile}
	if snapshot.bytes > maxMatchSnapshotBytes {
		return Matches{}, ErrBusy
	}
	s.matchPagesMu.Lock()
	if s.matchPages == nil {
		s.matchPages = map[string]*matchPageSnapshot{}
	}
	total := snapshot.bytes
	for id, entry := range s.matchPages {
		if !now.Before(entry.expires) {
			delete(s.matchPages, id)
		} else {
			total += entry.bytes
		}
	}
	for len(s.matchPages) >= maxMatchSnapshots || total > maxMatchSnapshotBytes {
		var oldest *matchPageSnapshot
		for _, entry := range s.matchPages {
			if oldest == nil || entry.created.Before(oldest.created) {
				oldest = entry
			}
		}
		if oldest == nil {
			break
		}
		delete(s.matchPages, oldest.id)
		total -= oldest.bytes
	}
	s.matchPages[snapshot.id] = snapshot
	s.matchPagesMu.Unlock()
	return snapshot.page(0, size, data.TrackedResults)
}

// Apply the register's current blocked status without a second lookup.
func (s *Service) companyForTracked(p match.Profile, status *CompanyStatus) (*CompanyStatus, match.Profile) {
	if status != nil && status.CanBid != nil {
		p.Blocked = !*status.CanBid
	}
	return status, p
}
func (s *Service) forgetMatchPage(id string) {
	s.matchPagesMu.Lock()
	delete(s.matchPages, id)
	s.matchPagesMu.Unlock()
}
func (snapshot *matchPageSnapshot) page(offset, size int, tracked []Result) (Matches, error) {
	if offset > len(snapshot.rows) || (offset == len(snapshot.rows) && offset != 0) {
		return Matches{}, ErrMatchCursor
	}
	end := min(offset+size, len(snapshot.rows))
	out := snapshot.data
	out.Results = append([]Result{}, snapshot.rows[offset:end]...)
	out.TrackedResults = tracked
	out.TotalMatched = len(snapshot.rows)
	out.Page = offset/size + 1
	out.PageSize = size
	if end < len(snapshot.rows) {
		out.NextCursor = encodeMatchPage(snapshot.id, end)
	}
	if offset > 0 {
		out.PreviousCursor = encodeMatchPage(snapshot.id, max(0, offset-size))
	}
	return out, nil
}
func filterMatchRows(rows []Result, r Request) []Result {
	out := make([]Result, 0, len(rows))
	f := r.Filters
	request := CatalogueRequest{Query: f.Query, Buyer: f.Buyer, Region: f.Region, CPV: f.CPV, Status: f.Status, Deadline: f.Deadline}
	for _, row := range rows {
		if (f.Verdict == "" || f.Verdict == "all" || row.Verdict == f.Verdict) && catalogueMatches(row, request, time.Time{}) {
			out = append(out, row)
		}
	}
	return out
}
