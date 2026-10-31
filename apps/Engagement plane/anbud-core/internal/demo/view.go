package demo

import (
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
	"github.com/triodelab/anbud/internal/normalize"
	"github.com/triodelab/anbud/internal/rawstore"
)

// Bounds of what a response shows of one notice. A notice may be 8 MiB, and a response must not
// grow with it.
const (
	maxTextRunes = 2000
	maxListed    = 100
	maxUnread    = 50
)

// Money is an amount as a decimal string: exact, with no floating point.
type Money struct {
	Currency string `json:"currency"`
	Amount   string `json:"amount"`
}

func money(a *eforms.Amount) *Money {
	if a == nil {
		return nil
	}
	return &Money{Currency: a.Currency, Amount: fmt.Sprintf("%d.%02d", a.Minor/100, a.Minor%100)}
}

// DeadlineView is the deadline a company has to meet next: the earliest of the notice's that has
// not passed.
type DeadlineView struct {
	At       time.Time `json:"at"`
	OsloDay  string    `json:"oslo_day"`
	DaysLeft int       `json:"days_left"`
	// Kind is "tender", or "participation" for the deadline for requests to participate.
	Kind string `json:"kind"`
	Lot  string `json:"lot,omitempty"`
}

// CiteView points at where in the notice a check's evidence is. Index is set for the fields that
// repeat, such as a selection criterion.
type CiteView struct {
	Field string `json:"field"`
	Lot   string `json:"lot,omitempty"`
	Index *int   `json:"index,omitempty"`
	// A check read from an uploaded document cites the file, the page of a PDF or the paragraph of
	// any other kind, and the sentence it was judged on.
	Document  string `json:"document,omitempty"`
	Page      int    `json:"page,omitempty"`
	Paragraph int    `json:"paragraph,omitempty"`
	Snippet   string `json:"snippet,omitempty"`
}

// CheckView is one rule applied to one notice.
type CheckView struct {
	Rule     string   `json:"rule"`
	Outcome  string   `json:"outcome"`
	Detail   string   `json:"detail"`
	Cite     CiteView `json:"cite"`
	DaysLeft *int     `json:"days_left,omitempty"`
}

func gateCounts(checks []match.Check) map[string]int {
	counts := map[string]int{"pass": 0, "conditional": 0, "unknown": 0, "fail": 0}
	for _, c := range checks {
		counts[string(c.Outcome)]++
	}
	return counts
}

func checkViews(checks []match.Check) []CheckView {
	out := make([]CheckView, 0, len(checks))
	for _, c := range checks {
		v := CheckView{Rule: c.Rule, Outcome: string(c.Outcome), Detail: c.Detail, DaysLeft: c.DaysLeft,
			Cite: CiteView{Field: c.Cite.Field, Lot: c.Cite.Lot}}
		if c.Cite.Field == "requirement" || c.Cite.Field == "selection criterion" {
			index := c.Cite.Index
			v.Cite.Index = &index
		}
		if c.Cite.Field == "document" {
			v.Cite.Document, v.Cite.Page, v.Cite.Paragraph = clip(c.Cite.Document, maxTextRunes), c.Cite.Page, c.Cite.Paragraph
			v.Cite.Snippet = clip(c.Cite.Snippet, maxTextRunes)
		}
		out = append(out, v)
	}
	return out
}

// Result is one notice and how well it fits the profile. It holds nothing a person could be
// identified by: the parser counts a notice's contact persons and never copies them.
type Result struct {
	Retained        bool          `json:"retained,omitempty"`
	RetentionReason string        `json:"retention_reason,omitempty"`
	NoticeID        string        `json:"notice_id"`
	NoticeType      string        `json:"notice_type"`
	NoticeSubtype   string        `json:"notice_subtype,omitempty"`
	ProcedureType   string        `json:"procedure_type,omitempty"`
	Title           string        `json:"title"`
	Buyer           string        `json:"buyer,omitempty"`
	BuyerOrgNumbers []string      `json:"buyer_org_numbers"`
	CPV             []string      `json:"cpv"`
	NUTS            []string      `json:"nuts"`
	EstimatedValue  *Money        `json:"estimated_value,omitempty"`
	Deadline        *DeadlineView `json:"deadline,omitempty"`
	// PlannedNotice is the day a prior information notice says its notice will be published, as
	// YYYY-MM-DD; absent on every other notice.
	PlannedNotice string `json:"planned_notice,omitempty"`
	// IssuedAt is when the notice was issued, as an RFC 3339 instant: what the tender pages count as new.
	IssuedAt string      `json:"issued_at,omitempty"`
	Verdict  string      `json:"verdict"`
	Band     string      `json:"band"`
	Gate     []CheckView `json:"gate"`
	// GateCounts counts the gate checks by outcome, all four always present. A notice that is
	// conditional only because its criteria are in documents nobody has read has unknown checks
	// and no conditional ones.
	GateCounts map[string]int `json:"gate_counts"`
	Fit        []CheckView    `json:"fit"`
	Amendment  *AmendmentView `json:"amendment,omitempty"`
	// Documents summarises what the organisation's uploaded tender documents for this notice came to.
	// It is absent when the organisation uploaded none.
	Documents *DocumentsView `json:"documents,omitempty"`
}

// DocumentsView counts what the uploaded documents of one notice held: files read, files that could
// not be read, and passages found that name something the rules know.
type DocumentsView struct {
	Read     int `json:"read"`
	Unread   int `json:"unread"`
	Findings int `json:"findings"`
}

func documentsView(ev match.Evidence) *DocumentsView {
	if ev.Documents == 0 && ev.Unread == 0 && len(ev.Findings) == 0 {
		return nil
	}
	return &DocumentsView{Read: ev.Documents, Unread: ev.Unread, Findings: len(ev.Findings) + ev.FindingsDropped}
}

func newResult(id string, c candidate, a match.Assessment, ev match.Evidence, now time.Time) Result {
	n := c.Notice
	r := Result{
		NoticeID: id, NoticeType: n.NoticeTypeCode, NoticeSubtype: n.SubTypeCode, ProcedureType: n.ProcedureType,
		Title:           clip(n.Title, maxTextRunes),
		BuyerOrgNumbers: nonNil(c.Norm.BuyerOrgNumbers), CPV: nonNil(c.Norm.CPV), NUTS: nonNil(c.Norm.NUTS),
		EstimatedValue: money(match.ValueOf(n)),
		IssuedAt:       issuedText(n.IssuedAt),
		Verdict:        string(a.Verdict), Band: string(a.Band),
		Gate: checkViews(a.Gate), GateCounts: gateCounts(a.Gate), Fit: checkViews(a.Fit),
		Documents: documentsView(ev),
	}
	if len(n.BuyerNames) > 0 {
		r.Buyer = clip(n.BuyerNames[0], maxTextRunes)
	}
	if d, ok := match.NextOpenDeadline(c.Norm, now); ok {
		r.Deadline = &DeadlineView{At: d.At.UTC(), OsloDay: normalize.OsloDay(d.At), DaysLeft: normalize.DaysLeft(d.At, now), Kind: string(d.Kind), Lot: d.Lot}
	}
	if p := c.Notice.PlannedNoticeDate; p != nil {
		r.PlannedNotice = p.Format("2006-01-02")
	}
	return r
}

// CompanyStatus says whether the company in the profile was checked in the register. When it was
// not, Reason says why (not_configured, not_found or unavailable) and nothing was failed for it.
type CompanyStatus struct {
	OrgNumber string `json:"org_number"`
	Checked   bool   `json:"checked"`
	Reason    string `json:"reason,omitempty"`
	Name      string `json:"name,omitempty"`
	CanBid    *bool  `json:"can_bid,omitempty"`
}

// Company is what the register holds about an organisation, for pre-filling a profile. It is
// facts, not a profile: one næringskode covers firms that sell different things.
type Company struct {
	OrgNumber          string `json:"org_number"`
	Name               string `json:"name"`
	OrgForm            string `json:"org_form,omitempty"`
	NACE               string `json:"nace,omitempty"`
	NACEText           string `json:"nace_text,omitempty"`
	Municipality       string `json:"municipality,omitempty"`
	MunicipalityNumber string `json:"municipality_number,omitempty"`
	Employees          *int   `json:"employees,omitempty"`
	CanBid             bool   `json:"can_bid"`
	Bankrupt           bool   `json:"bankrupt"`
	UnderLiquidation   bool   `json:"under_liquidation"`
	ForcedLiquidation  bool   `json:"forced_liquidation"`
}

// DocumentsStatus says whether the organisation's uploaded tender documents were used. When they could
// not be read, Reason says why (unavailable) and nothing was judged by them. Notices counts the
// tenders the organisation uploaded documents for.
type DocumentsStatus struct {
	Checked bool   `json:"checked"`
	Reason  string `json:"reason,omitempty"`
	Notices int    `json:"notices"`
}

// Funnel says what happened to the notices a request read, so that nothing is lost without a count.
type Funnel struct {
	Inactive      int `json:"inactive"`
	Unscheduled   int `json:"unscheduled"`
	Scanned       int `json:"scanned"`
	Unreadable    int `json:"unreadable"`    // not eForms, over a limit, or malformed
	Unsupported   int `json:"unsupported"`   // a shape no field map covers: never read by guesswork
	Awards        int `json:"awards"`        // result notices: not tenders to bid on
	Planning      int `json:"planning"`      // prior information notices: tenders announced, not yet open
	Opportunities int `json:"opportunities"` // current contract notices, which the gate judges
	Superseded    int `json:"superseded"`    // earlier notices of a tender whose current notice is counted above
	// Gate counts the opportunities by verdict: pass, conditional and fail.
	Gate map[string]int `json:"gate"`
	// Matched is how many opportunities the request's filter lets through, before the limit.
	Matched int `json:"matched"`
}

// Matches is the answer to a match request.
type Matches struct {
	NextCursor     string `json:"next_cursor,omitempty"`
	PreviousCursor string `json:"previous_cursor,omitempty"`
	Page           int    `json:"page,omitempty"`
	PageSize       int    `json:"page_size,omitempty"`
	TotalMatched   int    `json:"total_matched"`
	InboxTotal     int    `json:"inbox_total"`
	assessedIDs    []string
	evidenceKey    string
	validUntil     time.Time
	ActiveAssessed int            `json:"active_assessed"`
	TrackedResults []Result       `json:"tracked_results"`
	Coverage       string         `json:"coverage"`
	AsOf           time.Time      `json:"as_of"`
	Strictness     string         `json:"strictness"`
	Company        *CompanyStatus `json:"company,omitempty"`
	// Documents is present when the request was made for an organisation and uploaded documents are
	// configured: it says whether they were used.
	Documents *DocumentsStatus `json:"documents,omitempty"`
	Funnel    Funnel           `json:"funnel"`
	// GainedByWidening says, for each strictness wider than the requested one, how many more
	// tenders a company could still bid on it would show: "+N if you widen to wide".
	GainedByWidening map[string]int `json:"gained_by_widening"`
	Results          []Result       `json:"results"`
}

// Audit is everything behind one assessment, for a person to check it: the result, what the notice
// says, the evidence it was read from, what normalising changed, and what the parser read and did not.
type Audit struct {
	Result  Result         `json:"result"`
	Company *CompanyStatus `json:"company,omitempty"`
	// Documents is what the organisation's uploaded tender documents for this notice held, with the
	// passages found. It is absent when it uploaded none. DocumentsStatus says whether they were
	// looked for.
	Documents       *AuditDocuments   `json:"documents,omitempty"`
	DocumentsStatus *DocumentsStatus  `json:"documents_status,omitempty"`
	Notice          NoticeView        `json:"notice"`
	Evidence        Evidence          `json:"evidence"`
	Normalisation   NormalisationView `json:"normalisation"`
	Parse           ParseView         `json:"parse"`
	// Incumbency is the tender's history with its buyer, made from the award notices among the newest
	// stored notices; IncumbencyStatus says whether it could be made and what it read.
	Incumbency       *IncumbencyView   `json:"incumbency,omitempty"`
	IncumbencyStatus *IncumbencyStatus `json:"incumbency_status,omitempty"`
	Amendments       *AmendmentsView   `json:"amendments,omitempty"`
}

// AuditDocuments is what uploaded documents held, for a person to check: how many files were read and
// how many could not be, and the passages that name a capability, of every class, capped.
type AuditDocuments struct {
	Read          int           `json:"read"`
	Unread        int           `json:"unread"`
	Findings      []FindingView `json:"findings"`
	FindingsTotal int           `json:"findings_total"`
}

// FindingView is one passage of an uploaded document that names a capability. The class is a guess by
// keyword (requirement, award, negated or mention), and the sentence is what it was judged on.
type FindingView struct {
	Document   string `json:"document"`
	Capability string `json:"capability"`
	Class      string `json:"class"`
	Page       int    `json:"page,omitempty"`
	Paragraph  int    `json:"paragraph,omitempty"`
	Snippet    string `json:"snippet"`
}

func auditDocuments(ev match.Evidence) *AuditDocuments {
	if ev.Documents == 0 && ev.Unread == 0 && len(ev.Findings) == 0 {
		return nil
	}
	shown, total := capped(ev.Findings)
	d := &AuditDocuments{Read: ev.Documents, Unread: ev.Unread, Findings: make([]FindingView, 0, len(shown)), FindingsTotal: total + ev.FindingsDropped}
	for _, f := range shown {
		d.Findings = append(d.Findings, FindingView{
			Document: clip(f.Document, maxTextRunes), Capability: string(f.Capability), Class: string(f.Class),
			Page: f.Page, Paragraph: f.Paragraph, Snippet: clip(f.Snippet, maxTextRunes),
		})
	}
	return d
}

// NoticeView is what the notice states, clipped and capped.
type SourceChangeView struct {
	Replaces   string   `json:"replaces"`
	ReasonCode string   `json:"reason_code"`
	Reason     string   `json:"reason"`
	Sections   []string `json:"sections"`
}
type NoticeAwardView struct {
	Lot             string     `json:"lot"`
	Status          string     `json:"status"`
	TendersReceived *int       `json:"tenders_received,omitempty"`
	Value           *Money     `json:"value,omitempty"`
	ContractSigned  *time.Time `json:"contract_signed,omitempty"`
}
type NoticeView struct {
	ExclusionCriteria      []CriterionView   `json:"exclusion_criteria"`
	Awards                 []NoticeAwardView `json:"awards"`
	AwardDate              *time.Time        `json:"award_date,omitempty"`
	Change                 *SourceChangeView `json:"change,omitempty"`
	Description            string            `json:"description"`
	ProcurementType        string            `json:"procurement_type,omitempty"`
	Lots                   []LotView         `json:"lots"`
	LotsTotal              int               `json:"lots_total"`
	SelectionCriteria      []CriterionView   `json:"selection_criteria"`
	SelectionCriteriaTotal int               `json:"selection_criteria_total"`
	// ExclusionGrounds counts the ESPD exclusion grounds the notice lists: every tenderer
	// declares them, and the assessment treats them as one self-declared check.
	ExclusionGrounds int `json:"exclusion_grounds"`
	// CriteriaInDocuments is true when the notice says its selection criteria are in the tender
	// documents, which are not read yet.
	CriteriaInDocuments bool           `json:"criteria_in_documents"`
	Documents           []DocumentView `json:"documents"`
	DocumentsTotal      int            `json:"documents_total"`
}

// LotView is one lot of the notice.
type LotView struct {
	ID                    string               `json:"id"`
	Title                 string               `json:"title,omitempty"`
	CPV                   []string             `json:"cpv"`
	Value                 *Money               `json:"value,omitempty"`
	TenderDeadline        *time.Time           `json:"tender_deadline,omitempty"`
	ParticipationDeadline *time.Time           `json:"participation_deadline,omitempty"`
	Renewals              *int                 `json:"renewals,omitempty"`
	AwardCriteria         []AwardCriterionView `json:"award_criteria"`
}

// AwardCriterionView is a criterion a lot is awarded on. The weight is set only when the notice
// states it as an exact percentage.
type AwardCriterionView struct {
	Type          string `json:"type"`
	Name          string `json:"name,omitempty"`
	Description   string `json:"description,omitempty"`
	WeightPercent *int   `json:"weight_percent,omitempty"`
}

// CriterionView is a selection criterion: what a tenderer must show to qualify.
type CriterionView struct {
	Lot         string `json:"lot,omitempty"`
	Type        string `json:"type"`
	Name        string `json:"name,omitempty"`
	Description string `json:"description,omitempty"`
}

// DocumentView is a link the notice gives to its tender documents. It is the buyer's own link; it
// is never followed.
type DocumentView struct {
	Lot string `json:"lot,omitempty"`
	URL string `json:"url"`
}

// Evidence names the stored bytes the audit was read from.
type Evidence struct {
	Source    string    `json:"source"`
	NoticeID  string    `json:"notice_id"`
	SHA256    string    `json:"sha256"`
	FetchedAt time.Time `json:"fetched_at"`
	Bytes     int       `json:"bytes"`
}

// ChangeView is one correction normalising made.
type ChangeView struct {
	Field string `json:"field"`
	From  string `json:"from"`
	To    string `json:"to"`
	Rule  string `json:"rule"`
}

// NormalisationView lists what normalising changed and what it could not fix.
type NormalisationView struct {
	Changes  []ChangeView `json:"changes"`
	Problems []string     `json:"problems"`
}

// ParseView says what the parser did with the notice: its shape, what it stated it could not
// read, and which elements it did not read at all.
type ParseView struct {
	Root          string   `json:"root"`
	SDKVersion    string   `json:"sdk_version"`
	Flavour       string   `json:"flavour"`
	NoticeUUID    string   `json:"notice_uuid"`
	ContactPoints int      `json:"contact_points"`
	Issues        []string `json:"issues"`
	IssuesTotal   int      `json:"issues_total"`
	Unread        []string `json:"unread"`
	UnreadTotal   int      `json:"unread_total"`
}

func newAudit(doc rawstore.Document, c candidate, a match.Assessment, ev match.Evidence, now time.Time) Audit {
	n := c.Notice
	view := NoticeView{
		Description: clip(n.Description, maxTextRunes), ProcurementType: n.ProcurementType,
		ExclusionCriteria: []CriterionView{}, Awards: []NoticeAwardView{}, AwardDate: utc(n.AwardDate),
		Lots: []LotView{}, SelectionCriteria: []CriterionView{}, Documents: []DocumentView{},
	}

	lots, total := capped(n.Lots)
	view.LotsTotal = total
	for _, lot := range lots {
		view.Lots = append(view.Lots, newLotView(lot))
	}
	criteria, total := capped(n.SelectionCriteria)
	view.SelectionCriteriaTotal = total
	for _, sc := range criteria {
		view.SelectionCriteria = append(view.SelectionCriteria, CriterionView{
			Lot: sc.Lot, Type: sc.Type, Name: clip(sc.Name, maxTextRunes), Description: clip(sc.Description, maxTextRunes),
		})
	}
	documents, total := capped(n.Documents)
	view.DocumentsTotal = total
	for _, d := range documents {
		view.Documents = append(view.Documents, DocumentView{Lot: d.Lot, URL: clip(d.URI, maxTextRunes)})
	}
	for _, req := range n.Requirements {
		switch {
		case strings.HasPrefix(req.Type, eforms.RequirementExclusionGround):
			view.ExclusionGrounds++
			if len(view.ExclusionCriteria) < maxListed {
				view.ExclusionCriteria = append(view.ExclusionCriteria, CriterionView{Lot: req.Lot, Type: req.Type, Description: clip(req.Text, maxTextRunes)})
			}
		case req.Type == eforms.RequirementInTenderDocuments:
			view.CriteriaInDocuments = true
		}
	}

	if n.Change != nil {
		view.Change = &SourceChangeView{Replaces: clip(n.Change.Replaces, maxTextRunes), ReasonCode: n.Change.ReasonCode, Reason: clip(n.Change.Reason, maxTextRunes), Sections: nonNil(firstN(n.Change.Sections, maxListed))}
	}
	results, _ := capped(n.Results)
	for _, result := range results {
		view.Awards = append(view.Awards, NoticeAwardView{Lot: result.Lot, Status: result.Status, TendersReceived: result.TendersReceived, Value: money(result.Value), ContractSigned: utc(result.ContractSigned)})
	}
	changes := make([]ChangeView, 0, len(c.Norm.Changes))
	for _, ch := range c.Norm.Changes {
		changes = append(changes, ChangeView{Field: ch.Field, From: ch.From, To: ch.To, Rule: ch.Rule})
	}
	issues, _ := capped(unique(n.Issues))

	return Audit{
		Result:        newResult(doc.ID, c, a, ev, now),
		Documents:     auditDocuments(ev),
		Notice:        view,
		Evidence:      Evidence{Source: "doffin", NoticeID: doc.ID, SHA256: doc.SHA256, FetchedAt: doc.FetchedAt.UTC(), Bytes: len(doc.Body)},
		Normalisation: NormalisationView{Changes: changes, Problems: nonNil(c.Norm.Problems)},
		Parse: ParseView{
			Root: n.Root, SDKVersion: n.SDKVersion, Flavour: string(n.Flavour), NoticeUUID: n.NoticeID,
			ContactPoints: n.ContactPoints,
			Issues:        nonNil(issues), IssuesTotal: len(n.Issues),
			Unread: nonNil(firstN(n.Unmapped, maxUnread)), UnreadTotal: len(n.Unmapped),
		},
	}
}

func newLotView(lot eforms.Lot) LotView {
	v := LotView{
		ID: lot.ID, Title: clip(lot.Title, maxTextRunes), CPV: nonNil(lot.CPVMain), Value: money(lot.Value),
		TenderDeadline: utc(lot.Deadline), ParticipationDeadline: utc(lot.ParticipationDeadline),
		AwardCriteria: []AwardCriterionView{}, Renewals: lot.Renewals,
	}
	criteria, _ := capped(lot.AwardCriteria)
	for _, c := range criteria {
		v.AwardCriteria = append(v.AwardCriteria, AwardCriterionView{
			Type: c.Type, Name: clip(c.Name, maxTextRunes), Description: clip(c.Description, maxTextRunes), WeightPercent: c.Weight,
		})
	}
	return v
}

func utc(t *time.Time) *time.Time {
	if t == nil {
		return nil
	}
	u := t.UTC()
	return &u
}

// clip shortens s to at most max runes, ending in an ellipsis when it cut anything.
func clip(s string, max int) string {
	if utf8.RuneCountInString(s) <= max {
		return s
	}
	return string([]rune(s)[:max]) + "…"
}

// capped returns at most maxListed of the items, and how many there were.
func capped[T any](items []T) ([]T, int) {
	if len(items) > maxListed {
		return items[:maxListed], len(items)
	}
	return items, len(items)
}

func firstN[T any](items []T, n int) []T {
	if len(items) > n {
		return items[:n]
	}
	return items
}

func unique(in []string) []string {
	seen := make(map[string]bool, len(in))
	var out []string
	for _, s := range in {
		if !seen[s] {
			seen[s] = true
			out = append(out, s)
		}
	}
	return out
}

// nonNil makes a nil list an empty one, so it is [] in JSON and not null.
func nonNil[T any](in []T) []T {
	if in == nil {
		return []T{}
	}
	return in
}
