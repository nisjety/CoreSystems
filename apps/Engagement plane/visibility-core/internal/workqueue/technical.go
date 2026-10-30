package workqueue

import (
	"context"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type pageSpeedSource interface {
	GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection
}

// completedCheck reads cached, completed evidence only. It never starts provider work.
func (s *Service) completedCheck(ctx context.Context, site, strategy, id string) (pulse.PageSpeedCheck, *pulse.PageSpeedReport, error) {
	source, ok := s.source.(pageSpeedSource)
	if !ok {
		return pulse.PageSpeedCheck{}, nil, ErrSourceUnavailable
	}
	value := source.GetPageSpeed(ctx, site, strategy)
	if value.Status != "complete" || value.Site != site || value.Source != "google_pagespeed" || value.Strategy != strategy || value.Reason != "" || value.Report == nil || !validTechnicalStamp(value.Report.FetchTime) || !validVersion(value.Report.LighthouseVersion) {
		return pulse.PageSpeedCheck{}, nil, ErrSourceUnavailable
	}
	if newer(value.Report.FetchTime, s.now().UTC().Format(time.RFC3339Nano)) {
		return pulse.PageSpeedCheck{}, nil, ErrSourceUnavailable
	}
	seen := map[string]bool{}
	var selected pulse.PageSpeedCheck
	for _, check := range value.Report.Checks {
		if !validCheckID(check.ID) || seen[check.ID] || !validCheck(check.Status, check.Score) {
			return pulse.PageSpeedCheck{}, nil, ErrSourceUnavailable
		}
		seen[check.ID] = true
		if check.ID == id {
			selected = check
			selected.Score = cloneScore(check.Score)
		}
	}
	if selected.ID == "" {
		return pulse.PageSpeedCheck{}, nil, ErrNotFound
	}
	return selected, value.Report, nil
}
func (s *Service) SaveTechnical(ctx context.Context, id, site string, websiteRevision int64, strategy, checkID, fetchTime string) (Snapshot, bool, error) {
	if websiteRevision < 1 || !validStrategy(strategy) || !validCheckID(checkID) || !validTechnicalStamp(fetchTime) {
		return Snapshot{}, false, ErrInvalid
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, false, err
	}
	check, report, err := s.completedCheck(ctx, site, strategy, checkID)
	if err != nil {
		return Snapshot{}, false, err
	}
	if fetchTime != report.FetchTime {
		return Snapshot{}, false, ErrConflict
	}
	if !actionable(check.Status) {
		return Snapshot{}, false, ErrInvalid
	}
	evidence := Evidence{Kind: "pagespeed_check", Key: checkID, ObservedAt: report.FetchTime, Strategy: strategy, CheckStatus: check.Status, CheckScore: cloneScore(check.Score), LighthouseVersion: report.LighthouseVersion}
	if !validEvidence(evidence) {
		return Snapshot{}, false, ErrSourceUnavailable
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, false, err
	}
	created, err := s.repo.create(id, site, evidence, s.now().UTC().Format(time.RFC3339Nano))
	if err != nil {
		return Snapshot{}, false, err
	}
	result, err := s.List(id, site)
	return result, created, err
}
func (s *Service) Verify(ctx context.Context, id, site string, websiteRevision int64, taskID string, revision int64) (Snapshot, error) {
	if websiteRevision < 1 || revision < 1 || !taskPattern.MatchString(taskID) {
		return Snapshot{}, ErrInvalid
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, err
	}
	task, err := s.repo.task(id, site, taskID, revision)
	if err != nil {
		return Snapshot{}, err
	}
	if task.Evidence.Kind == "search_group_review" {
		return s.verifyReview(ctx, id, site, websiteRevision, task, revision)
	}
	if task.Evidence.Kind == "site_audit_finding" {
		return s.verifyAudit(id, site, websiteRevision, task, revision)
	}
	if task.Evidence.Kind != "pagespeed_check" {
		return Snapshot{}, ErrInvalid
	}
	check, report, err := s.completedCheck(ctx, site, task.Evidence.Strategy, task.Evidence.Key)
	if err != nil {
		return Snapshot{}, err
	}
	verification := Verification{Status: check.Status, Score: cloneScore(check.Score), ObservedAt: report.FetchTime, LighthouseVersion: report.LighthouseVersion, RecordedAt: s.now().UTC().Format(time.RFC3339Nano)}
	if !validVerification(&verification, task.Evidence) || (task.Verification != nil && !newer(verification.ObservedAt, task.Verification.ObservedAt)) {
		return Snapshot{}, ErrConflict
	}
	if _, err := s.binding(id, site, websiteRevision); err != nil {
		return Snapshot{}, err
	}
	if err := s.repo.verify(id, site, taskID, revision, verification); err != nil {
		return Snapshot{}, err
	}
	return s.List(id, site)
}
func (r *Repository) task(websiteID, site, id string, revision int64) (Task, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, task := range r.state.Tasks {
		if task.ID == id && task.WebsiteID == websiteID && task.Site == site {
			if task.Revision != revision {
				return Task{}, ErrConflict
			}
			return cloneTask(task), nil
		}
	}
	return Task{}, ErrNotFound
}
func (r *Repository) verify(websiteID, site, id string, revision int64, value Verification) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	next := cloneState(r.state)
	for i, task := range next.Tasks {
		if task.ID != id || task.WebsiteID != websiteID || task.Site != site {
			continue
		}
		if task.Revision != revision {
			return ErrConflict
		}
		if !validVerification(&value, task.Evidence) || (task.Verification != nil && !newer(value.ObservedAt, task.Verification.ObservedAt)) {
			return ErrConflict
		}
		task.Verification = cloneVerification(&value)
		task.Revision++
		task.UpdatedAt = value.RecordedAt
		next.Tasks[i] = task
		next.Activity = appendActivity(next.Activity, Activity{ID: newID("evt_"), TaskID: id, Kind: "verified", Status: task.Status, OccurredAt: value.RecordedAt, Verification: cloneVerification(&value)})
		return r.commit(next)
	}
	return ErrNotFound
}
