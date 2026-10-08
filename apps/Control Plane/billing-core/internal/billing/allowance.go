package billing

import (
	"errors"
	"fmt"
	"sort"
	"time"
)

var ErrAllowanceInvalid = errors.New("invalid allowance operation")
var ErrAllowanceConflict = errors.New("allowance operation conflicts with recorded history")
var ErrAllowanceCapacity = errors.New("allowance history requires archival before further calculation")

const maxAllowanceUnits int64 = 1_000_000_000_000

// Policies are immutable after activation. Changes require an explicit migration
// with settlement, rather than retroactively repricing or regranting history.
type AllowancePolicy struct {
	Pauses         []AllowancePause    `json:"-"`
	Revisions      []AllowanceRevision `json:"-"`
	Metric         string              `json:"metric"`
	Name           string              `json:"name"`
	Unit           string              `json:"unit"`
	Version        string              `json:"version"`
	StartsAt       time.Time           `json:"starts_at"`
	EndsAt         *time.Time          `json:"ends_at,omitempty"`
	IntervalMonths int                 `json:"interval_months"`
	IncludedUnits  int64               `json:"included_units"`
	RolloverCap    int64               `json:"rollover_cap"`
	// Zero disables carry-over; -1 means no expiry; positive values are extra cycles.
	RolloverCycles int `json:"rollover_cycles"`
}

// Changes take effect only at the original renewal boundary. Prior grants keep
// their original expiry; the new carry cap applies at the next renewal.
type AllowanceRevision struct {
	Version        string    `json:"version"`
	EffectiveAt    time.Time `json:"effective_at"`
	IncludedUnits  int64     `json:"included_units"`
	RolloverCap    int64     `json:"rollover_cap"`
	RolloverCycles int       `json:"rollover_cycles"`
	Reason         string    `json:"reason"`
}

func (v AllowanceRevision) validate(p AllowancePolicy) error {
	if v.Version == "" || len(v.Version) > 64 || v.Version == p.Version || v.Reason == "" || len(v.Reason) > 240 || v.IncludedUnits < 0 || v.IncludedUnits > maxAllowanceUnits || v.RolloverCap < 0 || v.RolloverCap > maxAllowanceUnits || v.RolloverCycles < -1 || v.RolloverCycles > 120 {
		return ErrAllowanceInvalid
	}
	if p.EndsAt != nil && !v.EffectiveAt.Before(*p.EndsAt) {
		return ErrAllowanceInvalid
	}
	for cycle := 1; cycle < 1200; cycle++ {
		boundary := allowanceBoundary(p, cycle)
		if boundary.Equal(v.EffectiveAt) {
			return nil
		}
		if boundary.After(v.EffectiveAt) {
			break
		}
	}
	return fmt.Errorf("%w: changes must start at a renewal boundary", ErrAllowanceInvalid)
}

type AllowancePause struct {
	From time.Time
	To   *time.Time
}

func (p AllowancePolicy) paused(at time.Time) bool {
	for _, pause := range p.Pauses {
		if !at.Before(pause.From) && (pause.To == nil || at.Before(*pause.To)) {
			return true
		}
	}
	return false
}

type AllowanceEntry struct {
	ID         string     `json:"id"`
	Kind       string     `json:"kind"`
	Units      int64      `json:"units"`
	OccurredAt time.Time  `json:"occurred_at"`
	ExpiresAt  *time.Time `json:"expires_at,omitempty"`
	Reason     string     `json:"reason"`
}

type AllowancePeriod struct {
	PolicyVersion string    `json:"policy_version"`
	From          time.Time `json:"from"`
	To            time.Time `json:"to"`
	Included      int64     `json:"included"`
	Carried       int64     `json:"carried"`
	Granted       int64     `json:"granted"`
	Used          int64     `json:"used"`
	Overage       int64     `json:"overage"`
	Expired       int64     `json:"expired"`
	Remaining     int64     `json:"remaining"`
}

type AllowanceReport struct {
	Policy     AllowancePolicy    `json:"policy"`
	Current    AllowancePeriod    `json:"current"`
	History    []AllowancePeriod  `json:"history"`
	AsOf       time.Time          `json:"as_of"`
	NextExpiry *time.Time         `json:"next_expiry,omitempty"`
	Active     bool               `json:"active"`
	NextChange *AllowanceRevision `json:"next_change,omitempty"`
}

func (p AllowancePolicy) Validate() error {
	if validateUsageDimension("metric", p.Metric, 128, false) != nil || p.Name == "" || len(p.Name) > 120 || p.Version == "" || len(p.Version) > 64 || p.StartsAt.IsZero() || p.StartsAt.Year() < 2000 || p.StartsAt.Year() > 2200 {
		return fmt.Errorf("%w: metric, name, version and valid starts_at required", ErrAllowanceInvalid)
	}
	if p.Unit != "tokens" && p.Unit != "requests" && p.Unit != "units" {
		return fmt.Errorf("%w: unsupported unit", ErrAllowanceInvalid)
	}
	if p.IntervalMonths < 1 || p.IntervalMonths > 12 || p.IncludedUnits < 0 || p.IncludedUnits > maxAllowanceUnits || p.RolloverCap < 0 || p.RolloverCap > maxAllowanceUnits || p.RolloverCycles < -1 || p.RolloverCycles > 120 {
		return fmt.Errorf("%w: invalid allowance or renewal policy", ErrAllowanceInvalid)
	}
	if p.EndsAt != nil && !p.EndsAt.After(p.StartsAt) {
		return fmt.Errorf("%w: ends_at must follow starts_at", ErrAllowanceInvalid)
	}
	return nil
}

func (e AllowanceEntry) Validate() error {
	if ValidateUsageEventID(e.ID) != nil || (e.Kind != "usage" && e.Kind != "grant") || e.Units <= 0 || e.Units > maxAllowanceUnits || e.OccurredAt.IsZero() || len(e.Reason) > 240 {
		return fmt.Errorf("%w: invalid entry", ErrAllowanceInvalid)
	}
	if e.ExpiresAt != nil && (!e.ExpiresAt.After(e.OccurredAt) || e.Kind != "grant") {
		return fmt.Errorf("%w: invalid expiry", ErrAllowanceInvalid)
	}
	return nil
}

// Always calculate from the original anchor; Jan 31 -> Feb 28 -> Mar 31.
func allowanceBoundary(p AllowancePolicy, cycle int) time.Time {
	a := p.StartsAt.UTC()
	month := time.Date(a.Year(), a.Month()+time.Month(cycle*p.IntervalMonths), 1, a.Hour(), a.Minute(), a.Second(), a.Nanosecond(), time.UTC)
	last := month.AddDate(0, 1, -1).Day()
	day := a.Day()
	if day > last {
		day = last
	}
	return month.AddDate(0, 0, day-1)
}

type allowanceLot struct {
	units     int64
	expiry    *time.Time
	recurring bool
}

// Replaying event time makes late deliveries deterministic. This is a usage
// projection, never an invoice or payment instruction. Closed periods may be
// corrected by late events; the original source entries stay immutable.
func CalculateAllowance(p AllowancePolicy, entries []AllowanceEntry, now time.Time) (AllowanceReport, error) {
	r := AllowanceReport{Policy: p, AsOf: now.UTC(), History: []AllowancePeriod{}}
	if err := p.Validate(); err != nil {
		return r, err
	}
	if len(entries) > 100000 {
		return r, ErrAllowanceCapacity
	}
	revisions := append([]AllowanceRevision(nil), p.Revisions...)
	sort.Slice(revisions, func(i, j int) bool { return revisions[i].EffectiveAt.Before(revisions[j].EffectiveAt) })
	versions := map[string]bool{p.Version: true}
	for i, v := range revisions {
		if err := v.validate(p); err != nil {
			return r, err
		}
		if versions[v.Version] || (i > 0 && revisions[i-1].EffectiveAt.Equal(v.EffectiveAt)) {
			return r, ErrAllowanceConflict
		}
		versions[v.Version] = true
		if v.EffectiveAt.After(now) && r.NextChange == nil {
			next := v
			r.NextChange = &next
		}
	}
	events := append([]AllowanceEntry(nil), entries...)
	seen := map[string]bool{}
	for _, e := range events {
		if err := e.Validate(); err != nil {
			return r, err
		}
		if seen[e.ID] {
			return r, ErrAllowanceConflict
		}
		seen[e.ID] = true
	}
	sort.Slice(events, func(i, j int) bool {
		a, b := events[i], events[j]
		if !a.OccurredAt.Equal(b.OccurredAt) {
			return a.OccurredAt.Before(b.OccurredAt)
		}
		if a.Kind != b.Kind {
			return a.Kind == "grant"
		}
		return a.ID < b.ID
	})
	if now.Before(p.StartsAt) {
		return r, nil
	}
	stop := now
	if p.EndsAt != nil && p.EndsAt.Before(stop) {
		stop = *p.EndsAt
	}
	lots := []allowanceLot{}
	idx := 0
	for idx < len(events) && events[idx].OccurredAt.Before(p.StartsAt) {
		idx++
	}
	total := func() int64 {
		var n int64
		for _, l := range lots {
			n += l.units
		}
		return n
	}
	expire := func(at time.Time) int64 {
		var n int64
		for i := range lots {
			if lots[i].expiry != nil && !lots[i].expiry.After(at) {
				n += lots[i].units
				lots[i].units = 0
			}
		}
		return n
	}
	sortLots := func() {
		sort.SliceStable(lots, func(i, j int) bool {
			if lots[i].expiry == nil {
				return false
			}
			if lots[j].expiry == nil {
				return true
			}
			return lots[i].expiry.Before(*lots[j].expiry)
		})
	}
	for cycle := 0; ; cycle++ {
		if cycle >= 1200 {
			return r, ErrAllowanceCapacity
		}
		start, end := allowanceBoundary(p, cycle), allowanceBoundary(p, cycle+1)
		if p.EndsAt != nil && !start.Before(*p.EndsAt) {
			break
		}
		cyclePolicy := p
		for _, v := range revisions {
			if v.EffectiveAt.After(start) {
				break
			}
			cyclePolicy.Version = v.Version
			cyclePolicy.IncludedUnits = v.IncludedUnits
			cyclePolicy.RolloverCap = v.RolloverCap
			cyclePolicy.RolloverCycles = v.RolloverCycles
		}
		included := cyclePolicy.IncludedUnits
		if p.paused(start) {
			included = 0
		}
		period := AllowancePeriod{From: start, To: end, Included: included, PolicyVersion: cyclePolicy.Version}
		if p.EndsAt != nil && p.EndsAt.Before(period.To) {
			period.To = *p.EndsAt
		}
		period.Expired += expire(start)
		if cycle > 0 {
			cap := cyclePolicy.RolloverCap
			if cyclePolicy.RolloverCycles == 0 {
				cap = 0
			}
			sortLots()
			for i := range lots {
				l := &lots[i]
				if !l.recurring {
					continue
				}
				keep := l.units
				if keep > cap {
					keep = cap
				}
				period.Expired += l.units - keep
				l.units = keep
				cap -= keep
				period.Carried += keep
			}
		}
		var expiry *time.Time
		if cyclePolicy.RolloverCycles >= 0 {
			e := allowanceBoundary(p, cycle+1+cyclePolicy.RolloverCycles)
			expiry = &e
		}
		lots = append(lots, allowanceLot{units: included, expiry: expiry, recurring: true})
		cutoff := end
		if stop.Before(cutoff) {
			cutoff = stop
		}
		for idx < len(events) && events[idx].OccurredAt.Before(end) && !events[idx].OccurredAt.After(cutoff) {
			e := events[idx]
			idx++
			if p.EndsAt != nil && !e.OccurredAt.Before(*p.EndsAt) {
				continue
			}
			period.Expired += expire(e.OccurredAt)
			if e.Kind == "grant" {
				lots = append(lots, allowanceLot{units: e.Units, expiry: e.ExpiresAt})
				period.Granted += e.Units
				continue
			}
			period.Used += e.Units
			remaining := e.Units
			sortLots()
			for i := range lots {
				used := lots[i].units
				if used > remaining {
					used = remaining
				}
				lots[i].units -= used
				remaining -= used
				if remaining == 0 {
					break
				}
			}
			period.Overage += remaining
		}
		period.Expired += expire(cutoff)
		period.Remaining = total()
		// Remove exhausted lots to keep replay bounded by live credit lots.
		kept := lots[:0]
		for _, l := range lots {
			if l.units > 0 {
				kept = append(kept, l)
			}
		}
		lots = kept
		r.Current = period
		r.Policy = cyclePolicy
		if !period.To.After(stop) {
			r.History = append(r.History, period)
		}
		if end.After(stop) || (p.EndsAt != nil && !end.Before(*p.EndsAt)) {
			break
		}
	}
	r.Active = (p.EndsAt == nil || now.Before(*p.EndsAt)) && !p.paused(now)
	for _, l := range lots {
		if l.units > 0 && l.expiry != nil && (r.NextExpiry == nil || l.expiry.Before(*r.NextExpiry)) {
			e := *l.expiry
			r.NextExpiry = &e
		}
	}
	return r, nil
}
