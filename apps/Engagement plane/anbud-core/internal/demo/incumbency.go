package demo

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"time"

	"github.com/triodelab/anbud/internal/incumbency"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/orgcore"
)

const (
	// maxWinnerLookups bounds the register lookups one request makes to tell organisations from sole
	// traders. A winner beyond it is not checked, and so not named.
	maxWinnerLookups = 20
	// lookupWorkers bounds the lookups running at once.
	lookupWorkers = 4
	// defaultWinnersTimeout bounds all the lookups of one request. Names are advice: a slow register
	// costs the names and nothing else.
	defaultWinnersTimeout = 5 * time.Second
	// soleTraderForm is the organisasjonsform code of an enkeltpersonforetak.
	soleTraderForm = "ENK"

	DefaultHorizonMonths = 12
	MaxHorizonMonths     = 36
)

// The states of the winner check, as a response reports it.
const (
	winnersChecked       = "checked"        // every winner to check was answered
	winnersPartial       = "partial"        // some were not: those are not named
	winnersNotConfigured = "not_configured" // no register: no winner is named
)

// classify asks the register what each organisation number is: an organisation, a sole trader, or
// (no answer) unverified. It returns the classes and the state of the check.
func (s *Service) classify(ctx context.Context, numbers []string) (map[string]incumbency.Class, string) {
	if s.companies == nil {
		return nil, winnersNotConfigured
	}
	classes := make(map[string]incumbency.Class, len(numbers))
	if len(numbers) == 0 {
		return classes, winnersChecked
	}
	ctx, cancel := context.WithTimeout(ctx, s.winnersTimeout)
	defer cancel()

	var (
		mu     sync.Mutex
		failed bool
		wg     sync.WaitGroup
		slots  = make(chan struct{}, lookupWorkers)
	)
	for _, number := range numbers {
		wg.Add(1)
		slots <- struct{}{}
		go func() {
			defer wg.Done()
			defer func() { <-slots }()
			org, err := s.companies.Lookup(ctx, number)
			mu.Lock()
			defer mu.Unlock()
			switch {
			case err != nil:
				failed = true
				if s.log != nil && !errors.Is(err, orgcore.ErrNotFound) {
					// The cause, not the number: a winner's number is not log material.
					s.log.Warn().Err(err).Msg("demo: a winner could not be checked")
				}
			case org.OrgForm == soleTraderForm:
				classes[number] = incumbency.SoleTrader
			default:
				classes[number] = incumbency.Organisation
			}
		}()
	}
	wg.Wait()
	if failed {
		return classes, winnersPartial
	}
	return classes, winnersChecked
}

// IncumbencyStatus says whether the panel could be made and what it read.
type IncumbencyStatus struct {
	Checked bool   `json:"checked"`
	Reason  string `json:"reason,omitempty"` // busy, timeout or unavailable when not checked
	Scanned int    `json:"scanned"`          // stored notices read
	Awards  int    `json:"awards"`           // result notices among them
	Winners string `json:"winners"`          // checked, partial or not_configured
}

// historyOf is what the audit sheet shows of a tender's history: its incumbency panel, and the notices
// of its procedure with what changed. One scan of the newest stored notices feeds both; the audited
// notice is added to the procedure's notices on its own, so the history holds it even when the scan
// window does not. Both are advice: a scan that cannot run costs them, and the status says why.
func (s *Service) historyOf(ctx context.Context, c candidate, scan int) (*IncumbencyView, *IncumbencyStatus, *AmendmentsView) {
	release, err := s.acquire()
	if err != nil {
		return nil, &IncumbencyStatus{Reason: "busy"}, nil
	}
	defer release()
	contracts, awards, funnel, err := s.scan(ctx, scanLimit(scan))
	if err != nil {
		reason := "unavailable"
		if errors.Is(err, ErrTimeout) {
			reason = "timeout"
		} else if s.log != nil {
			s.log.Warn().Err(err).Msg("demo: award notices could not be read")
		}
		return nil, &IncumbencyStatus{Reason: reason}, nil
	}
	target := incumbency.Target{ProcedureID: c.Notice.ProcedureID, BuyerOrgNumbers: c.Notice.BuyerOrgNumbers, CPV: c.Norm.CPV}
	classes, winners := s.classify(ctx, incumbency.WinnersToCheck(target, awards, maxWinnerLookups))
	view := incumbencyView(incumbency.Build(target, awards, classes))
	status := &IncumbencyStatus{Checked: true, Scanned: funnel.Scanned, Awards: funnel.Awards, Winners: winners}
	return &view, status, amendmentsOf(c, contracts)
}

// ReTenderRequest is a validated request for the contracts likely to be tendered again soon. The slider's
// step is kept once, in Scope.Level: balanced when the request gives none.
type ReTenderRequest struct {
	Scope         incumbency.Scope
	HorizonMonths int
	Scan          int
}

type reTenderRequest struct {
	Profile       *profileRequest `json:"profile"`
	HorizonMonths *int            `json:"horizon_months"`
	Scan          *int            `json:"scan"`
	Strictness    *string         `json:"strictness"`
}

// DecodeReTenderRequest reads and validates a radar request. The profile is optional; only its CPV
// codes and regions narrow the radar. Every problem is reported at once, and none quotes a value.
func DecodeReTenderRequest(body []byte) (ReTenderRequest, []FieldError) {
	var wire reTenderRequest
	if e := decodeStrict(body, &wire); e != nil {
		return ReTenderRequest{}, []FieldError{*e}
	}
	var v validator
	r := ReTenderRequest{HorizonMonths: DefaultHorizonMonths, Scan: MaxScan}
	if wire.HorizonMonths != nil {
		if *wire.HorizonMonths < 1 || *wire.HorizonMonths > MaxHorizonMonths {
			v.add("horizon_months", fmt.Sprintf("must be between 1 and %d", MaxHorizonMonths))
		} else {
			r.HorizonMonths = *wire.HorizonMonths
		}
	}
	if wire.Scan != nil {
		if *wire.Scan < 1 || *wire.Scan > MaxScan {
			v.add("scan", fmt.Sprintf("must be between 1 and %d", MaxScan))
		} else {
			r.Scan = *wire.Scan
		}
	}
	level, _ := strictnessFrom(&v, wire.Strictness)
	if wire.Profile != nil {
		p := buildProfile(&v, wire.Profile)
		r.Scope = incumbency.Scope{CPV: p.CPV, NUTS: p.NUTS}
	}
	r.Scope.Level = level
	if len(v.errs) > 0 {
		return ReTenderRequest{}, v.errs
	}
	return r, nil
}

// ReTenders is the radar: contracts whose base period ends between today and the horizon. Strictness is
// the slider's step, and Widening says what each wider step would add. Results hold at most MaxReTenders
// rows, the exact matches first; Matching is the total before that cap.
type ReTenders struct {
	AsOf       time.Time      `json:"as_of"`
	Window     WindowView     `json:"window"`
	Strictness string         `json:"strictness"`
	Matching   int            `json:"matching"`
	Scanned    int            `json:"scanned"`
	Awards     int            `json:"awards"`
	Winners    string         `json:"winners"`
	Widening   []WideningView `json:"widening"`
	Results    []AwardView    `json:"results"`
}

// WideningView is one wider step of the slider: how many more contracts it lists, and what it widens.
type WideningView struct {
	To    string   `json:"to"`
	Adds  int      `json:"adds"`
	Dials []string `json:"dials"`
}

// wideningViews shows each wider step with its count and the dials it widens.
func wideningViews(ws []incumbency.Widen) []WideningView {
	out := make([]WideningView, 0, len(ws))
	for _, w := range ws {
		out = append(out, WideningView{To: strictnessName(w.Level), Adds: w.Adds, Dials: dialsOf(w.Level)})
	}
	return out
}

// dialsOf names what a step widens, for the slider's "+N if you widen X", as codes: the app words them in
// its own language. Strict widens nothing.
func dialsOf(level match.Strictness) []string {
	switch level {
	case match.Balanced:
		return []string{"cpv_group", "nuts2_region"}
	case match.Wide:
		return []string{"cpv_division", "nuts1_region"}
	}
	return []string{}
}

// WindowView is the radar's span of days, both ends included.
type WindowView struct {
	From string `json:"from"`
	To   string `json:"to"`
}

// ReTenders reads the award notices among the newest stored notices and lists the contracts that
// end within the horizon, for the request's codes: exact matches first, each group nearest end first.
func (s *Service) ReTenders(ctx context.Context, r ReTenderRequest) (ReTenders, error) {
	now := s.now()
	release, err := s.acquire()
	if err != nil {
		return ReTenders{}, err
	}
	defer release()
	_, awards, funnel, err := s.scan(ctx, scanLimit(r.Scan))
	if err != nil {
		return ReTenders{}, err
	}
	u := now.UTC()
	from := time.Date(u.Year(), u.Month(), u.Day(), 0, 0, 0, 0, time.UTC)
	window := incumbency.Window{From: from, To: from.AddDate(0, r.HorizonMonths, 0)}
	ending := incumbency.Ending(awards, r.Scope, window)
	classes, winners := s.classify(ctx, incumbency.NorwegianWinners(ending, maxWinnerLookups))
	results := []AwardView{}
	for i, e := range incumbency.Show(ending, classes) {
		row := awardView(e)
		row.Reach = reachOf(ending[i], r.Scope)
		results = append(results, row)
	}
	return ReTenders{
		AsOf: u, Window: WindowView{From: dateOf(&window.From), To: dateOf(&window.To)},
		Strictness: strictnessName(r.Scope.Level), Matching: incumbency.Matching(awards, r.Scope, window),
		Widening: wideningViews(incumbency.Widening(awards, r.Scope, window)),
		Scanned:  funnel.Scanned, Awards: funnel.Awards, Winners: winners, Results: results,
	}, nil
}

// IncumbencyView is a tender's history with its buyer, as the audit sheet shows it.
type IncumbencyView struct {
	Related               int               `json:"related"`
	Incumbent             *AwardView        `json:"incumbent,omitempty"`
	TopWinners            []WinnerCountView `json:"top_winners"`
	WithheldWinners       int               `json:"withheld_winners"`
	MedianTendersReceived *float64          `json:"median_tenders_received,omitempty"`
	Awards                []AwardView       `json:"awards"`
}

// AwardView is a past award. Its dates are calendar days; ContractEnd is empty when the end is open
// or unknown, and EndBasis says whether it was stated or estimated, and from what.
type AwardView struct {
	NoticeID        string `json:"notice_id"`
	Title           string `json:"title"`
	LotTitle        string `json:"lot_title,omitempty"`
	Buyer           string `json:"buyer,omitempty"`
	Relation        string `json:"relation,omitempty"`
	AwardDate       string `json:"award_date,omitempty"`
	Signed          string `json:"signed,omitempty"`
	ContractEnd     string `json:"contract_end,omitempty"`
	EndBasis        string `json:"end_basis"`
	Renewals        *int   `json:"renewals,omitempty"`
	TendersReceived *int   `json:"tenders_received,omitempty"`
	// Reach is set on the radar only: "exact" when the strictest step already reaches the contract's codes,
	// "near" when only a wider step does. It tells a reader why a contract is on the list.
	Reach   string       `json:"reach,omitempty"`
	Value   *Money       `json:"value,omitempty"`
	Winners []WinnerView `json:"winners"`
}

// WinnerView is a winner as it may be shown: named only when it is an organisation. Status is
// organisation, sole_trader or unverified.
type WinnerView struct {
	Name      string `json:"name,omitempty"`
	OrgNumber string `json:"org_number,omitempty"`
	Status    string `json:"status"`
}

// WinnerCountView is an organisation and how many of the related awards it won.
type WinnerCountView struct {
	Name      string `json:"name"`
	OrgNumber string `json:"org_number"`
	Wins      int    `json:"wins"`
}

func incumbencyView(p incumbency.Panel) IncumbencyView {
	v := IncumbencyView{
		Related: p.Related, WithheldWinners: p.WithheldWinners, MedianTendersReceived: p.MedianTendersReceived,
		TopWinners: []WinnerCountView{}, Awards: []AwardView{},
	}
	if p.Incumbent != nil {
		a := awardView(*p.Incumbent)
		v.Incumbent = &a
	}
	for _, w := range p.TopWinners {
		v.TopWinners = append(v.TopWinners, WinnerCountView{Name: clip(w.Name, maxTextRunes), OrgNumber: w.OrgNumber, Wins: w.Wins})
	}
	for _, e := range p.Entries {
		v.Awards = append(v.Awards, awardView(e))
	}
	return v
}

// reachOf says how a radar contract reaches the scope: exact or near, see AwardView.Reach.
func reachOf(a incumbency.Award, s incumbency.Scope) string {
	if incumbency.ExactMatch(a, s) {
		return "exact"
	}
	return "near"
}

func awardView(e incumbency.Entry) AwardView {
	v := AwardView{
		NoticeID: e.NoticeID, Title: clip(e.Title, maxTextRunes), LotTitle: clip(e.LotTitle, maxTextRunes), Relation: string(e.Relation),
		AwardDate: dateOf(e.AwardDate), Signed: dateOf(e.Signed), ContractEnd: dateOf(e.End.At), EndBasis: string(e.End.Basis),
		Renewals: e.Renewals, TendersReceived: e.TendersReceived, Value: money(e.Value), Winners: []WinnerView{},
	}
	if len(e.BuyerNames) > 0 {
		v.Buyer = clip(e.BuyerNames[0], maxTextRunes)
	}
	for _, w := range e.Winners {
		v.Winners = append(v.Winners, WinnerView{Name: clip(w.Name, maxTextRunes), OrgNumber: w.OrgNumber, Status: w.Class.String()})
	}
	return v
}

func dateOf(t *time.Time) string {
	if t == nil {
		return ""
	}
	return t.UTC().Format(time.DateOnly)
}
