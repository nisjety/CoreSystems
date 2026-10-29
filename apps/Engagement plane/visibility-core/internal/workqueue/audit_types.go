package workqueue

import (
	"encoding/json"
	"net/url"
	"regexp"
	"slices"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

// AuditEvidence references a stored Quarry-backed audit, never client-submitted
// page content. It has its own wire variant rather than invented GSC metrics.
type AuditEvidence struct {
	AuditID     string   `json:"auditId"`
	FindingID   string   `json:"findingId"`
	PageURL     string   `json:"pageUrl"`
	Severity    string   `json:"severity"`
	RunID       string   `json:"runId"`
	Fingerprint string   `json:"fingerprint"`
	RelatedURLs []string `json:"relatedUrls"`
}
type AuditVerification struct {
	AuditID     string `json:"auditId"`
	FindingID   string `json:"findingId"`
	PageURL     string `json:"pageUrl"`
	RuleID      string `json:"ruleId"`
	RunID       string `json:"runId"`
	Fingerprint string `json:"fingerprint"`
}
type auditEvidenceJSON struct {
	Kind       string `json:"kind"`
	Key        string `json:"key"`
	ObservedAt string `json:"observedAt"`
	AuditEvidence
}
type auditVerificationJSON struct {
	Status     string `json:"status"`
	ObservedAt string `json:"observedAt"`
	RecordedAt string `json:"recordedAt"`
	AuditVerification
}

var auditIDPattern = regexp.MustCompile(`^audit_[a-f0-9]{32}$`)
var findingIDPattern = regexp.MustCompile(`^finding_[a-f0-9]{32}$`)
var auditRunPattern = regexp.MustCompile(`^run_[0-7][0-9A-HJKMNP-TV-Z]{25}$`)
var fingerprintPattern = regexp.MustCompile(`^blake3:[a-f0-9]{64}$`)

func validAuditRule(value string) bool {
	switch value {
	case "missing_title_signal", "missing_description_signal", "http_error", "duplicate_title_signal", "canonical_target_review", "broken_checked_link", "h1_structure", "heading_sequence", "noindex_directive", "redirect_chain_review", "structured_data_parse", "hreflang_syntax", "canonical_outside_site", "hreflang_reciprocity":
		return true
	}
	return false
}
func validAuditStatus(value string) bool {
	return value == "passed" || value == "failed" || value == "review" || value == "inconclusive"
}
func validAuditEvidence(e Evidence) bool {
	a := e.Audit
	if a == nil || !auditIDPattern.MatchString(a.AuditID) || !findingIDPattern.MatchString(a.FindingID) || !validAuditRule(e.Key) || !validTechnicalStamp(e.ObservedAt) || !validAuditProvenance(a.RunID, a.Fingerprint) {
		return false
	}
	if a.Severity != "error" && a.Severity != "warning" && a.Severity != "review" {
		return false
	}
	u, err := url.Parse(a.PageURL)
	if err != nil || !auditSiteMatches("https://"+u.Host+"/", a) {
		return false
	}
	return e.OpportunityType == "" && e.Period.StartDate == "" && e.Period.EndDate == "" && e.Clicks == 0 && e.Impressions == 0 && e.CTR == 0 && e.Position == 0 && e.Strategy == "" && e.CheckStatus == "" && e.CheckScore == nil && e.LighthouseVersion == ""
}
func validAuditProvenance(runID, fingerprint string) bool {
	return auditRunPattern.MatchString(runID) && fingerprintPattern.MatchString(fingerprint)
}
func auditSiteMatches(site string, a *AuditEvidence) bool {
	if a == nil || !discovery.SafeURL(site, a.PageURL) || a.RelatedURLs == nil || len(a.RelatedURLs) > 100 {
		return false
	}
	seen := map[string]bool{}
	for _, related := range a.RelatedURLs {
		if !discovery.SafeURL(site, related) || seen[related] {
			return false
		}
		seen[related] = true
	}
	return true
}
func normalizedAuditPage(site, page string) string {
	u, err := url.Parse(page)
	s, siteErr := url.Parse(site)
	if err != nil || siteErr != nil {
		return page
	}
	u.Host = s.Host
	return u.String()
}
func validAuditVerification(v *Verification, e Evidence) bool {
	if v == nil || e.Audit == nil || v.Audit == nil || v.Score != nil || v.LighthouseVersion != "" {
		return false
	}
	a := v.Audit
	return validAuditStatus(v.Status) && auditIDPattern.MatchString(a.AuditID) && a.AuditID != e.Audit.AuditID && (a.FindingID == "" || findingIDPattern.MatchString(a.FindingID)) && ((v.Status != "failed" && v.Status != "review") || a.FindingID != "") && a.PageURL == e.Audit.PageURL && a.RuleID == e.Key && validAuditProvenance(a.RunID, a.Fingerprint) && validTechnicalStamp(v.ObservedAt) && validTechnicalStamp(v.RecordedAt) && !newer(v.ObservedAt, v.RecordedAt) && newer(v.ObservedAt, e.ObservedAt)
}
func cloneAuditEvidence(a *AuditEvidence) *AuditEvidence {
	if a == nil {
		return nil
	}
	copy := *a
	copy.RelatedURLs = slices.Clone(a.RelatedURLs)
	return &copy
}
func marshalAuditEvidence(e Evidence) ([]byte, error) {
	if e.Audit == nil {
		return nil, ErrInvalid
	}
	return json.Marshal(auditEvidenceJSON{Kind: e.Kind, Key: e.Key, ObservedAt: e.ObservedAt, AuditEvidence: *e.Audit})
}
func unmarshalAuditEvidence(raw []byte, e *Evidence) error {
	var value auditEvidenceJSON
	if err := decodeStrict(raw, &value); err != nil {
		return err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || len(fields) != 10 {
		return ErrInvalid
	}
	*e = Evidence{Kind: value.Kind, Key: value.Key, ObservedAt: value.ObservedAt, Audit: &value.AuditEvidence}
	return nil
}

type legacyVerification Verification

func (v Verification) MarshalJSON() ([]byte, error) {
	if v.Audit == nil {
		return json.Marshal(legacyVerification(v))
	}
	return json.Marshal(auditVerificationJSON{Status: v.Status, ObservedAt: v.ObservedAt, RecordedAt: v.RecordedAt, AuditVerification: *v.Audit})
}
func (v *Verification) UnmarshalJSON(raw []byte) error {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return err
	}
	if _, ok := fields["auditId"]; ok {
		var value auditVerificationJSON
		if err := decodeStrict(raw, &value); err != nil {
			return err
		}
		if len(fields) != 9 {
			return ErrInvalid
		}
		*v = Verification{Status: value.Status, ObservedAt: value.ObservedAt, RecordedAt: value.RecordedAt, Audit: &value.AuditVerification}
		return nil
	}
	var value legacyVerification
	if err := decodeStrict(raw, &value); err != nil {
		return err
	}
	*v = Verification(value)
	return nil
}
