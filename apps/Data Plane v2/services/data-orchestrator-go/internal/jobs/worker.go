// Package jobs — durable job worker (plan P2-4, closes defect D12).
//
// Replaces `OrchestratorHandler.launch`'s fire-and-forget
// `go func(){ executor.Run(...) }()`. That design pinned work to whichever
// replica served the POST and left a row stranded in `running` forever if the
// process restarted mid-job, because nothing ever polled the table.
//
// The HTTP handler now only records intent (`Create` → `pending`, 202) and this
// worker performs it. That makes a job survive a restart, lets any replica pick
// it up, and turns a transient failure into a retry instead of a permanent
// half-finished job.
//
// This is the same lease/claim shape already used by `wiki_event_outbox`,
// `index_deletion_outbox` and `quickwit_admin_jobs` — `data_orchestrator_jobs`
// was the one queue in the plane that lacked it.
package jobs

import (
	"context"
	"time"

	"github.com/rs/zerolog/log"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

const (
	// LeaseSeconds bounds how long a claimed job may run before another worker
	// may consider it dead. Generous because a reindex can legitimately take
	// minutes; `SetProgress` renews it, so this is the ceiling on *silence*,
	// not on total runtime.
	LeaseSeconds = 300

	// PollInterval between claim attempts when the queue is empty. Jobs are
	// operator-initiated and not latency-critical, so this is deliberately slow
	// enough not to add measurable load to a Postgres shared by nine services.
	PollInterval = 2 * time.Second

	// MaxAttempts before a job is failed terminally. Counted on *claim*, so a
	// worker that dies without reporting still burns one — otherwise a job that
	// reliably crashes its worker would be retried forever.
	MaxAttempts = 3
)

// Runner is the execution half, satisfied by *Executor.
type Runner interface {
	Run(ctx context.Context, job model.Job) error
}

// Worker claims and executes durable jobs.
type Worker struct {
	store    JobStore
	executor Runner
	owner    string
}

func NewWorker(store JobStore, executor Runner, owner string) *Worker {
	return &Worker{store: store, executor: executor, owner: owner}
}

// Run polls until `ctx` is cancelled.
//
// Every error path is non-fatal: the loop logs and continues, because a worker
// that exits on a transient database blip would silently stop all durable job
// processing for the process's lifetime.
func (w *Worker) Run(ctx context.Context) {
	log.Info().
		Str("owner", w.owner).
		Int("lease_seconds", LeaseSeconds).
		Int("max_attempts", MaxAttempts).
		Msg("durable job worker started")

	ticker := time.NewTicker(PollInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			log.Info().Str("owner", w.owner).Msg("durable job worker stopping")
			return
		case <-ticker.C:
			w.tick(ctx)
		}
	}
}

func (w *Worker) tick(ctx context.Context) {
	// Retire permanently-failing jobs first. They are already excluded from
	// ClaimNext, so without this they would sit in `running` forever, never
	// reported as failed.
	if n, err := w.store.ExpireExhausted(ctx, MaxAttempts); err != nil {
		log.Warn().Err(err).Msg("expire exhausted jobs failed")
	} else if n > 0 {
		log.Warn().Int64("count", n).Msg("jobs abandoned after exhausting attempts")
	}

	job, err := w.store.ClaimNext(ctx, w.owner, LeaseSeconds*time.Second, MaxAttempts)
	if err != nil {
		log.Warn().Err(err).Msg("claim next job failed")
		return
	}
	if job == nil {
		return // idle
	}

	log.Info().
		Str("job_id", job.JobID).
		Str("org_id", job.OrgID).
		Str("job_type", string(job.JobType)).
		Msg("durable job claimed")

	// Detached from the poll tick: cancelling a tick must not kill a job that is
	// already underway. The lease is what bounds it instead.
	jobCtx := context.WithoutCancel(ctx)
	if err := w.executor.Run(jobCtx, *job); err != nil {
		// Do NOT mark failed here. The executor owns terminal state (it calls
		// Complete/Fail itself), and leaving the lease to expire is what allows
		// a retry. ExpireExhausted is the terminal backstop.
		log.Error().Err(err).Str("job_id", job.JobID).Msg("durable job execution failed")
		return
	}
	log.Info().Str("job_id", job.JobID).Msg("durable job finished")
}
