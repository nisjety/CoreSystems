package demo

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"sync"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/noticeid"
	"github.com/triodelab/anbud/internal/orgcore"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/bzp"
	"github.com/triodelab/anbud/internal/sources/fts"
)

const (
	defaultMaxConcurrent = 2
	defaultScanTimeout   = 20 * time.Second
	// defaultDocumentsTimeout bounds the call that reads what an organisation's uploaded documents say.
	// It is made while a scan slot is held, after the scan's own deadline, so it needs one of its own:
	// documents are advice, and a store that does not answer must not hold a slot for ever.
	defaultDocumentsTimeout = 10 * time.Second
)

var (
	// ErrBusy means too many scans are running. Nothing was read; try again shortly.
	ErrBusy = errors.New("demo: too many scans in progress")
	// ErrTimeout means the scan did not finish in time. The caller's own cancel is not this.
	ErrTimeout = errors.New("demo: the scan took too long")
	// ErrNotFound means the notice is not held.
	ErrNotFound = errors.New("demo: notice not found")
	// ErrInvalidNoticeID means the id does not have the shape of a notice id.
	ErrInvalidNoticeID = errors.New("demo: invalid notice id")
	// ErrNotAnOpportunity means the notice is a result notice or a prior information notice: there is
	// nothing to bid on.
	ErrNotAnOpportunity = errors.New("demo: the notice is not a tender to bid on")
	// ErrUnreadable means the stored body is not an eForms notice this code can read.
	ErrUnreadable = errors.New("demo: the stored body is not a readable notice")
	// ErrInvalidOrgNumber means the number is not a Norwegian organisation number.
	ErrInvalidOrgNumber = errors.New("demo: invalid organisation number")
	// ErrCompanyNotConfigured means organisation lookups are not set up (ANBUD_ORG_CORE_*).
	ErrCompanyNotConfigured = errors.New("demo: organisation lookup is not configured")
	// ErrCompanyNotFound means Enhetsregisteret has no such organisation.
	ErrCompanyNotFound = errors.New("demo: organisation not found")
	// ErrCompanyUnavailable means the lookup failed: org-core, its credential, or the register.
	// The cause is wrapped and is for the log, not for the caller.
	ErrCompanyUnavailable = errors.New("demo: organisation lookup unavailable")
)

// UnsupportedError means the notice has a shape no field map covers. It is refused, never read by
// guesswork, and says which shape so a person can see what is missing.
type UnsupportedError struct {
	Root, SDKVersion, Flavour string
}

func (e *UnsupportedError) Error() string {
	return fmt.Sprintf("demo: no field map for %s, SDK %s, flavour %s", e.Root, e.SDKVersion, e.Flavour)
}

// NoticeSource is the read side of the raw store.
type NoticeSource interface {
	RecentNotices(ctx context.Context, limit int, fn func(rawstore.Document) error) error
	Notice(ctx context.Context, id string) (rawstore.Document, error)
}

// CompanyLookup looks an organisation up in the register, through org-core.
type CompanyLookup interface {
	Lookup(ctx context.Context, orgNumber string) (orgcore.Organisation, error)
}

// EvidenceSource says what the tender documents an organisation uploaded say about notices. A notice
// that has no upload is not in the answer.
type EvidenceSource interface {
	Evidence(ctx context.Context, org string, noticeIDs []string) (map[string]match.Evidence, error)
}

// Options configures a Service.
type TrackingSource interface {
	TrackedNoticeIDs(context.Context, string) ([]string, error)
}

type Options struct {
	Tracking TrackingSource
	Notices  NoticeSource
	// Companies is nil when organisation lookups are not configured.
	Companies CompanyLookup
	// Evidence is nil when uploaded tender documents are not configured.
	Evidence EvidenceSource
	Now      func() time.Time // time.Now when nil
	// MaxConcurrent bounds the scans running at once; two when zero.
	MaxConcurrent int
	// ScanTimeout bounds one scan; 20 seconds when zero.
	ScanTimeout time.Duration
	// DocumentsTimeout bounds reading the uploaded documents; 10 seconds when zero.
	DocumentsTimeout time.Duration
	// WinnersTimeout bounds the register lookups that tell a winner's kind; 5 seconds when zero.
	WinnersTimeout time.Duration
	Logger         *zerolog.Logger // optional
}

// Service assesses notices against profiles and holds bounded, scoped pagination snapshots.
// Public catalogue projections are cached by database generation.
type Service struct {
	matchPagesMu      sync.Mutex
	matchPages        map[string]*matchPageSnapshot
	tracking          TrackingSource
	preparation       cataloguePreparation
	catalogueMu       sync.Mutex
	catalogueSnapshot *catalogueSnapshot
	notices           NoticeSource
	companies         CompanyLookup
	evidence          EvidenceSource
	now               func() time.Time
	slots             chan struct{}
	scanTimeout       time.Duration
	docsTimeout       time.Duration
	// winnersTimeout bounds the register lookups of one request.
	winnersTimeout time.Duration
	log            *zerolog.Logger
}

// New builds a Service. A notice source is required.
func New(o Options) (*Service, error) {
	if o.Notices == nil {
		return nil, errors.New("demo: a notice source is required")
	}
	s := &Service{tracking: o.Tracking, notices: o.Notices, companies: o.Companies, evidence: o.Evidence, now: o.Now, scanTimeout: o.ScanTimeout,
		docsTimeout: o.DocumentsTimeout, winnersTimeout: o.WinnersTimeout, log: o.Logger}
	if s.now == nil {
		s.now = time.Now
	}
	if s.scanTimeout <= 0 {
		s.scanTimeout = defaultScanTimeout
	}
	if s.docsTimeout <= 0 {
		s.docsTimeout = defaultDocumentsTimeout
	}
	if s.winnersTimeout <= 0 {
		s.winnersTimeout = defaultWinnersTimeout
	}
	slots := o.MaxConcurrent
	if slots <= 0 {
		slots = defaultMaxConcurrent
	}
	s.slots = make(chan struct{}, slots)
	return s, nil
}

// candidate is a parsed contract notice with its normalised form and the id it is stored under.
type candidate struct {
	id      string
	fetched time.Time
	match.Candidate
}

// readKind says what a stored body turned out to be.
type readKind int

const (
	kindOpportunity readKind = iota
	kindAward
	kindPlanning // a prior information notice: a tender announced, not yet open to bid on
	kindUnsupported
	kindUnreadable
)

// read parses and normalises one stored body. Only a contract notice is an opportunity: a result
// notice is parsed, for the incumbency panel and the radar, and is not assessed.
func read(doc rawstore.Document) (candidate, readKind, *UnsupportedError) {
	if doc.Source == noticeid.SourceBZP {
		// A BZP record is the board's JSON, read into the same model as the other sources.
		n, err := bzp.Parse(doc.Body)
		if err != nil {
			return candidate{}, kindUnreadable, nil
		}
		return classify(doc, n)
	}
	if doc.Source == noticeid.SourceFTS {
		// Find a Tender publishes OCDS, not eForms: its release package is read into the same model.
		n, err := fts.Parse(doc.Body)
		if err != nil {
			return candidate{}, kindUnreadable, nil
		}
		return classify(doc, n)
	}
	n, err := eforms.Parse(doc.Body)
	switch {
	case errors.Is(err, eforms.ErrNoFieldMap):
		return candidate{}, kindUnsupported, &UnsupportedError{Root: n.Root, SDKVersion: n.SDKVersion, Flavour: string(n.Flavour)}
	case err != nil:
		return candidate{}, kindUnreadable, nil
	}
	return classify(doc, n)
}

// classify says what kind of notice a parsed notice is: an award, a planning notice or an opportunity.
func classify(doc rawstore.Document, n eforms.Notice) (candidate, readKind, *UnsupportedError) {
	switch {
	case n.Root == "ContractAwardNotice":
		return candidate{id: doc.ID, fetched: doc.FetchedAt, Candidate: match.Candidate{Notice: n}}, kindAward, nil
	case n.Root == "PriorInformationNotice":
		return candidate{id: doc.ID, fetched: doc.FetchedAt, Candidate: match.Candidate{Notice: n, Norm: normalize.Notice(n)}}, kindPlanning, nil
	}
	return candidate{id: doc.ID, fetched: doc.FetchedAt, Candidate: match.Candidate{Notice: n, Norm: normalize.Notice(n)}}, kindOpportunity, nil
}

// computeMatches reads active stored opportunities, assesses each tender against the profile and returns
// the ones the request asks for, best first, with a count of what happened to every notice read.
func (s *Service) computeMatches(ctx context.Context, r Request) (Matches, error) {
	now := s.now()
	status, profile := s.company(ctx, r.Profile)

	release, err := s.acquire()
	if err != nil {
		return Matches{}, err
	}
	defer release()
	if r.TrackedOnly {
		tracked, err := s.trackedResults(ctx, r, profile, now)
		if err != nil {
			return Matches{}, err
		}
		return Matches{AsOf: now.UTC(), Strictness: strictnessName(r.Strictness), Results: []Result{}, TrackedResults: tracked, Coverage: "tracked_only", Funnel: Funnel{Gate: map[string]int{"pass": 0, "conditional": 0, "fail": 0}}, GainedByWidening: map[string]int{}}, nil
	}

	scanCtx, cancel := context.WithTimeout(ctx, s.scanTimeout)
	defer cancel()
	readAt := now
	if r.ShowAll {
		readAt = time.Time{}
	}
	contracts, funnel, err := s.activeContracts(scanCtx, readAt)
	if err != nil {
		if ctx.Err() == nil && errors.Is(scanCtx.Err(), context.DeadlineExceeded) {
			return Matches{}, ErrTimeout
		}
		return Matches{}, err
	}
	set := settle(contracts)
	candidates := make([]candidate, 0, len(set.current))
	activeCount := 0
	for _, c := range set.current {
		_, active := match.NextOpenDeadline(c.Norm, now)
		if active {
			activeCount++
		}
		if active || r.ShowAll {
			candidates = append(candidates, c)
		} else if len(c.Norm.Deadlines) > 0 {
			funnel.Inactive++
		} else {
			funnel.Unscheduled++
		}
	}
	funnel.Superseded = set.superseded
	evidence, documents := s.documents(ctx, r.Org, candidates)

	type assessed struct {
		c candidate
		a match.Assessment
	}
	var shown []assessed
	for _, c := range candidates {
		a := match.AssessWith(c.Notice, c.Norm, profile, r.Strictness, now, evidence[c.id])
		funnel.Gate[string(a.Verdict)]++
		item := assessed{c, a}
		_, active := match.NextOpenDeadline(c.Norm, now)
		if r.ShowAll || (active && a.Verdict != match.Fail && a.Band != match.None) {
			shown = append(shown, item)
		}
	}
	sort.SliceStable(shown, func(i, j int) bool {
		if ai, bi := match.ContextRank(shown[i].a), match.ContextRank(shown[j].a); ai != bi && shown[i].a.Verdict == shown[j].a.Verdict && shown[i].a.Band == shown[j].a.Band {
			return ai > bi
		}
		return better(shown[i].c, shown[i].a, shown[j].c, shown[j].a, now)
	})
	funnel.Opportunities = len(candidates)
	funnel.Matched = len(shown)

	limit := r.Limit
	if limit <= 0 {
		limit = DefaultLimit
	}
	if r.Paginate {
		limit = len(shown)
	}
	results := make([]Result, 0, min(limit, len(shown)))
	for _, item := range shown[:min(limit, len(shown))] {
		res := newResult(item.c.id, item.c, item.a, evidence[item.c.id], now)
		if p, ok := set.amended[item.c.id]; ok {
			complete := true
			// Hydrate only selected amendment histories that had inactive identity-only
			// rows. Never compute a diff against deliberately omitted source fields.
			for i, v := range p.Versions {
				if v.Notice.Title != "" {
					continue
				}
				prior, err := s.notices.Notice(ctx, set.cs[indexOf(v)].id)
				if err != nil {
					complete = false
					break
				}
				parsed, kind, _ := read(prior)
				if kind != kindOpportunity {
					complete = false
					break
				}
				p.Versions[i].Notice = parsed.Notice
			}
			if complete {
				res.Amendment = amendmentOf(p)
			}
		}
		results = append(results, res)
	}

	matchCandidates := make([]match.Candidate, len(candidates))
	for i, c := range candidates {
		matchCandidates[i] = c.Candidate
	}
	tracked, err := s.trackedResults(ctx, r, profile, now)
	if err != nil {
		return Matches{}, err
	}
	// Widening is counted without the documents. They cannot change it: what a document says can make
	// a check conditional and never failed, and widening adds tenders the gate does not fail.
	ids := make([]string, len(candidates))
	validUntil := now.Add(10 * time.Minute)
	for i, c := range candidates {
		ids[i] = c.id
		for _, d := range c.Norm.Deadlines {
			if d.At.After(now) && d.At.Before(validUntil) {
				validUntil = d.At
			}
		}
	}
	coverage := "all_active"
	if r.ShowAll {
		coverage = "all_opportunities"
	}
	return Matches{
		AsOf: now.UTC(), Strictness: strictnessName(r.Strictness), Company: status, Documents: documents, Funnel: funnel,
		GainedByWidening: gainedByWidening(matchCandidates, profile, r.Strictness, now),
		Results:          results, TrackedResults: tracked, Coverage: coverage, ActiveAssessed: activeCount, assessedIDs: ids, evidenceKey: matchingEvidenceKey(evidence, documents), validUntil: validUntil,
	}, nil
}

func scanLimit(n int) int {
	switch {
	case n < 1:
		return DefaultScan
	case n > MaxScan:
		return MaxScan
	}
	return n
}

func strictnessName(s match.Strictness) string {
	if int(s) < 0 || int(s) >= len(strictnessNames) {
		return strictnessNames[match.Balanced]
	}
	return strictnessNames[s]
}

// gainedByWidening counts, for each strictness wider than from, the tenders it would add.
func gainedByWidening(candidates []match.Candidate, p match.Profile, from match.Strictness, now time.Time) map[string]int {
	gained := map[string]int{}
	for level := from + 1; level <= match.Wide; level++ {
		gained[strictnessName(level)] = match.Gained(candidates, p, from, level, now)
	}
	return gained
}

var verdictRank = map[match.Outcome]int{match.Pass: 0, match.Conditional: 1, match.Fail: 2}
var bandRank = map[match.Band]int{match.Strong: 0, match.Good: 1, match.Stretch: 2, match.None: 3}

// better orders results: tenders a company can bid on before those it cannot, the better fit
// first, the nearest deadline first, and the notice id to break a tie so the order is stable.
func better(a candidate, aa match.Assessment, b candidate, ba match.Assessment, now time.Time) bool {
	if x, y := verdictRank[aa.Verdict], verdictRank[ba.Verdict]; x != y {
		return x < y
	}
	if x, y := bandRank[aa.Band], bandRank[ba.Band]; x != y {
		return x < y
	}
	da, aok := match.NextOpenDeadline(a.Norm, now)
	db, bok := match.NextOpenDeadline(b.Norm, now)
	switch {
	case aok && bok && !da.At.Equal(db.At):
		return da.At.Before(db.At)
	case aok != bok:
		return aok
	}
	return a.id < b.id
}

// acquire takes a scan slot without waiting: a busy service says so, and does not queue work.
func (s *Service) acquire() (release func(), err error) {
	select {
	case s.slots <- struct{}{}:
		return func() { <-s.slots }, nil
	default:
		return nil, ErrBusy
	}
}

// scan reads up to limit stored notices and parses each, counting what each turned out to be. It
// returns the tenders, and the awarded lots of the result notices.
func (s *Service) scan(ctx context.Context, limit int) ([]candidate, []incumbency.Award, Funnel, error) {
	funnel := Funnel{Gate: map[string]int{"pass": 0, "conditional": 0, "fail": 0}}
	var candidates []candidate
	var awards []incumbency.Award

	scanCtx, cancel := context.WithTimeout(ctx, s.scanTimeout)
	defer cancel()
	type scanned struct {
		c    candidate
		kind readKind
	}
	var reads []scanned
	var copies []twin
	err := s.notices.RecentNotices(scanCtx, limit, func(doc rawstore.Document) error {
		if err := scanCtx.Err(); err != nil {
			return err
		}
		funnel.Scanned++
		c, kind, _ := read(doc)
		reads = append(reads, scanned{c: c, kind: kind})
		copies = append(copies, twin{noticeID: c.Notice.NoticeID, version: c.Notice.VersionID, source: doc.Source})
		return nil
	})
	// A notice held from Doffin and from TED is one notice (twins.go): its other copy is neither a
	// tender nor an award a second time.
	keep := keepOnePerNotice(copies)
	for i, r := range reads {
		if !keep[i] {
			continue
		}
		switch r.kind {
		case kindOpportunity:
			candidates = append(candidates, r.c)
		case kindAward:
			funnel.Awards++
			awards = append(awards, incumbency.FromNotice(r.c.id, r.c.Notice)...)
		case kindPlanning:
			funnel.Planning++
		case kindUnsupported:
			funnel.Unsupported++
		default:
			funnel.Unreadable++
		}
	}
	if err != nil {
		// Our own deadline is a timeout; the caller's cancel is the caller's own.
		if ctx.Err() == nil && errors.Is(scanCtx.Err(), context.DeadlineExceeded) {
			return nil, nil, Funnel{}, ErrTimeout
		}
		return nil, nil, Funnel{}, err
	}
	return candidates, awards, funnel, nil
}

// company looks the profile's company up in the register, when it names one, and returns the
// status to report and the profile with the register's verdict applied. A lookup that cannot be
// made does not stop the demo and fails nothing: it is reported as not checked.
func (s *Service) company(ctx context.Context, p match.Profile) (*CompanyStatus, match.Profile) {
	if p.OrgNumber == "" {
		return nil, p
	}
	status := &CompanyStatus{OrgNumber: p.OrgNumber}
	if s.companies == nil {
		status.Reason = "not_configured"
		return status, p
	}
	org, err := s.companies.Lookup(ctx, p.OrgNumber)
	switch {
	case err == nil:
		canBid := org.CanBid()
		status.Checked, status.Name, status.CanBid = true, org.Name, &canBid
		p.Blocked = !canBid
	case errors.Is(err, orgcore.ErrNotFound):
		status.Reason = "not_found"
	default:
		status.Reason = "unavailable"
		if s.log != nil {
			s.log.Warn().Err(err).Msg("demo: organisation lookup failed")
		}
	}
	return status, p
}

// documents reads what the organisation's uploaded tender documents say about the candidates, in one
// question, and says whether it could. Documents are advice: a store that cannot be read costs the
// advice and nothing else, and the status says so. Nothing is read, and there is no status, when the
// request names no organisation or no document service is configured.
func (s *Service) documents(ctx context.Context, org string, candidates []candidate) (map[string]match.Evidence, *DocumentsStatus) {
	if org == "" || s.evidence == nil {
		return nil, nil
	}
	status := &DocumentsStatus{}
	if len(candidates) == 0 {
		status.Checked = true
		return nil, status
	}
	ids := make([]string, len(candidates))
	for i, c := range candidates {
		ids[i] = c.id
	}
	ctx, cancel := context.WithTimeout(ctx, s.docsTimeout)
	defer cancel()
	evidence, err := s.evidence.Evidence(ctx, org, ids)
	if err != nil {
		status.Reason = "unavailable"
		if s.log != nil {
			// The cause is for the log. The organisation is not: an id of a tenant is not log material.
			s.log.Warn().Err(err).Msg("demo: uploaded documents could not be read")
		}
		return nil, status
	}
	status.Checked, status.Notices = true, len(evidence)
	return evidence, status
}

// Audit assesses one stored notice and returns everything behind the assessment. It refuses an id
// that is not a notice id before it asks the store anything.
func (s *Service) Audit(ctx context.Context, noticeID string, r Request) (Audit, error) {
	if !noticeid.Valid(noticeID) {
		return Audit{}, ErrInvalidNoticeID
	}
	now := s.now()
	status, profile := s.company(ctx, r.Profile)

	doc, err := s.notices.Notice(ctx, noticeID)
	switch {
	case errors.Is(err, rawstore.ErrNotFound):
		return Audit{}, ErrNotFound
	case err != nil:
		return Audit{}, err
	}
	c, kind, unsupported := read(doc)
	switch kind {
	case kindUnsupported:
		return Audit{}, unsupported
	case kindUnreadable:
		return Audit{}, ErrUnreadable
	case kindAward, kindPlanning:
		return Audit{}, ErrNotAnOpportunity
	}
	evidence, documents := s.documents(ctx, r.Org, []candidate{c})
	ev := evidence[c.id]
	a := match.AssessWith(c.Notice, c.Norm, profile, r.Strictness, now, ev)
	audit := newAudit(doc, c, a, ev, now)
	audit.Company = status
	audit.DocumentsStatus = documents
	audit.Incumbency, audit.IncumbencyStatus, audit.Amendments = s.historyOf(ctx, c, r.Scan)
	return audit, nil
}

// Company looks an organisation up in the register, for pre-filling a profile.
func (s *Service) Company(ctx context.Context, orgNumber string) (Company, error) {
	number, err := normalize.OrgNumber(orgNumber)
	if err != nil {
		return Company{}, ErrInvalidOrgNumber
	}
	if s.companies == nil {
		return Company{}, ErrCompanyNotConfigured
	}
	org, err := s.companies.Lookup(ctx, number)
	switch {
	case err == nil:
	case errors.Is(err, orgcore.ErrNotFound):
		return Company{}, ErrCompanyNotFound
	case errors.Is(err, orgcore.ErrInvalidOrgNumber):
		return Company{}, ErrInvalidOrgNumber
	default:
		return Company{}, fmt.Errorf("%w: %w", ErrCompanyUnavailable, err)
	}
	return Company{
		OrgNumber: org.OrgNumber, Name: org.Name, OrgForm: org.OrgForm, NACE: org.NACE, NACEText: org.NACEText,
		Municipality: org.Municipality, MunicipalityNumber: org.MunicipalityNumber, Employees: org.Employees,
		CanBid: org.CanBid(), Bankrupt: org.Bankrupt, UnderLiquidation: org.UnderLiquidation, ForcedLiquidation: org.ForcedLiquidation,
	}, nil
}
