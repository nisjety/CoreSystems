package sitesources

import (
	"context"
	"encoding/json"
	"net/url"
	"strings"
	"time"
)

func (s *Service) cookiebot(ctx context.Context, b cookieBinding, key string, window Window, stillBound func() bool) *Cookiebot {
	result, missing := s.cookiebotDomain(ctx, b, key, window, stillBound)
	if !missing || ctx.Err() != nil {
		return result
	}
	if strings.HasPrefix(b.Domain, "www.") {
		b.Domain = strings.TrimPrefix(b.Domain, "www.")
	} else {
		b.Domain = "www." + b.Domain
	}
	if !strings.Contains(b.Domain, ".") {
		return result
	}
	next, _ := s.cookiebotDomain(ctx, b, key, window, stillBound)
	return next
}
func (s *Service) cookiebotDomain(ctx context.Context, b cookieBinding, key string, window Window, stillBound func() bool) (*Cookiebot, bool) {
	base := "https://consent.cookiebot.com/api/v1/" + key + "/json/domaingroup/" + b.DomainGroupID + "/"
	query := url.Values{"startdate": {strings.ReplaceAll(window.StartDate, "-", "")}, "enddate": {strings.ReplaceAll(window.EndDate, "-", "")}}
	raw, status, consentCode := s.fetchResponse(ctx, base+"domain/"+b.Domain+"/consent/stats?"+query.Encode(), nil, stillBound)
	consents := Consents{Status: status}
	if status == "" {
		consents = parseConsents(raw, b, window)
	}
	raw, status, inventoryCode := s.fetchResponse(ctx, base+b.Culture+"/domain/"+b.Domain+"/cookies", nil, stillBound)
	inventory := Inventory{Status: status}
	if status == "" {
		inventory = parseInventory(raw, b, s.now().UTC())
	}
	return &Cookiebot{ReportingDomain: b.Domain, Consents: consents, Inventory: inventory}, consentCode == 404 && inventoryCode == 404
}
func parseConsents(raw []byte, b cookieBinding, window Window) Consents {
	failed := Consents{Status: "unavailable"}
	root, ok := object(raw)
	if !ok {
		return failed
	}
	domain, dok := text(root, "domain")
	path, pok := text(root, "urlpath")
	start, sok := text(root, "utcstartdate")
	end, eok := text(root, "utcenddate")
	if !dok || domain != b.Domain || !pok || path != "/" || !sok || !eok || !boundary(start, window.StartDate, false) || !boundary(end, window.EndDate, true) {
		return failed
	}
	stats, ok := object(root["consentstat"])
	if !ok {
		return failed
	}
	daysRaw, exists := stats["consentday"]
	if !exists || string(daysRaw) == "null" {
		return failed
	}
	var days []json.RawMessage
	if len(daysRaw) > 0 && daysRaw[0] == '[' {
		if json.Unmarshal(daysRaw, &days) != nil || days == nil {
			return failed
		}
	} else {
		if _, ok := object(daysRaw); !ok {
			return failed
		}
		days = []json.RawMessage{daysRaw}
	}
	if len(days) > 7 {
		return failed
	}
	if len(days) == 0 {
		return Consents{Status: "no_data", ReportedDays: 0}
	}
	seen := map[string]bool{}
	totals := Totals{}
	for _, day := range days {
		fields, ok := object(day)
		if !ok {
			return failed
		}
		date, ok := text(fields, "Date")
		if !ok {
			return failed
		}
		date, ok = consentDay(date, window)
		if !ok || seen[date] {
			return failed
		}
		seen[date] = true
		values := []*int64{&totals.OptIn, &totals.OptOut, &totals.Preferences, &totals.Statistics, &totals.Marketing}
		names := []string{"OptIn", "OptOut", "TypeOptInPref", "TypeOptInStat", "TypeOptInMark"}
		for i, name := range names {
			n, ok := count(fields[name])
			if !ok {
				return failed
			}
			sum, ok := add(*values[i], n)
			if !ok {
				return failed
			}
			*values[i] = sum
		}
	}
	return Consents{Status: "connected", ReportedDays: len(days), Totals: &totals}
}
func parseInventory(raw []byte, b cookieBinding, now time.Time) Inventory {
	failed := Inventory{Status: "unavailable"}
	root, ok := object(raw)
	if !ok {
		return failed
	}
	domain, dok := text(root, "domain")
	path, pok := text(root, "urlpath")
	scan, sok := text(root, "utcscandate")
	culture, cok := text(root, "culture")
	stamp, err := time.Parse(time.RFC3339Nano, scan)
	if !dok || domain != b.Domain || !pok || path != "/" || !sok || err != nil || !strings.HasSuffix(scan, "Z") || stamp.After(now) || stamp.Year() < 2000 || !cok || !culturePattern.MatchString(culture) {
		return failed
	}
	var cookies []json.RawMessage
	if json.Unmarshal(root["cookies"], &cookies) != nil || cookies == nil || len(cookies) > 5000 {
		return failed
	}
	total, third, secure, httpOnly := int64(len(cookies)), int64(0), int64(0), int64(0)
	categories := Categories{}
	for _, cookie := range cookies {
		fields, ok := object(cookie)
		if !ok {
			return failed
		}
		category, ok := count(fields["Category"])
		if !ok || category < 1 || category > 5 {
			return failed
		}
		for _, field := range []struct {
			name   string
			target *int64
		}{{"ThirdParty", &third}, {"Secure", &secure}, {"HTTPOnly", &httpOnly}} {
			n, ok := bit(fields[field.name])
			if !ok {
				return failed
			}
			*field.target += n
		}
		switch category {
		case 1:
			categories.Necessary++
		case 2:
			categories.Preferences++
		case 3:
			categories.Statistics++
		case 4:
			categories.Marketing++
		case 5:
			categories.Unclassified++
		}
	}
	return Inventory{Status: "connected", ScannedAt: stamp.UTC().Format(time.RFC3339Nano), Total: &total, Categories: &categories, ThirdParty: &third, Secure: &secure, HTTPOnly: &httpOnly}
}
