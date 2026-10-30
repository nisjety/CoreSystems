package match

import (
	"fmt"
	"slices"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/normalize"
)

// Outcome is the result of one check.
type Outcome string

const (
	Pass        Outcome = "pass"
	Conditional Outcome = "conditional"
	Fail        Outcome = "fail"
	Unknown     Outcome = "unknown"
)

// Band is how well a notice fits a profile. There is no percentage.
type Band string

const (
	None    Band = "none"
	Stretch Band = "stretch"
	Good    Band = "good"
	Strong  Band = "strong"
)

// Strictness is how far the fit reaches beyond the profile's own codes.
type Strictness int

const (
	// Strict matches only inside the profile's CPV areas and regions.
	Strict Strictness = iota
	// Balanced also matches a CPV code in the same group (first three digits) and a region in the
	// same NUTS 2 region, and doubles the value range both ways.
	Balanced
	// Wide also matches a CPV code in the same division (first two digits), a region in the same
	// country part (NUTS 1), and drops the value ceiling.
	Wide
)

// The rules a check can name.
const (
	RuleCompany     = "company"
	RuleDeadline    = "deadline"
	RuleExcluded    = "excluded_cpv"
	RuleRequirement = "requirement"
	RuleExclusion   = "exclusion_grounds"
	RuleDocuments   = "criteria_in_documents"
	// RuleDocumentRequirement is a requirement found in a document the company uploaded.
	RuleDocumentRequirement = "document_requirement"
	RuleCPV                 = "cpv"
	RuleRegion              = "region"
	RuleValue               = "value"
	RuleKeywords            = "keywords"
)

// Cite points at where a check's evidence is: a field of the notice, the lot, and for a repeated
// field such as a requirement, its index in the parsed notice. A check read from an uploaded
// document cites the file, the page or paragraph, and the sentence it was judged on.
type Cite struct {
	Field string
	Lot   string
	Index int

	Document  string // the file's path in the uploaded archive
	Page      int
	Paragraph int
	Snippet   string
}

// Check is one rule applied to one notice.
type Check struct {
	Rule     string
	Outcome  Outcome
	Detail   string
	Cite     Cite
	DaysLeft *int // set by deadline and requirement checks
}

// Assessment is the gate and the fit of one notice for one profile.
type Assessment struct {
	Verdict Outcome // pass, conditional or fail: the gate alone
	Band    Band    // the fit alone; shown even when the gate fails, so the reason is clear
	Gate    []Check
	Fit     []Check
}

// Candidate is a notice with its normalised form.
type Candidate struct {
	Notice eforms.Notice
	Norm   normalize.Result
}

// Assess applies the gate and the fit, with no uploaded documents. It is pure: now is passed in.
func Assess(n eforms.Notice, norm normalize.Result, p Profile, s Strictness, now time.Time) Assessment {
	return AssessWith(n, norm, p, s, now, Evidence{})
}

// AssessWith applies the gate and the fit with what the tender documents the company uploaded for
// the notice say. What is read from a document is advice: it is judged like the notice's own
// criteria, but it can make a check conditional and never failed, because a sentence found by keyword
// is not a requirement proved.
func AssessWith(n eforms.Notice, norm normalize.Result, p Profile, s Strictness, now time.Time, ev Evidence) Assessment {
	var a Assessment
	if p.Blocked {
		a.Gate = append(a.Gate, Check{Rule: RuleCompany, Outcome: Fail, Detail: "the register shows the company bankrupt or being wound up"})
	}
	deadline, days := deadlineCheck(norm, now)
	a.Gate = append(a.Gate, deadline)
	a.Gate = append(a.Gate, excludedChecks(norm, p)...)
	a.Gate = append(a.Gate, pointerChecks(n, ev)...)
	a.Gate = append(a.Gate, requirementChecks(n, norm, p, now, days)...)
	a.Gate = append(a.Gate, documentRequirementChecks(n, p, ev, days)...)
	a.Verdict = verdict(a.Gate)

	a.Fit = fitChecks(n, norm, p, s)
	a.Band = band(a.Fit)
	return a
}

// Gained counts the candidates that have no band at strictness from and some band at to, and that
// the gate does not fail: the "+N tenders if you widen" figure counts tenders a company could
// still bid on. The gate does not depend on strictness.
func Gained(candidates []Candidate, p Profile, from, to Strictness, now time.Time) int {
	count := 0
	for _, c := range candidates {
		after := Assess(c.Notice, c.Norm, p, to, now)
		if after.Verdict != Fail && after.Band != None && Assess(c.Notice, c.Norm, p, from, now).Band == None {
			count++
		}
	}
	return count
}

// NextOpenDeadline returns the earliest deadline that has not passed, and false when there is
// none. A deadline that falls exactly now is still open.
func NextOpenDeadline(norm normalize.Result, now time.Time) (normalize.Deadline, bool) {
	var first normalize.Deadline
	found := false
	for _, d := range norm.Deadlines {
		if d.At.Before(now) {
			continue
		}
		if !found || d.At.Before(first.At) {
			first, found = d, true
		}
	}
	return first, found
}

// openDeadlines returns the deadlines that have not passed, and how many were stated. A deadline
// that falls exactly now is still open.
func openDeadlines(norm normalize.Result, now time.Time) (open []normalize.Deadline, stated int) {
	for _, d := range norm.Deadlines {
		if !d.At.Before(now) {
			open = append(open, d)
		}
	}
	return open, len(norm.Deadlines)
}

// deadlineField names the notice field a deadline of this kind comes from.
func deadlineField(kind normalize.DeadlineKind) string {
	if kind == normalize.DeadlineParticipation {
		return "participation deadline"
	}
	return "deadline"
}

// deadlineCheck passes while any stated deadline is open, and reports the Oslo days left to the
// earliest open one and the lot it belongs to. The returned days are nil when none is open. A
// deadline for requests to participate says so, because it is not the deadline for tenders.
func deadlineCheck(norm normalize.Result, now time.Time) (Check, *int) {
	c := Check{Rule: RuleDeadline, Cite: Cite{Field: "deadline"}}
	first, ok := NextOpenDeadline(norm, now)
	switch {
	case len(norm.Deadlines) == 0:
		c.Outcome, c.Detail = Unknown, "the notice states no deadline"
		return c, nil
	case !ok:
		c.Outcome, c.Detail = Fail, "every deadline the notice states has passed"
		return c, nil
	}
	days := normalize.DaysLeft(first.At, now)
	c.Outcome, c.DaysLeft = Pass, &days
	c.Cite = Cite{Field: deadlineField(first.Kind), Lot: first.Lot}
	c.Detail = fmt.Sprintf("%d days left (Oslo calendar days)", days)
	if first.Kind == normalize.DeadlineParticipation {
		c.Detail += " to request to participate"
	}
	return c, &days
}

func excludedChecks(norm normalize.Result, p Profile) []Check {
	var out []Check
	for _, code := range norm.CPV {
		for _, area := range p.ExcludedCPV {
			if within(code, area, normalize.CPVAncestors) {
				out = append(out, Check{Rule: RuleExcluded, Outcome: Fail,
					Detail: fmt.Sprintf("CPV %s is inside the excluded area %s", code, area), Cite: Cite{Field: "cpv"}})
			}
		}
	}
	return out
}

// pointerChecks reads the notice's tendering-qualification entries (eforms Requirements). In the
// captured notices these are not criteria: exg-* are the ESPD exclusion grounds every tenderer
// declares, summed up as one check (a company the register blocks is failed by the company check);
// epo-procurement-document says the criteria are in the tender documents, which nothing here
// reads, so it is unknown; epo-notice, epo-acc-espd-request and none add nothing to check.
func pointerChecks(n eforms.Notice, ev Evidence) []Check {
	var out []Check
	grounds, pointers := 0, 0
	var first Cite
	for i, req := range n.Requirements {
		switch {
		case strings.HasPrefix(req.Type, eforms.RequirementExclusionGround):
			grounds++
		case req.Type == eforms.RequirementInTenderDocuments:
			if pointers == 0 {
				first = Cite{Field: "requirement", Lot: req.Lot, Index: i}
			}
			pointers++
		}
	}
	if grounds > 0 {
		out = append(out, Check{Rule: RuleExclusion, Outcome: Pass,
			Detail: fmt.Sprintf("%d exclusion grounds; the company declares them in the ESPD", grounds),
			Cite:   Cite{Field: "requirement"}})
	}
	if pointers > 0 {
		// A notice that points there once for each lot says one thing, so it is one check.
		out = append(out, Check{Rule: RuleDocuments, Outcome: Unknown, Detail: documentsDetail(pointers, ev), Cite: first})
	}
	return out
}

// documentsDetail says what is known of criteria that are in the tender documents: not read, or read
// as far as the rules go.
func documentsDetail(pointers int, ev Evidence) string {
	switch {
	case ev.Documents == 0 && ev.Unread == 0:
		return fmt.Sprintf("the selection criteria are in the tender documents, which are not read yet (%d entries of the notice say so)", pointers)
	case ev.Documents == 0:
		return fmt.Sprintf("the selection criteria are in the tender documents; %s could not be read (a scan, a damaged file or a kind of file that is not read), so they are still not read", uploaded(ev.Unread))
	}
	detail := fmt.Sprintf("the selection criteria are in the tender documents; %s read", uploadedWere(ev.Documents))
	if ev.Unread > 0 {
		detail += fmt.Sprintf(", and %d could not be read", ev.Unread)
	}
	return detail + ". The rules look for known certificates and registrations only, so turnover, references, staff and other criteria still need a person"
}

// uploaded is "1 uploaded document" or "3 uploaded documents".
func uploaded(n int) string {
	if n == 1 {
		return "1 uploaded document"
	}
	return fmt.Sprintf("%d uploaded documents", n)
}

// uploadedWere is "1 uploaded document was" or "3 uploaded documents were".
func uploadedWere(n int) string {
	if n == 1 {
		return uploaded(n) + " was"
	}
	return uploaded(n) + " were"
}

// documentRequirementChecks makes one check for each capability that the uploaded documents ask
// for, unless the notice's own selection criteria already state it. It is judged like a criterion of
// the notice, and a failure is softened to conditional: the passage was found by keyword, and a person
// reads it. The check cites the first passage that asks for the capability.
func documentRequirementChecks(n eforms.Notice, p Profile, ev Evidence, days *int) []Check {
	covered := map[Capability]bool{}
	for _, sc := range n.SelectionCriteria {
		if c, ok := capabilityOf(sc.Name + "\n" + sc.Description); ok {
			covered[c] = true
		}
	}

	var order []Capability
	first := map[Capability]Finding{}
	passages := map[Capability]int{}
	files := map[Capability]map[string]bool{}
	for _, f := range ev.Findings {
		if f.Class != ClassRequirement || covered[f.Capability] {
			continue
		}
		if _, ok := first[f.Capability]; !ok {
			order = append(order, f.Capability)
			first[f.Capability] = f
			files[f.Capability] = map[string]bool{}
		}
		passages[f.Capability]++
		files[f.Capability][f.Document] = true
	}

	var out []Check
	for _, c := range order {
		f := first[c]
		outcome, detail := Pass, "held: "+string(c)
		if !p.Held[c] {
			outcome, detail = judgeLeadTime(c, p.leadTime(c), days)
			if outcome == Fail {
				outcome = Conditional
			}
		}
		counted := fmt.Sprintf("%s in %s", countOf(passages[c], "passage"), countOf(len(files[c]), "document"))
		if ev.FindingsDropped > 0 {
			// The evidence holds only the best of the findings, so what is counted is of what is held.
			counted = fmt.Sprintf("at least %s in at least %s", countOf(passages[c], "passage"), countOf(len(files[c]), "document"))
		}
		detail += fmt.Sprintf("; asked for in the uploaded documents (%s), found by keyword: read the wording", counted)
		out = append(out, Check{
			Rule: RuleDocumentRequirement, Outcome: outcome, Detail: detail, DaysLeft: days,
			Cite: Cite{Field: "document", Document: f.Document, Page: f.Page, Paragraph: f.Paragraph, Snippet: f.Snippet},
		})
	}
	return out
}

// countOf is "1 passage" or "3 passages".
func countOf(n int, noun string) string {
	if n == 1 {
		return fmt.Sprintf("1 %s", noun)
	}
	return fmt.Sprintf("%d %ss", n, noun)
}

// requirementChecks judges each selection criterion of the notice by its name and description. A
// criterion is judged against its own lot's deadline when that lot has an open one, else the
// earliest open deadline. A criterion no rule recognises is unknown.
func requirementChecks(n eforms.Notice, norm normalize.Result, p Profile, now time.Time, earliest *int) []Check {
	open, _ := openDeadlines(norm, now)
	var out []Check
	for i, req := range n.SelectionCriteria {
		c := Check{Rule: RuleRequirement, Cite: Cite{Field: "selection criterion", Lot: req.Lot, Index: i}}
		days := earliest
		if req.Lot != "" {
			for _, d := range open {
				if d.Lot == req.Lot {
					left := normalize.DaysLeft(d.At, now)
					days = &left
				}
			}
		}
		c.DaysLeft = days
		capability, ok := capabilityOf(req.Name + "\n" + req.Description)
		switch {
		case !ok:
			c.Outcome, c.Detail = Unknown, "no rule recognises this requirement; it needs reading"
		case p.Held[capability]:
			c.Outcome, c.Detail = Pass, fmt.Sprintf("held: %s", capability)
		default:
			c.Outcome, c.Detail = judgeLeadTime(capability, p.leadTime(capability), days)
		}
		out = append(out, c)
	}
	return out
}

func judgeLeadTime(c Capability, lt LeadTime, days *int) (Outcome, string) {
	switch {
	case days != nil && lt.Days <= *days:
		return Conditional, fmt.Sprintf("not held: %s takes about %d days, %d days left", c, lt.Days, *days)
	case lt.EquivalentAccepted:
		return Conditional, fmt.Sprintf("not held: %s takes about %d days, but equivalent documentation is often accepted", c, lt.Days)
	case days == nil:
		return Conditional, fmt.Sprintf("not held: %s takes about %d days, and no open deadline is known", c, lt.Days)
	}
	return Fail, fmt.Sprintf("not held: %s takes about %d days, only %d days left", c, lt.Days, *days)
}

// verdict is fail when any gate check fails, conditional when any is conditional or unknown, and
// pass otherwise.
func verdict(gate []Check) Outcome {
	v := Pass
	for _, c := range gate {
		switch c.Outcome {
		case Fail:
			return Fail
		case Conditional, Unknown:
			v = Conditional
		}
	}
	return v
}

func fitChecks(n eforms.Notice, norm normalize.Result, p Profile, s Strictness) []Check {
	checks := []Check{cpvCheck(norm, p, s), regionCheck(norm, p, s), valueCheck(n, p, s), keywordCheck(n, p)}
	if len(p.SEOKeywords) > 0 {
		checks = append(checks, contextKeywordCheck(n, p.SEOKeywords, "company_seo"))
	}
	if len(p.UserKeywords) > 0 {
		checks = append(checks, contextKeywordCheck(n, p.UserKeywords, "user_focus"))
	}
	return checks
}

// cpvNear and nutsNear are how many leading characters two codes must share to be near at each step:
// balanced reaches the same CPV group (three digits) and the same NUTS 2 region (four characters), wide
// the same CPV division (two digits) and the same NUTS 1 country part (three characters). Strict shares
// none.
var (
	cpvNear  = map[Strictness]int{Strict: 0, Balanced: 3, Wide: 2}
	nutsNear = map[Strictness]int{Strict: 0, Balanced: 4, Wide: 3}
)

// CodeReaches reports whether a notice's codes reach a profile's at a strictness step, by the rule the
// gate's CPV and region checks use: a code inside a profile area or covering one (the gate's pass at
// every step), or, beyond strict, a code near one (the gate's conditional). With cpv it compares CPV
// codes, otherwise NUTS regions. Empty lists reach nothing. The radar uses this function too, and a test
// pins it against the checks themselves.
func CodeReaches(notice, profile []string, s Strictness, cpv bool) bool {
	ancestors, near := normalize.NUTSAncestors, nutsNear[s]
	if cpv {
		ancestors, near = normalize.CPVAncestors, cpvNear[s]
	}
	outcome, _, _ := relate(notice, profile, ancestors, near)
	return outcome != Fail
}

// within reports whether code lies inside area: equal, or area is one of its ancestors.
func within(code, area string, ancestors func(string) []string) bool {
	return code == area || slices.Contains(ancestors(code), area)
}

// relate compares notice codes with profile codes: pass when a notice code is inside a profile
// area or a notice code covers a profile area (a national notice covers every region), conditional
// when they share a prefix of the given length, and fail otherwise.
func relate(notice, profile []string, ancestors func(string) []string, near int) (Outcome, string, string) {
	best, detail := Fail, ""
	for _, n := range notice {
		for _, p := range profile {
			switch {
			case within(n, p, ancestors) || within(p, n, ancestors):
				return Pass, n, p
			case near > 0 && len(n) >= near && len(p) >= near && n[:near] == p[:near]:
				best, detail = Conditional, n+" near "+p
			}
		}
	}
	return best, detail, ""
}

func cpvCheck(norm normalize.Result, p Profile, s Strictness) Check {
	c := Check{Rule: RuleCPV, Cite: Cite{Field: "cpv"}}
	if len(p.CPV) == 0 || len(norm.CPV) == 0 {
		c.Outcome, c.Detail = Unknown, "no CPV code to compare"
		return c
	}
	near := cpvNear[s]
	outcome, a, b := relate(norm.CPV, p.CPV, normalize.CPVAncestors, near)
	c.Outcome = outcome
	switch outcome {
	case Pass:
		c.Detail = fmt.Sprintf("CPV %s is within %s", a, b)
	case Conditional:
		c.Detail = "CPV " + a
	default:
		c.Detail = "no CPV code is near the profile's"
	}
	return c
}

func regionCheck(norm normalize.Result, p Profile, s Strictness) Check {
	c := Check{Rule: RuleRegion, Cite: Cite{Field: "nuts"}}
	if len(p.NUTS) == 0 || len(norm.NUTS) == 0 {
		c.Outcome, c.Detail = Unknown, "no region to compare"
		return c
	}
	near := nutsNear[s]
	outcome, a, b := relate(norm.NUTS, p.NUTS, normalize.NUTSAncestors, near)
	c.Outcome = outcome
	switch outcome {
	case Pass:
		c.Detail = fmt.Sprintf("region %s matches %s", a, b)
	case Conditional:
		c.Detail = "region " + a
	default:
		c.Detail = "no region is near the profile's"
	}
	return c
}

// valueCheck compares the notice's estimated value, or its largest lot value, with the profile's
// range in the same currency. Strict keeps the range; balanced halves the floor and doubles the
// ceiling; wide quarters the floor and drops the ceiling.
func valueCheck(n eforms.Notice, p Profile, s Strictness) Check {
	c := Check{Rule: RuleValue, Cite: Cite{Field: "estimated value"}}
	value := ValueOf(n)
	if value != nil && n.EstimatedValue == nil {
		c.Cite = Cite{Field: "lot value", Lot: largestLot(n).ID}
	}
	if value == nil || (p.MinValue == nil && p.MaxValue == nil) {
		c.Outcome, c.Detail = Unknown, "no value to compare"
		return c
	}
	for _, bound := range []*eforms.Amount{p.MinValue, p.MaxValue} {
		if bound != nil && !strings.EqualFold(bound.Currency, value.Currency) {
			c.Outcome, c.Detail = Unknown, "the value is in another currency than the profile's range"
			return c
		}
	}
	floorDiv := map[Strictness]int64{Strict: 1, Balanced: 2, Wide: 4}[s]
	ceilMul := map[Strictness]int64{Strict: 1, Balanced: 2, Wide: 0}[s]
	if p.MinValue != nil && value.Minor < p.MinValue.Minor/floorDiv {
		c.Outcome, c.Detail = Fail, "the value is below the profile's range"
		return c
	}
	if p.MaxValue != nil && ceilMul > 0 && value.Minor > p.MaxValue.Minor*ceilMul {
		c.Outcome, c.Detail = Fail, "the value is above the profile's range"
		return c
	}
	c.Outcome, c.Detail = Pass, "the value is within the profile's range"
	return c
}

// ValueOf is the value a notice is judged and shown by: its own estimate when it states one, else
// the largest value of any lot, or nil when it states none.
func ValueOf(n eforms.Notice) *eforms.Amount {
	if n.EstimatedValue != nil {
		return n.EstimatedValue
	}
	if lot := largestLot(n); lot.Value != nil {
		return lot.Value
	}
	return nil
}

// largestLot is the lot with the largest value, or the zero Lot when none states one.
func largestLot(n eforms.Notice) eforms.Lot {
	var best eforms.Lot
	for _, lot := range n.Lots {
		if lot.Value != nil && (best.Value == nil || lot.Value.Minor > best.Value.Minor) {
			best = lot
		}
	}
	return best
}

// keywordCheck looks for the profile's keywords in the title, the description and lot titles.
func keywordCheck(n eforms.Notice, p Profile) Check {
	c := Check{Rule: RuleKeywords, Cite: Cite{Field: "title"}}
	if len(p.Keywords) == 0 {
		c.Outcome, c.Detail = Unknown, "the profile has no keywords"
		return c
	}
	texts := []string{n.Title, n.Description}
	for _, lot := range n.Lots {
		texts = append(texts, lot.Title)
	}
	haystack := strings.ToLower(strings.Join(texts, "\n"))
	var found []string
	for _, k := range p.Keywords {
		k = strings.TrimSpace(k)
		if k == "" {
			continue
		}
		matches := strings.Contains(haystack, strings.ToLower(k))
		if utf8.RuneCountInString(k) <= 3 {
			matches = contextKeywordCheck(n, []string{k}, RuleKeywords).Outcome == Pass
		}
		if matches {
			found = append(found, k)
		}
	}
	if len(found) == 0 {
		c.Outcome, c.Detail = Fail, "no keyword found"
		return c
	}
	c.Outcome, c.Detail = Pass, "keywords found: "+strings.Join(found, ", ")
	return c
}

// band turns the fit into a band. Strong: the CPV is inside the profile and neither region nor
// value is outside. Good: some scope signal (CPV inside or near, or a keyword) and nothing outside.
// Stretch: a scope signal with one or more of region and value outside. None: no scope signal.
func band(fit []Check) Band {
	outcome := map[string]Outcome{}
	for _, c := range fit {
		outcome[c.Rule] = c.Outcome
	}
	cpv := outcome[RuleCPV]
	scope := cpv == Pass || cpv == Conditional || outcome[RuleKeywords] == Pass || outcome["company_seo"] == Pass
	if !scope {
		return None
	}
	outside := outcome[RuleRegion] == Fail || outcome[RuleValue] == Fail
	switch {
	case outside:
		return Stretch
	case cpv == Pass:
		return Strong
	}
	return Good
}
