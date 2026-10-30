package workqueue

import (
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/searchresearch"
	"math"
	"strings"
	"time"
	"unicode"
)

type ReviewEvidence struct {
	Rule searchresearch.Rule `json:"rule"`
}
type ReviewVerification struct {
	RuleID  string `json:"ruleId"`
	Key     string `json:"key"`
	Present bool   `json:"present"`
	Matches bool   `json:"matches"`
}
type reviewWire struct {
	Kind       string `json:"kind"`
	Key        string `json:"key"`
	ObservedAt string `json:"observedAt"`
	Period     struct {
		StartDate string `json:"startDate"`
		EndDate   string `json:"endDate"`
	} `json:"period"`
	Clicks      int64           `json:"clicks"`
	Impressions int64           `json:"impressions"`
	CTR         float64         `json:"ctr"`
	Position    float64         `json:"position"`
	Review      *ReviewEvidence `json:"review"`
}
type ResearchSource interface {
	Get(context.Context, string, string) (searchresearch.Snapshot, error)
}

func (s *Service) WithResearch(source ResearchSource) *Service {
	next := *s
	next.research = source
	return &next
}
func matchesRule(r searchresearch.Rule, a searchresearch.Aggregate) bool {
	return strings.Contains(strings.ToLower(a.Key), strings.ToLower(r.Contains)) && a.Impressions >= r.MinImpressions && (r.MaxPosition == 0 || a.Position <= r.MaxPosition)
}
func validReviewEvidence(e Evidence) bool {
	if e.Review == nil || e.OpportunityType != "" || e.Audit != nil || e.Strategy != "" || e.CheckStatus != "" || e.CheckScore != nil || e.LighthouseVersion != "" || !validTechnicalStamp(e.ObservedAt) || len(e.Key) == 0 || len(e.Key) > 2048 || strings.ContainsFunc(e.Key, unicode.IsControl) || e.Clicks < 0 || e.Impressions < 0 || e.Clicks > e.Impressions || e.Impressions > 1<<45 || math.IsNaN(e.Position) || math.IsInf(e.Position, 0) || e.Position < 0 || e.Position > 1e6 || math.IsNaN(e.CTR) || e.CTR < 0 || e.CTR > 1 {
		return false
	}
	r := e.Review.Rule
	ctr := 0.0
	if e.Impressions > 0 {
		ctr = float64(e.Clicks) / float64(e.Impressions)
	}
	start, a := time.Parse("2006-01-02", e.Period.StartDate)
	end, b := time.Parse("2006-01-02", e.Period.EndDate)
	return a == nil && b == nil && !end.Before(start) && end.Sub(start) <= 90*24*time.Hour && searchresearch.ValidRule(r) && matchesRule(r, searchresearch.Aggregate{Key: e.Key, Impressions: e.Impressions, Position: e.Position}) && math.Abs(ctr-e.CTR) < .000001 && len(e.Period.StartDate) == 10 && e.Period.StartDate <= e.Period.EndDate
}
func marshalReview(e Evidence) ([]byte, error) {
	v := reviewWire{Kind: e.Kind, Key: e.Key, ObservedAt: e.ObservedAt, Clicks: e.Clicks, Impressions: e.Impressions, CTR: e.CTR, Position: e.Position, Review: e.Review}
	v.Period.StartDate = e.Period.StartDate
	v.Period.EndDate = e.Period.EndDate
	return json.Marshal(v)
}
func unmarshalReview(raw []byte, e *Evidence) error {
	var v reviewWire
	if err := decodeStrict(raw, &v); err != nil {
		return err
	}
	*e = Evidence{Kind: v.Kind, Key: v.Key, ObservedAt: v.ObservedAt, Clicks: v.Clicks, Impressions: v.Impressions, CTR: v.CTR, Position: v.Position, Review: v.Review}
	e.Period.StartDate = v.Period.StartDate
	e.Period.EndDate = v.Period.EndDate
	return nil
}
func (s *Service) SaveReview(ctx context.Context, id, site string, rev int64, ruleID, key string) (Snapshot, bool, error) {
	if rev < 1 || len(key) == 0 || len(key) > 2048 {
		return Snapshot{}, false, ErrInvalid
	}
	if _, err := s.binding(id, site, rev); err != nil {
		return Snapshot{}, false, err
	}
	if s.research == nil {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	report, err := s.research.Get(ctx, id, site)
	if err != nil || report.WebsiteRevision != rev || report.Status != "connected" || report.Stale {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	var e Evidence
	for _, g := range report.Groups {
		if g.Rule.ID != ruleID {
			continue
		}
		for _, a := range g.Items {
			if a.Key == key {
				ctr := 0.0
				if a.Impressions > 0 {
					ctr = float64(a.Clicks) / float64(a.Impressions)
				}
				e = Evidence{Kind: "search_group_review", Key: key, ObservedAt: report.ObservedAt, Period: report.Period, Clicks: a.Clicks, Impressions: a.Impressions, CTR: ctr, Position: a.Position, Review: &ReviewEvidence{Rule: g.Rule}}
			}
		}
	}
	if !validReviewEvidence(e) {
		return Snapshot{}, false, ErrNotFound
	}
	if _, err = s.binding(id, site, rev); err != nil {
		return Snapshot{}, false, err
	}
	created, err := s.repo.create(id, site, e, s.now().UTC().Format("2006-01-02T15:04:05.999999999Z07:00"))
	if err != nil {
		return Snapshot{}, false, err
	}
	v, err := s.List(id, site)
	return v, created, err
}
func (s *Service) verifyReview(ctx context.Context, id, site string, rev int64, task Task, revision int64) (Snapshot, error) {
	if s.research == nil || task.Evidence.Review == nil {
		return Snapshot{}, ErrSourceUnavailable
	}
	r, err := s.research.Get(ctx, id, site)
	if err != nil || r.Status != "connected" || r.Stale || r.WebsiteRevision != rev {
		return Snapshot{}, ErrSourceUnavailable
	}
	rule := task.Evidence.Review.Rule
	rows := r.Queries
	if rule.Entity == "page" {
		rows = r.Pages
	}
	present, matches := false, false
	for _, a := range rows {
		if a.Key == task.Evidence.Key {
			present = true
			matches = matchesRule(rule, a)
			break
		}
	}
	status := "inconclusive"
	if present {
		status = "passed"
		if matches {
			status = "review"
		}
	}
	v := Verification{Status: status, ObservedAt: r.ObservedAt, RecordedAt: s.now().UTC().Format("2006-01-02T15:04:05.999999999Z07:00"), Review: &ReviewVerification{RuleID: rule.ID, Key: task.Evidence.Key, Present: present, Matches: matches}}
	if !validVerification(&v, task.Evidence) || task.Verification != nil && !newer(v.ObservedAt, task.Verification.ObservedAt) {
		return Snapshot{}, ErrConflict
	}
	if _, err = s.binding(id, site, rev); err != nil {
		return Snapshot{}, err
	}
	if err = s.repo.verify(id, site, task.ID, revision, v); err != nil {
		return Snapshot{}, err
	}
	return s.List(id, site)
}
