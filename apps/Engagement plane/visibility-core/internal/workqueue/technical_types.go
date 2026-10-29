package workqueue

import (
	"bytes"
	"encoding/json"
	"io"
	"regexp"
	"time"
)

type Verification struct {
	Review            *ReviewVerification `json:"review,omitempty"`
	Status            string              `json:"status"`
	Score             *float64            `json:"score"`
	ObservedAt        string              `json:"observedAt"`
	LighthouseVersion string              `json:"lighthouseVersion"`
	RecordedAt        string              `json:"recordedAt"`
	Audit             *AuditVerification  `json:"-"`
}

// This flat variant deliberately excludes every GSC-only field, including zeros.
type technicalEvidence struct {
	Kind              string   `json:"kind"`
	Key               string   `json:"key"`
	ObservedAt        string   `json:"observedAt"`
	Strategy          string   `json:"strategy"`
	CheckStatus       string   `json:"checkStatus"`
	CheckScore        *float64 `json:"checkScore"`
	LighthouseVersion string   `json:"lighthouseVersion"`
}
type gscEvidence Evidence

func (e Evidence) MarshalJSON() ([]byte, error) {
	if e.Kind == "search_group_review" {
		return marshalReview(e)
	}
	if e.Kind == "site_audit_finding" {
		return marshalAuditEvidence(e)
	}
	if e.Kind == "pagespeed_check" {
		return json.Marshal(technicalEvidence{e.Kind, e.Key, e.ObservedAt, e.Strategy, e.CheckStatus, e.CheckScore, e.LighthouseVersion})
	}
	return json.Marshal(gscEvidence(e))
}
func (e *Evidence) UnmarshalJSON(raw []byte) error {
	var kind struct {
		Kind string `json:"kind"`
	}
	if err := json.Unmarshal(raw, &kind); err != nil {
		return err
	}
	if kind.Kind == "search_group_review" {
		return unmarshalReview(raw, e)
	}
	if kind.Kind == "site_audit_finding" {
		return unmarshalAuditEvidence(raw, e)
	}
	if kind.Kind == "pagespeed_check" {
		var value technicalEvidence
		if err := decodeStrict(raw, &value); err != nil {
			return err
		}
		var fields map[string]json.RawMessage
		if json.Unmarshal(raw, &fields) != nil || len(fields) != 7 {
			return ErrInvalid
		}
		*e = Evidence{Kind: value.Kind, Key: value.Key, ObservedAt: value.ObservedAt, Strategy: value.Strategy, CheckStatus: value.CheckStatus, CheckScore: cloneScore(value.CheckScore), LighthouseVersion: value.LighthouseVersion}
		return nil
	}
	var value gscEvidence
	if err := decodeStrict(raw, &value); err != nil {
		return err
	}
	*e = Evidence(value)
	return nil
}
func decodeStrict(raw []byte, value any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return ErrInvalid
	}
	return nil
}
func validStrategy(value string) bool { return value == "mobile" || value == "desktop" }
func validCheckID(value string) bool {
	switch value {
	case "document-title", "meta-description", "http-status-code", "is-crawlable", "robots-txt", "canonical", "hreflang", "link-text", "crawlable-anchors", "image-alt", "structured-data":
		return true
	}
	return false
}
func validCheck(status string, score *float64) bool {
	switch status {
	case "passed":
		return score != nil && *score == 1
	case "failed":
		return score != nil && *score == 0
	case "not_applicable", "manual", "informative", "error", "not_reported":
		return score == nil
	}
	return false
}

var versionPattern = regexp.MustCompile(`^[A-Za-z0-9.+-]{1,64}$`)
var technicalStampPattern = regexp.MustCompile(`^\d{4}-\d\d-\d\dT(?:[01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d{1,9})?(?:Z|[+-](?:[01]\d|2[0-3]):[0-5]\d)$`)

func validTechnicalStamp(value string) bool {
	return len(value) <= 40 && technicalStampPattern.MatchString(value) && validStamp(value)
}

func validVersion(value string) bool { return versionPattern.MatchString(value) }
func validTechnicalEvidence(e Evidence) bool {
	return validCheckID(e.Key) && validStrategy(e.Strategy) && validCheck(e.CheckStatus, e.CheckScore) && actionable(e.CheckStatus) && validTechnicalStamp(e.ObservedAt) && validVersion(e.LighthouseVersion) && e.OpportunityType == "" && e.Period.StartDate == "" && e.Period.EndDate == "" && e.Clicks == 0 && e.Impressions == 0 && e.CTR == 0 && e.Position == 0
}
func actionable(status string) bool {
	return status == "failed" || status == "manual" || status == "error"
}
func newer(stamp, previous string) bool {
	a, err := time.Parse(time.RFC3339Nano, stamp)
	b, err2 := time.Parse(time.RFC3339Nano, previous)
	return err == nil && err2 == nil && a.After(b)
}
func validVerification(v *Verification, e Evidence) bool {
	if e.Kind == "search_group_review" {
		return v != nil && v.Review != nil && e.Review != nil && v.Audit == nil && v.Score == nil && v.LighthouseVersion == "" && v.Review.RuleID == e.Review.Rule.ID && v.Review.Key == e.Key && validTechnicalStamp(v.RecordedAt) && validTechnicalStamp(v.ObservedAt) && newer(v.ObservedAt, e.ObservedAt) && !newer(v.ObservedAt, v.RecordedAt) && ((!v.Review.Present && !v.Review.Matches && v.Status == "inconclusive") || (v.Review.Present && ((v.Review.Matches && v.Status == "review") || (!v.Review.Matches && v.Status == "passed"))))
	}
	if v != nil && v.Review != nil {
		return false
	}
	if e.Kind == "site_audit_finding" {
		return validAuditVerification(v, e)
	}
	return v != nil && v.Audit == nil && e.Kind == "pagespeed_check" && validCheck(v.Status, v.Score) && validVersion(v.LighthouseVersion) && validTechnicalStamp(v.RecordedAt) && validTechnicalStamp(v.ObservedAt) && !newer(v.ObservedAt, v.RecordedAt) && newer(v.ObservedAt, e.ObservedAt)
}
func cloneScore(value *float64) *float64 {
	if value == nil {
		return nil
	}
	copy := *value
	return &copy
}
func cloneVerification(v *Verification) *Verification {
	if v == nil {
		return nil
	}
	copy := *v
	if v.Review != nil {
		r := *v.Review
		copy.Review = &r
	}
	copy.Score = cloneScore(v.Score)
	if v.Audit != nil {
		a := *v.Audit
		copy.Audit = &a
	}
	return &copy
}
func cloneTask(t Task) Task {
	if t.Planning != nil {
		p := *t.Planning
		t.Planning = &p
	}
	if t.Evidence.Review != nil {
		r := *t.Evidence.Review
		t.Evidence.Review = &r
	}
	t.Evidence.CheckScore = cloneScore(t.Evidence.CheckScore)
	t.Evidence.Audit = cloneAuditEvidence(t.Evidence.Audit)
	t.Verification = cloneVerification(t.Verification)
	return t
}
func cloneActivity(e Activity) Activity {
	if e.Planning != nil {
		p := *e.Planning
		e.Planning = &p
	}
	e.Verification = cloneVerification(e.Verification)
	return e
}
func findingKey(websiteID, site string, e Evidence) string {
	variant := e.OpportunityType
	if e.Kind == "search_group_review" && e.Review != nil {
		raw, _ := json.Marshal(e.Review.Rule)
		variant = string(raw)
	}
	if e.Kind == "pagespeed_check" {
		variant = e.Strategy
	}
	if e.Kind == "site_audit_finding" && e.Audit != nil {
		variant = normalizedAuditPage(site, e.Audit.PageURL)
	}
	return websiteID + "\x00" + site + "\x00" + e.Kind + "\x00" + variant + "\x00" + e.Key
}
