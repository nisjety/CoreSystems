package siteaudit

import (
	"reflect"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

func safeText(value string, max int) bool {
	return len(value) <= max && utf8.ValidString(value) && strings.IndexFunc(value, unicode.IsControl) < 0
}
func validCapture(site, raw string, p CapturePage, now time.Time) bool {
	observed, err := time.Parse(time.RFC3339Nano, p.ObservedAt)
	return p.URL == raw && discovery.SafeURL(site, p.URL) && discovery.SafeURL(site, p.FinalURL) && p.HTTPStatus >= 200 && p.HTTPStatus <= 599 &&
		safeText(p.Title, 512) && safeText(p.Description, 2048) && safeText(p.ContentType, 128) && runPattern.MatchString(p.RunID) && fingerprintPattern.MatchString(p.Fingerprint) &&
		validStamp(p.ObservedAt) && err == nil && !observed.After(now) && len(p.Links) <= maxLinks && validSEO(site, p.SEO)
}
func validReason(value string) bool {
	switch value {
	case "", "not_configured", "source_unavailable", "policy_blocked", "access_denied", "quota_limited", "invalid_source", "timeout", "unsupported_content", "interrupted", "context_changed", "storage_unavailable":
		return true
	}
	return false
}
func validateSnapshot(v Snapshot) error {
	if !auditPattern.MatchString(v.ID) || !websitePattern.MatchString(v.WebsiteID) || !discovery.SafeURL(v.Site, v.Site) || v.WebsiteRevision < 1 || v.Source != "quarry_capture" || v.RuleVersion != RuleVersion || (v.Limit != Limit && v.Limit != 10) || !v.Partial || !validStamp(v.StartedAt) || v.Pages == nil || v.Findings == nil || v.History == nil || len(v.History) != 0 || len(v.Pages) > v.Limit || len(v.Findings) > v.Limit*len(rules) || !validReason(v.Reason) {
		return ErrInvalid
	}
	if v.Coverage != nil {
		c := v.Coverage
		if c.Attempted != len(v.Pages) || c.Discovered < c.Attempted || c.Discovered > 1000 || c.Checked < 0 || c.Failed < 0 || c.Checked+c.Failed != c.Attempted || c.Pending != c.Discovered-c.Attempted || c.LimitReached != (c.Attempted >= v.Limit && c.Pending > 0) || !validReason(c.StopReason) {
			return ErrInvalid
		}
		checked := 0
		partial := false
		for _, p := range v.Pages {
			if p.Status == "checked" {
				checked++
			}
			partial = partial || p.ExtractionPartial
		}
		if checked != c.Checked || partial != c.ExtractionPartial {
			return ErrInvalid
		}
	}
	switch v.Status {
	case "running":
		if v.CompletedAt != "" || len(v.Pages) != 0 || len(v.Findings) != 0 || v.Reason != "" || v.Coverage != nil {
			return ErrInvalid
		}
	case "complete", "failed":
		if v.Status == "complete" && (v.Reason != "" || len(v.Pages) == 0) || v.Status == "failed" && v.Reason == "" {
			return ErrInvalid
		}
		if !validStamp(v.CompletedAt) {
			return ErrInvalid
		}
		a, _ := time.Parse(time.RFC3339Nano, v.StartedAt)
		b, _ := time.Parse(time.RFC3339Nano, v.CompletedAt)
		if b.Before(a) {
			return ErrInvalid
		}
	default:
		return ErrInvalid
	}
	seen := map[string]bool{}
	for _, p := range v.Pages {
		if !discovery.SafeURL(v.Site, p.URL) || seen[p.URL] || p.Links == nil || p.Checks == nil || len(p.Links) > maxLinks || len(p.Checks) > len(rules) || !validReason(p.Reason) {
			return ErrInvalid
		}
		seen[p.URL] = true
		if p.Status == "failed" {
			if p.HTTPStatus != 0 || p.FinalURL != "" || p.Title != "" || p.Description != "" || p.Canonical != "" || p.RunID != "" || p.Fingerprint != "" || p.ObservedAt != "" || len(p.Checks) != 0 || len(p.Links) != 0 || p.Reason == "" || p.SEO != nil {
				return ErrInvalid
			}
			continue
		}
		if p.Status != "checked" || p.Reason != "" || !validCapture(v.Site, p.URL, CapturePage{URL: p.URL, FinalURL: p.FinalURL, HTTPStatus: p.HTTPStatus, Title: p.Title, Description: p.Description, ObservedAt: p.ObservedAt, RunID: p.RunID, Fingerprint: p.Fingerprint, Links: p.Links, SEO: p.SEO}, mustTime(v.CompletedAt)) {
			return ErrInvalid
		}
		if mustTime(p.ObservedAt).Before(mustTime(v.StartedAt)) {
			return ErrInvalid
		}
		if p.Canonical != "" && !discovery.SafeURL(v.Site, p.Canonical) {
			return ErrInvalid
		}
		for _, link := range p.Links {
			if !discovery.SafeURL(v.Site, link) {
				return ErrInvalid
			}
		}
	}
	pages, findings := evaluate(v.Pages)
	if !reflect.DeepEqual(pages, v.Pages) || !reflect.DeepEqual(findings, v.Findings) {
		return ErrInvalid
	}
	return nil
}
func mustTime(raw string) time.Time { v, _ := time.Parse(time.RFC3339Nano, raw); return v }
