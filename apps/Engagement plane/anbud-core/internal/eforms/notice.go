package eforms

import (
	"errors"
	"time"
)

// ErrNoFieldMap means the notice is well formed, but this code has no field map for its root
// element, SDK version and flavour. It is refused, never parsed by guesswork: a shape nobody has
// checked against real notices gets no fields at all.
var ErrNoFieldMap = errors.New("eforms: no field map for this root, SDK version and flavour; not parsed")

// AccessUnknown is the access of a document whose access flag the parser does not map. It is not
// "unrestricted": a document is taken as open only when a notice says so.
const AccessUnknown = ""

// Amount is a money value in minor units (øre for NOK). It is exact: no floating point.
type Amount struct {
	Minor    int64
	Currency string
}

// DocumentRef is a link a notice gives to its tender documents, with the lot it belongs to.
type DocumentRef struct {
	Lot    string // the lot's ID, or "" for a link of the notice as a whole
	URI    string
	Access string // "unrestricted", "restricted" or AccessUnknown
}

// The type codes of the entries a notice lists as tendering-qualification requirements, which in
// the captured notices are not the qualification criteria themselves (those are SelectionCriteria)
// but the ESPD exclusion grounds every tenderer declares, and pointers to where the criteria are.
const (
	// RequirementExclusionGround is the prefix of the exclusion-ground codes (exg-crim-fraud, ...).
	RequirementExclusionGround = "exg-"
	// RequirementInTenderDocuments says the criteria are in the tender documents.
	RequirementInTenderDocuments = "epo-procurement-document"
)

// Requirement is a qualification requirement the notice states: what a tenderer must show. Type is
// the code the notice gives; Text is the description as written. Lot is the lot's ID, or "" for a
// requirement of the notice as a whole.
type Requirement struct {
	Lot  string
	Type string
	Text string
}

// AwardCriterion is one criterion a lot is awarded on. Type is the code the notice gives (price,
// quality, cost); Weight is set only when the notice states the weight as a percentage, never from
// an order of importance or another kind of parameter.
type AwardCriterion struct {
	Type        string
	Name        string
	Description string
	Weight      *int
}

// SelectionCriterion is a selection criterion a lot states: what a tenderer must show to qualify,
// such as references, turnover or a certificate. Type is the selection-criterion code
// (slc-abil-ref-services, ...) or "other". Name and Description are the Norwegian text when the
// notice gives one, else the first language given. A criterion the notice marks not used is left
// out.
type SelectionCriterion struct {
	Lot         string
	Type        string
	Name        string
	Description string
}

// Lot is one part of a procurement, as the notice states it.
type Lot struct {
	ID       string
	Title    string
	CPVMain  []string
	Value    *Amount
	Deadline *time.Time // the deadline for tenders (BT-131)
	// ParticipationDeadline is the deadline for requests to participate (BT-1311): a restricted
	// procedure states this one, and tenders are invited later. Either, both or neither may be set.
	ParticipationDeadline *time.Time
	AwardCriteria         []AwardCriterion
	// Period is the contract's duration as the notice states it (BT-536, BT-537, BT-36), or nil
	// when it states none. Renewals is the most renewals the buyer may make (BT-58), or nil.
	Period   *ContractPeriod
	Renewals *int
}

// ContractPeriod is how long a lot's contract runs, in whichever form the notice gives: a start
// and an end (calendar days, at midnight UTC), a duration, or open-ended. Start may come with a
// duration and no end. The parser does not turn a duration into an end: that needs a start, and
// for an award a signing date, and the reader that has both decides.
type ContractPeriod struct {
	Start     *time.Time
	End       *time.Time
	Duration  *Duration
	Unlimited bool // the notice says the contract has no end (timeperiod UNLIMITED)
}

// Duration is a length with its unit from the eForms duration-unit code list: DAY, WEEK, MONTH or
// YEAR.
type Duration struct {
	Value int
	Unit  string
}

// Winner is an organisation an award notice names as the winning tenderer. A sole trader's name is
// a person's name, so a Winner can be personal data; see docs/PERSONAL-DATA.md.
type Winner struct {
	Name string
	// OrgNumber is the registry identifier the notice gives, with its spaces removed, or "" when it
	// gives none or what it gives is not shaped like an identifier. It is not checked to be a
	// Norwegian organisation number: a winner may be foreign. normalize.OrgNumber says which.
	OrgNumber string
}

// LotResult is the outcome of one lot in an award notice. Status is the winner-selection code the
// notice gives (selec-w: a winner was chosen; clos-nw and open-nw: no winner). TendersReceived is
// the count of tenders the notice states, or nil when it states none. A winner the parser cannot
// follow from the result to the organisation list is an issue, never a guess.
type LotResult struct {
	Lot             string
	Status          string
	TendersReceived *int
	Winners         []Winner
	Value           *Amount // the value of the winning tender, when the notice states it
	// ContractSigned is the day the lot's contract was concluded (BT-145), at midnight UTC, or nil
	// when the notice states none.
	ContractSigned *time.Time
}

// Notice is the parsed view of an eForms notice: stage 2 of the pipeline. Only what a field map
// reads is set. Contact persons are counted and never copied, so no name, e-mail address or
// telephone number of a person is in a Notice. Issues records what a notice states and the parser
// could not read, in fixed text without the value. Unmapped lists the elements no field map read,
// by path of local names, so that what the parser does not understand is visible.
// ContractingSystem is a source-coded framework agreement or dynamic purchasing system.
type ContractingSystem struct {
	Kind string
	Code string
}

type Notice struct {
	Header

	Title              string
	Description        string
	ProcurementType    string
	ProcedureType      string
	ContractingSystems []ContractingSystem
	EstimatedValue     *Amount
	CPVMain            []string
	CPVAdditional      []string
	NUTS               []string
	Deadline           *time.Time // the deadline for tenders; see Lot for the two kinds
	// ParticipationDeadline is the notice-level deadline for requests to participate.
	ParticipationDeadline *time.Time
	BuyerNames            []string
	BuyerOrgNumbers       []string
	ContactPoints         int
	Documents             []DocumentRef
	Requirements          []Requirement
	// SelectionCriteria are the qualification criteria. Requirements, despite the name, mostly hold
	// ESPD exclusion grounds (exg-*) and pointers to where the criteria are (epo-*).
	SelectionCriteria []SelectionCriterion
	Lots              []Lot

	// PlannedNoticeDate is the day a prior information notice says the notice it announces will be
	// published (BT-127), at midnight UTC, or nil: other notices state none.
	PlannedNoticeDate *time.Time

	// Set by award notices only.
	AwardedValue *Amount
	AwardDate    *time.Time // the calendar day, at midnight UTC
	Results      []LotResult

	// IssuedAt is when the notice was issued (BT-803): its issue date and issue time, as an instant. It
	// is nil when the notice gives no usable one; the date alone is not an instant. It is what orders
	// the notices of a procedure.
	IssuedAt *time.Time
	// Change is the change section of a notice that changes an earlier one (BT-758, BT-140, BT-141,
	// BT-13716), or nil when it has none.
	Change *ChangeInfo

	Issues   []string
	Unmapped []string
}

// ChangeInfo is what a change notice says about itself. Replaces is the reference to the notice it
// changes, as written: either the notice identifier and version ("<uuid>-01"), which a stored notice
// can be matched to, or the TED publication number ("123456-2026"), which none can: a publication
// number is not part of an eForms body. ReasonCode is from the change-corrig-justification code list
// (update-add, ...). Reason is the buyer's own description, the Norwegian text when there is one; it
// is free text and may name a person. Sections are the identifiers of the parts that changed.
type ChangeInfo struct {
	Replaces   string
	ReasonCode string
	Reason     string
	Sections   []string
}

// ReplacedNotice returns the identifier and version of the notice the change replaces, when the
// reference has that form. A TED publication number does not, and gives ok false.
func (c ChangeInfo) ReplacedNotice() (id, version string, ok bool) {
	m := replacedNoticeForm.FindStringSubmatch(c.Replaces)
	if m == nil {
		return "", "", false
	}
	return m[1], m[2], true
}
