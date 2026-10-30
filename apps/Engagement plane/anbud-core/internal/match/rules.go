package match

import (
	"strings"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/orgcore"
)

// Capability is something a company holds or can obtain that a qualification requirement asks
// for: a certificate, a registration, a declaration.
type Capability string

// The capabilities the default rules recognise.
const (
	CapESPD           Capability = "espd"
	CapHMSDeclaration Capability = "hms_declaration"
	CapTaxCertificate Capability = "tax_certificate"
	CapStartBank      Capability = "startbank"
	CapAchilles       Capability = "achilles"
	CapMiljofyrtarn   Capability = "miljofyrtarn"
	CapISO9001        Capability = "iso9001"
	CapISO14001       Capability = "iso14001"
)

// LeadTime is how long a capability takes to obtain, in days. EquivalentAccepted marks one that
// buyers commonly accept "or equivalent" for, so missing it is never a hard fail on its own.
type LeadTime struct {
	Days               int
	EquivalentAccepted bool
}

// DefaultLeadTimes is a placeholder table, from the 2026-10-10 council review. A profile's
// LeadTimes override it entry by entry.
var DefaultLeadTimes = map[Capability]LeadTime{
	CapESPD:           {Days: 0},
	CapHMSDeclaration: {Days: 0},
	CapTaxCertificate: {Days: 3},
	CapStartBank:      {Days: 14},
	CapAchilles:       {Days: 30},
	CapMiljofyrtarn:   {Days: 120, EquivalentAccepted: true},
	CapISO9001:        {Days: 270, EquivalentAccepted: true},
	CapISO14001:       {Days: 270, EquivalentAccepted: true},
}

// RequirementRule maps a requirement to a capability when its text contains one of the keywords,
// compared in lower case.
type RequirementRule struct {
	Capability Capability
	Keywords   []string
}

// DefaultRequirementRules is a placeholder keyword list. A requirement no rule recognises is
// "unknown": it needs a person or a later reading stage, and it is never passed by default.
var DefaultRequirementRules = []RequirementRule{
	{CapISO14001, []string{"iso 14001", "iso14001", "iso-14001"}},
	{CapISO9001, []string{"iso 9001", "iso9001", "iso-9001"}},
	{CapMiljofyrtarn, []string{"miljøfyrtårn", "miljofyrtarn"}},
	{CapStartBank, []string{"startbank"}},
	{CapAchilles, []string{"achilles"}},
	{CapTaxCertificate, []string{"skatteattest", "attest for skatt"}},
	{CapHMSDeclaration, []string{"hms-egenerklæring", "hms egenerklæring"}},
	{CapESPD, []string{"espd", "egenerklæringsskjema"}},
}

// Profile is what a company is matched on. It is a value: nothing here stores it.
type Profile struct {
	OrgNumber string
	// Blocked is set when the register shows the company bankrupt or being wound up.
	Blocked      bool
	CPV          []string
	NUTS         []string
	MinValue     *eforms.Amount
	MaxValue     *eforms.Amount
	Keywords     []string
	SEOKeywords  []string
	UserKeywords []string
	ExcludedCPV  []string
	Held         map[Capability]bool
	LeadTimes    map[Capability]LeadTime
}

// ProfileFromOrganisation starts a profile from the register: the organisation number and whether
// the company can bid. Scope (CPV, region, value) is not guessed from the NACE code: one NACE code
// covers firms that bid on different things, so the user fills those in.
func ProfileFromOrganisation(o orgcore.Organisation) Profile {
	return Profile{OrgNumber: o.OrgNumber, Blocked: !o.CanBid()}
}

func (p Profile) leadTime(c Capability) LeadTime {
	if lt, ok := p.LeadTimes[c]; ok {
		return lt
	}
	return DefaultLeadTimes[c]
}

// capabilityOf returns the capability a requirement's text names, by the first rule that matches.
func capabilityOf(text string) (Capability, bool) {
	lower := strings.ToLower(text)
	for _, rule := range DefaultRequirementRules {
		for _, k := range rule.Keywords {
			if strings.Contains(lower, k) {
				return rule.Capability, true
			}
		}
	}
	return "", false
}
