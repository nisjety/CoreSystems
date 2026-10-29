package doffinsync

import (
	"errors"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

// errNegativeCount is the decode failure of a search response whose hit count is
// negative. Such a response cannot describe a day, so it is never trusted (E5).
var errNegativeCount = errors.New("negative hit count")

// pageResult is one Search call. err is the Search error for the caller to
// classify; a storage failure never reaches here, it is a halt.
type pageResult struct {
	resp doffin.SearchResponse
	err  error
}

// searchDay searches the first page and decides whether the day ends there.
// When it does not, it returns the distinct ids of the complete day and
// ended=false. When it does, rec holds the final state and ended=true.
func (r *run) searchDay(d time.Time, rec *DayRecord) ([]string, bool, *halt) {
	first, ended, h := r.fetch(d, 1, rec)
	if ended {
		return nil, true, h
	}
	total, accessible := int64(first.NumHitsTotal), int64(first.NumHitsAccessible)
	rec.Total, rec.Accessible = &total, &accessible
	switch {
	case accessible > maxAccessible:
		// Doffin caps the accessible count at maxAccessible, so a larger one is not a
		// day this sync understands. It must not set the paging loop without a bound.
		setDay(rec, StatePartial, CodeCountMismatch)
		return nil, true, nil
	case total > accessible:
		setDay(rec, StateCapped, CodeSaturated)
		return nil, true, nil
	case total < accessible:
		setDay(rec, StatePartial, CodeCountMismatch)
		return nil, true, nil
	}
	ids, ended, h := r.collect(d, first, total, accessible, rec)
	if ended {
		return nil, true, h
	}
	if total == 0 {
		// Two sweeps agree that the day holds no notice: the day is done.
		setDay(rec, StateDone, "")
		return nil, true, nil
	}
	return ids, false, nil
}

// dayQuery is what a sweep needs to know about the day it pages: the day, how many
// pages it spans, and the two counts that every page must repeat.
type dayQuery struct {
	day        time.Time
	pages      int
	total      int64
	accessible int64
}

// collect pages a day whose total equals its accessible count, in sweeps (D10, D-B3).
// Every page of every sweep must report the same counts. The ids are complete once
// at least two sweeps have run and their union holds exactly the total. Doffin does
// not keep tied hits in the same order between requests, so one sweep over several
// pages can repeat some ids and miss others (live test V2); the union of the sweeps
// still converges on the day. A day of one page has no page boundary to reorder
// across, so every sweep of it must hold the whole day by itself. A union above the
// total means the day changed under the sweeps, and the sweep stops at that page. A
// union still below the total after maxSweeps leaves the day partial. The downloads
// follow the union, in first-seen order.
//
// A union that reaches the total proves that every notice was seen only while the
// day's membership holds still during the sweeps. A withdrawal and an arrival in the
// same moments would leave the total unchanged and one notice unseen. A recheck of
// the day finds it, because it downloads whatever is not yet held.
func (r *run) collect(d time.Time, first doffin.SearchResponse, total, accessible int64, rec *DayRecord) ([]string, bool, *halt) {
	size := int64(r.e.pageSize)
	q := dayQuery{day: d, pages: int((accessible + size - 1) / size), total: total, accessible: accessible}
	union := newIDSet()
	resp := first
	for sweep := 1; sweep <= maxSweeps; sweep++ {
		if sweep > 1 {
			again, ended, h := r.fetch(d, 1, rec)
			if ended {
				return nil, true, h
			}
			resp = again
		}
		own, ended, h := r.sweep(q, resp, union, rec)
		if ended {
			return nil, true, h
		}
		switch {
		case q.pages == 1 && own != total:
			setDay(rec, StatePartial, CodeCountMismatch)
			return nil, true, nil
		case sweep >= 2 && int64(union.len()) == total:
			return union.ids, false, nil
		}
	}
	setDay(rec, StatePartial, CodeCountMismatch)
	return nil, true, nil
}

// sweep pages through a day from its first page, adds every hit id to union, and
// returns how many distinct ids this sweep alone saw. ended is true when the day has
// been finished, with its state set in rec: a page that reports other counts, an
// empty page before the last, or a union that has passed the total.
func (r *run) sweep(q dayQuery, first doffin.SearchResponse, union *idSet, rec *DayRecord) (int64, bool, *halt) {
	own := newIDSet()
	resp := first
	for n := 1; ; n++ {
		if n > 1 {
			next, ended, h := r.fetch(q.day, n, rec)
			if ended {
				return 0, true, h
			}
			resp = next
		}
		switch {
		case int64(resp.NumHitsTotal) != q.total || int64(resp.NumHitsAccessible) != q.accessible:
			setDay(rec, StatePartial, CodeCountChanged)
			return 0, true, nil
		case len(resp.Hits) == 0 && n < q.pages:
			setDay(rec, StatePartial, CodeCountMismatch)
			return 0, true, nil
		}
		own.add(resp.Hits)
		union.add(resp.Hits)
		if int64(union.len()) > q.total {
			setDay(rec, StatePartial, CodeCountChanged)
			return 0, true, nil
		}
		if n >= q.pages {
			return int64(own.len()), false, nil
		}
	}
}

// idSet is the set of distinct hit ids seen so far, kept in first-seen order.
type idSet struct {
	ids  []string
	seen map[string]struct{}
}

func newIDSet() *idSet { return &idSet{seen: make(map[string]struct{})} }

func (s *idSet) len() int { return len(s.ids) }

// add keeps the hit ids not seen before.
func (s *idSet) add(hits doffin.OneOrMany[doffin.Hit]) {
	for _, h := range hits {
		if _, dup := s.seen[h.ID]; dup {
			continue
		}
		s.seen[h.ID] = struct{}{}
		s.ids = append(s.ids, h.ID)
	}
}

// fetch searches page n. A search error ends the day through table S. ended is
// true when the day is finished, and h is set when the run must stop too.
func (r *run) fetch(d time.Time, n int, rec *DayRecord) (doffin.SearchResponse, bool, *halt) {
	res, h := r.page(d, n, rec)
	if h != nil {
		return doffin.SearchResponse{}, true, h
	}
	if res.err != nil {
		return doffin.SearchResponse{}, true, r.searchFailed(res.err, rec)
	}
	return res.resp, false, nil
}

// page searches one page and stores its response bytes before anything else
// looks at them. A storage failure is handled by storeFailed. A negative hit
// count is a decode failure (E5). Every successful page resets the
// consecutive-error count (D-B1).
func (r *run) page(d time.Time, n int, rec *DayRecord) (pageResult, *halt) {
	r.counters.Searches++
	page := n
	resp, err := r.e.deps.Search.Search(r.ctx, doffin.SearchParams{
		IssueDateFrom:  d,
		IssueDateTo:    d,
		NumHitsPerPage: r.e.pageSize,
		Page:           &page,
		SortBy:         sortBy,
	})
	if len(resp.Raw) > 0 {
		sp := SearchPage{
			QueryKey:    queryKey(d, r.e.pageSize),
			Page:        n,
			ContentType: resp.ContentType,
			Body:        []byte(resp.Raw),
			FetchedAt:   resp.FetchedAt,
		}
		if perr := r.e.deps.Pages.PutSearchPage(r.ctx, r.runID, sp); perr != nil {
			return pageResult{}, r.storeFailed(rec, "store search page", perr)
		}
	}
	if err == nil && (resp.NumHitsTotal < 0 || resp.NumHitsAccessible < 0) {
		err = &doffin.DecodeError{Operation: "search", Err: errNegativeCount}
	}
	if err == nil {
		r.consecutive = 0
	}
	return pageResult{resp: resp, err: err}, nil
}

// searchFailed applies table S. Every search error ends the day.
func (r *run) searchFailed(err error, rec *DayRecord) *halt {
	if r.ctx.Err() != nil {
		return r.cancelDay(rec, err)
	}
	var rl *doffin.RateLimitedError
	var de *doffin.DecodeError
	switch {
	case errors.Is(err, doffin.ErrUnauthorized):
		setDay(rec, StateFailed, CodeUnauthorized)
		return keyRejectedHalt()
	case errors.As(err, &rl):
		setDay(rec, StatePaused, CodeRateLimited)
		return r.pausedHalt(rl)
	case errors.Is(err, doffin.ErrInvalidParams), errors.Is(err, doffin.ErrNotFound):
		setDay(rec, StateFailed, CodeInvalidParams)
		return &halt{reason: ReasonFailed, lastError: msgSearchRejected}
	case errors.As(err, &de):
		setDay(rec, StateFailed, CodeDecodeError)
	default:
		setDay(rec, StateFailed, CodeSearchFailed)
	}
	return r.countError(rec)
}

// pausedHalt stops the run and stores the instant before which Doffin must not be called.
func (r *run) pausedHalt(rl *doffin.RateLimitedError) *halt {
	until := r.e.now().Add(rl.RetryAfter).UTC()
	return &halt{reason: ReasonPaused, lastError: msgRateLimited, retry: &until}
}

// countError counts one error toward the five-in-a-row limit. The fifth stops the run.
func (r *run) countError(rec *DayRecord) *halt {
	r.consecutive++
	if r.consecutive < consecutiveLimit {
		return nil
	}
	setDay(rec, StateFailed, CodeConsecutiveErrors)
	return &halt{reason: ReasonFailed, lastError: msgConsecutive}
}

// fetchDay resolves the ids the store already knows, then downloads the rest in
// first-seen order. The day is done only when nothing is unresolved: every id is
// held, stored, a terminal 404 or a terminal rejection.
func (r *run) fetchDay(d time.Time, hitIDs []string, rec *DayRecord) *halt {
	res, err := r.e.deps.Notices.Resolve(r.ctx, hitIDs)
	if err != nil {
		return r.storeFailed(rec, "resolve notices", err)
	}
	for _, c := range r.resolveCandidates(d, hitIDs, res, rec) {
		if h := r.downloadOne(d, c, rec); h != nil {
			return h
		}
	}
	if rec.Unresolved == 0 {
		setDay(rec, StateDone, "")
	} else {
		rec.State = StatePartial
	}
	return nil
}

// resolveCandidates counts the held ids, the terminal rejections and the terminal
// 404s, leaves the cooling ids unresolved, and returns the rest as download
// candidates. A 404 on a day younger than NotFoundGrace is still a candidate, and
// so is a rejected id whose latest refusal is at least RejectCooloff old (R1).
func (r *run) resolveCandidates(d time.Time, hitIDs []string, res map[string]Resolution, rec *DayRecord) []candidate {
	old := r.isOld(d)
	var cands []candidate
	for _, id := range hitIDs {
		rs := res[id]
		switch {
		case rs.Kind == ResolutionHeld:
			rec.Held++
			r.counters.HeldSkipped++
		case rs.Kind == ResolutionRejected && rs.Rejections >= MaxRejections:
			// Refused MaxRejections times: terminal, in its own bucket (R1, R4).
			rec.Rejected++
		case rs.Kind == ResolutionRejected && r.cooling(rs):
			// Refused too recently to ask again: no call, no attempt, no budget (R1).
			markUnresolved(rec, CodeItemRejected)
		case rs.Kind == ResolutionNotFound && old:
			rec.NotFound++
			r.counters.NotFound++
		default:
			cands = append(cands, candidate{id: id, refusals: refusalsOf(rs)})
		}
	}
	return cands
}

// downloadOne fetches one candidate and stores it or applies table D.
func (r *run) downloadOne(d time.Time, c candidate, rec *DayRecord) *halt {
	if r.ctx.Err() != nil {
		return r.cancelDay(rec, nil)
	}
	// A malformed id is refused before any call (E6). It spends no budget, and no
	// attempt row is written, because no attempt happened.
	if !r.e.validID(c.id) {
		markUnresolved(rec, CodeItemRejected)
		return nil
	}
	if limit := r.e.opts.MaxDownloads; limit > 0 && r.counters.Downloads >= limit {
		setDay(rec, StatePartial, CodeDownloadLimit)
		return &halt{reason: ReasonPartial, lastError: msgDownloadLimit}
	}
	r.counters.Downloads++
	n, err := r.e.deps.Download.Download(r.ctx, c.id)
	if err != nil {
		return r.downloadFailed(d, c, err, rec)
	}
	r.consecutive = 0
	return r.storeNotice(n, rec)
}

// storeNotice writes the body and its attempt in one transaction. A store that
// refuses the body stops the run; the id is fetched again on the next run.
func (r *run) storeNotice(n doffin.Notice, rec *DayRecord) *halt {
	inserted, err := r.e.deps.Notices.StoreNotice(r.ctx, r.runID, n)
	if err != nil {
		return r.storeFailed(rec, "store notice", err)
	}
	if inserted {
		rec.Stored++
		r.counters.Stored++
	} else {
		rec.Held++
		r.counters.Duplicates++
	}
	return nil
}

// downloadFailed records one failed download and applies table D. A cancelled
// call has no response, so it leaves no attempt row.
func (r *run) downloadFailed(d time.Time, c candidate, err error, rec *DayRecord) *halt {
	if r.ctx.Err() != nil {
		return r.cancelDay(rec, err)
	}
	// Backstop for the pre-check in downloadOne: the client refuses a malformed
	// id before any HTTP call, so no attempt happened and no row is written.
	if errors.Is(err, doffin.ErrInvalidParams) {
		markUnresolved(rec, CodeItemRejected)
		return nil
	}
	outcome, status, msg := attemptFor(err)
	attempt := Attempt{DoffinID: c.id, Outcome: outcome, HTTPStatus: status, Message: msg, At: r.e.now().UTC()}
	if aerr := r.e.deps.Notices.RecordAttempt(r.ctx, r.runID, attempt); aerr != nil {
		return r.storeFailed(rec, "record attempt", aerr)
	}
	return r.applyDownloadError(d, c, err, rec)
}

// applyDownloadError is the day and run effect of a failed download (table D).
// Content refusals and not-found notices do not count toward the
// five-in-a-row limit (D-B1).
func (r *run) applyDownloadError(d time.Time, c candidate, err error, rec *DayRecord) *halt {
	var rl *doffin.RateLimitedError
	switch {
	case errors.Is(err, doffin.ErrUnauthorized):
		setDay(rec, StateFailed, CodeUnauthorized)
		return keyRejectedHalt()
	case errors.As(err, &rl):
		setDay(rec, StatePaused, CodeRateLimited)
		return r.pausedHalt(rl)
	case errors.Is(err, doffin.ErrNotFound) && r.isOld(d):
		rec.NotFound++
		r.counters.NotFound++
		return nil
	case errors.Is(err, doffin.ErrNotFound):
		markUnresolved(rec, CodeNotFoundGrace)
		return nil
	case errors.Is(err, doffin.ErrEmptyResponse), errors.Is(err, doffin.ErrResponseTooLarge):
		r.refused(c, rec)
		return nil
	default:
		markUnresolved(rec, CodeItemRejected)
		return r.countError(rec)
	}
}

// storeFailed ends the day for a failed store write. A write that failed because
// the run's context ended is a cancellation, not a store fault. Its body is lost,
// and the id is fetched again on the next run (D-B8).
func (r *run) storeFailed(rec *DayRecord, op string, err error) *halt {
	if r.ctx.Err() != nil {
		return r.cancelDay(rec, nil)
	}
	setDay(rec, StateFailed, CodeStoreFailed)
	return storeFailure(op, err)
}

// cancelDay ends the day as interrupted and stops the run. When the call that the
// cancellation cut short was waiting out a rate-limit back-off, err carries the
// *RateLimitedError, and the run keeps the instant before which Doffin must not be
// called. The reason stays cancelled. err may be nil.
func (r *run) cancelDay(rec *DayRecord, err error) *halt {
	setDay(rec, StateInterrupted, CodeCancelled)
	h := cancelledHalt()
	var rl *doffin.RateLimitedError
	if errors.As(err, &rl) {
		until := r.e.now().Add(rl.RetryAfter).UTC()
		h.retry = &until
	}
	return h
}

// isOld reports whether a 404 on day d is terminal.
func (r *run) isOld(d time.Time) bool {
	return r.e.now().Sub(d) >= NotFoundGrace
}

// setDay sets the day's state and error code.
func setDay(rec *DayRecord, state DayState, code ErrorCode) {
	rec.State = state
	rec.ErrorCode = code
}

// markUnresolved counts an unresolved candidate. The first code of the day is kept.
func markUnresolved(rec *DayRecord, code ErrorCode) {
	rec.Unresolved++
	if rec.ErrorCode == "" {
		rec.ErrorCode = code
	}
}
