package siem

import (
	"context"
	"errors"
	"net/http"
	"time"

	"github.com/prometheus/client_golang/prometheus"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

const (
	// PollInterval is how often the runner re-reads the destination list and
	// sweeps every destination for undelivered rows.
	PollInterval = 30 * time.Second
	// BatchRows is the most chain rows one delivery carries. At the ~1.8 KiB a
	// typical row encodes to, that is about 900 KiB, under MaxBodyBytes; the
	// packer stops earlier when rows are larger.
	BatchRows = 500
	// maxBatchesPerPass keeps one backlogged destination from holding the
	// sweep: the rest of its backlog continues on the next pass.
	maxBatchesPerPass = 100
	// headReserveBytes is held back from every body for the audit_chain_head
	// record that closes a batch.
	headReserveBytes = 8 << 10
)

// Destination states reported by audit_siem_destinations.
const (
	stateActive     = "active"
	stateBackingOff = "backing_off"
	stateInvalid    = "invalid"
)

// Batch results reported by audit_siem_export_batches_total.
const (
	resultOK     = "ok"
	resultFailed = "failed" // the destination did not accept the batch
	resultError  = "error"  // the exporter could not read or record (database, encoding)
)

// The series carry destination kind, state and result only: no org, destination
// id or URL label, so a tenant's configuration never becomes a metric.
var (
	exportLag = prometheus.NewGaugeVec(prometheus.GaugeOpts{
		Name: "audit_siem_export_lag_seconds",
		Help: "Age of the oldest audit row a destination of this kind has not acknowledged (0 when caught up), worst destination of the kind.",
	}, []string{"destination_kind"})
	exportBatches = prometheus.NewCounterVec(prometheus.CounterOpts{
		Name: "audit_siem_export_batches_total",
		Help: "SIEM export batches by result (ok, failed, error).",
	}, []string{"result"})
	exportDestinations = prometheus.NewGaugeVec(prometheus.GaugeOpts{
		Name: "audit_siem_destinations",
		Help: "Enabled SIEM destinations by state (active, backing_off, invalid).",
	}, []string{"state"})
)

func init() {
	prometheus.MustRegister(exportLag, exportBatches, exportDestinations)
}

// DestinationLister is org-core's destination read.
type DestinationLister interface {
	ListDestinations(ctx context.Context, updatedSince time.Time) ([]Destination, error)
}

// Runner delivers every enabled destination's chain in order. It is
// single-threaded by design: one destination, one batch in flight at a time.
type Runner struct {
	Destinations DestinationLister
	Source       Source
	Cursors      Cursors
	Client       *http.Client
	Now          func() time.Time

	state map[string]*destState
}

// destState is what the runner remembers in memory between passes. Cursors
// are durable; this is only scheduling and the lag clock.
type destState struct {
	notBefore    time.Time
	pendingSince time.Time
}

func (r *Runner) now() time.Time {
	if r.Now != nil {
		return r.Now().UTC()
	}
	return time.Now().UTC()
}

// Run sweeps until ctx ends.
func (r *Runner) Run(ctx context.Context) {
	r.Pass(ctx)
	ticker := time.NewTicker(PollInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			r.Pass(ctx)
		}
	}
}

// Pass is one sweep: read the enabled destinations, then drain each in order
// until it is caught up, fails, or has had maxBatchesPerPass batches.
func (r *Runner) Pass(ctx context.Context) {
	if r.state == nil {
		r.state = map[string]*destState{}
	}
	destinations, err := r.Destinations.ListDestinations(ctx, time.Time{})
	if err != nil {
		exportBatches.WithLabelValues(resultError).Inc()
		log.Error().Err(err).Msg("siem exporter could not read destinations")
		return
	}

	seen := make(map[string]struct{}, len(destinations))
	counts := map[string]float64{stateActive: 0, stateBackingOff: 0, stateInvalid: 0}
	lag := map[string]float64{"ndjson_webhook": 0, "splunk_hec": 0}
	for _, d := range destinations {
		if ctx.Err() != nil {
			return
		}
		seen[d.ID] = struct{}{}
		st := r.state[d.ID]
		if st == nil {
			st = &destState{}
			r.state[d.ID] = st
		}
		counts[r.exportDestination(ctx, d, st)]++
		if !st.pendingSince.IsZero() {
			if age := r.now().Sub(st.pendingSince).Seconds(); age > lag[d.Kind] {
				lag[d.Kind] = age
			}
		}
	}
	// A destination org-core no longer lists (disabled or deleted) is dropped
	// from memory; its durable cursor stays for the audit trail and a re-enable.
	for id := range r.state {
		if _, ok := seen[id]; !ok {
			delete(r.state, id)
		}
	}
	for state, n := range counts {
		exportDestinations.WithLabelValues(state).Set(n)
	}
	for kind, seconds := range lag {
		exportLag.WithLabelValues(kind).Set(seconds)
	}
}

// exportDestination drains one destination and reports its state for the
// destinations gauge.
func (r *Runner) exportDestination(ctx context.Context, d Destination, st *destState) string {
	if r.now().Before(st.notBefore) {
		return stateBackingOff
	}
	f, ok := formatFor(d.Kind)
	if !ok {
		r.fail(ctx, d, st, "kind_invalid")
		return stateInvalid
	}
	if _, err := ValidateURL(d.URL); err != nil {
		r.fail(ctx, d, st, FailureCode(err))
		return stateInvalid
	}

	for i := 0; i < maxBatchesPerPass; i++ {
		if ctx.Err() != nil {
			return stateActive
		}
		cur, err := r.Cursors.Get(ctx, d.ID)
		if err != nil {
			r.internalError(d, "cursor_read", err)
			return stateActive
		}
		page, err := r.Source.Page(ctx, d.OrgID, cur.Seq, BatchRows)
		if err != nil {
			r.internalError(d, "chain_read", err)
			return stateActive
		}
		if len(page.Events) == 0 {
			st.pendingSince = time.Time{}
			return stateActive
		}
		st.pendingSince = page.Events[0].IngestedAt

		attestation, err := r.Source.NewestAttestation(ctx, d.OrgID)
		if err != nil {
			r.internalError(d, "attestation_read", err)
			return stateActive
		}
		now := r.now()
		b, err := assemble(d, f, cur.Seq, page, &Head{OrgID: d.OrgID, HeadSeq: page.HeadSeq, Attestation: attestation}, now)
		if err != nil {
			r.internalError(d, "encode", err)
			return stateActive
		}

		status, err := Post(ctx, r.Client, d.URL, f.header(d.Secret, b.body, now), b.body)
		switch {
		case err != nil:
			if errors.Is(err, context.Canceled) {
				return stateActive
			}
			r.fail(ctx, d, st, FailureCode(err))
			return stateBackingOff
		case status < 200 || status > 299:
			r.fail(ctx, d, st, statusFailure(status))
			return stateBackingOff
		}
		// The cursor moves only after the 2xx. A crash here redelivers this
		// batch, which the receiver de-duplicates on (org_id, chain_seq).
		if err := r.Cursors.Advance(ctx, d, b.through, b.events); err != nil {
			r.internalError(d, "cursor_write", err)
			return stateActive
		}
		exportBatches.WithLabelValues(resultOK).Inc()
	}
	return stateActive
}

// fail records a rejected or undeliverable batch and schedules the retry.
func (r *Runner) fail(ctx context.Context, d Destination, st *destState, code string) {
	exportBatches.WithLabelValues(resultFailed).Inc()
	streak, err := r.Cursors.Fail(ctx, d, code)
	if err != nil {
		r.internalError(d, "cursor_write", err)
		streak = 1
	}
	st.notBefore = r.now().Add(Backoff(streak))
	// Destination identity and a fixed code only: never the URL, a header, the
	// secret or the receiver's answer.
	log.Warn().Str("destination", d.ID).Str("org_id", d.OrgID).Str("kind", d.Kind).
		Str("code", code).Int("consecutive_failures", streak).Msg("siem export batch failed")
}

func (r *Runner) internalError(d Destination, stage string, err error) {
	exportBatches.WithLabelValues(resultError).Inc()
	log.Error().Err(err).Str("destination", d.ID).Str("org_id", d.OrgID).Str("stage", stage).Msg("siem exporter error")
}

// batch is one assembled delivery.
type batch struct {
	body    []byte
	through int64 // highest chain_seq this body accounts for, delivered or marked
	events  int   // audit_event records in the body
}

// assemble turns a page of chain rows into a delivery body. Every chain_seq
// from after+1 through the batch's last is accounted for: an event record, or
// a gap marker saying why it is absent (rows missing from the chain table,
// rows the destination's filter excludes, a row too large for one body). Rows
// are packed in order until the next would not fit; the remainder goes in the
// next batch, so nothing is skipped silently and the order never changes.
func assemble(d Destination, f format, after int64, page store.CompliancePage, head *Head, now time.Time) (batch, error) {
	budget := MaxBodyBytes - headReserveBytes
	var (
		out      batch
		size     int
		expected = after + 1
		filtered *Gap
	)
	out.through = after

	add := func(item Item) (fits bool, err error) {
		line, err := f.line(item)
		if err != nil {
			return false, err
		}
		if size+len(line)+1 > budget {
			return false, nil
		}
		out.body = append(out.body, line...)
		out.body = append(out.body, '\n')
		size += len(line) + 1
		return true, nil
	}
	flushFiltered := func() (bool, error) {
		if filtered == nil {
			return true, nil
		}
		fits, err := add(Item{At: now, Gap: filtered})
		if fits {
			out.through = filtered.To
			filtered = nil
		}
		return fits, err
	}

events:
	for i := range page.Events {
		ev := &page.Events[i]
		if ev.ChainSeq > expected {
			fits, err := flushFiltered()
			if err != nil || !fits {
				return finish(out, f, head, now, err)
			}
			gap := &Gap{OrgID: d.OrgID, From: expected, To: ev.ChainSeq - 1, Reason: GapMissing}
			if fits, err = add(Item{At: now, Gap: gap}); err != nil || !fits {
				return finish(out, f, head, now, err)
			}
			out.through = gap.To
		}
		expected = ev.ChainSeq + 1

		if !d.EventFilter.Matches(ev.Event, ev.Plane) {
			if filtered != nil && filtered.To == ev.ChainSeq-1 {
				filtered.To = ev.ChainSeq
			} else {
				fits, err := flushFiltered()
				if err != nil || !fits {
					return finish(out, f, head, now, err)
				}
				filtered = &Gap{OrgID: d.OrgID, From: ev.ChainSeq, To: ev.ChainSeq, Reason: GapFiltered}
			}
			continue
		}

		fits, err := flushFiltered()
		if err != nil || !fits {
			return finish(out, f, head, now, err)
		}
		item := Item{At: ev.OccurredAt, Event: ev}
		fits, err = add(item)
		if err != nil {
			return finish(out, f, head, now, err)
		}
		switch {
		case fits:
			out.events++
			out.through = ev.ChainSeq
		case len(out.body) == 0:
			// Larger than a whole body on its own: say so rather than stall the
			// destination behind a row that can never be sent.
			gap := &Gap{OrgID: d.OrgID, From: ev.ChainSeq, To: ev.ChainSeq, Reason: GapOversize}
			if _, err := add(Item{At: now, Gap: gap}); err != nil {
				return finish(out, f, head, now, err)
			}
			out.through = ev.ChainSeq
		default:
			break events
		}
	}
	if _, err := flushFiltered(); err != nil {
		return finish(out, f, head, now, err)
	}
	return finish(out, f, head, now, nil)
}

// finish closes the body with the chain head record and refuses an empty one.
func finish(out batch, f format, head *Head, now time.Time, err error) (batch, error) {
	if err != nil {
		return batch{}, err
	}
	if len(out.body) == 0 {
		return batch{}, errors.New("batch is empty")
	}
	if head != nil {
		line, err := f.line(Item{At: now, Head: head})
		if err != nil {
			return batch{}, err
		}
		out.body = append(out.body, line...)
		out.body = append(out.body, '\n')
	}
	return out, nil
}
