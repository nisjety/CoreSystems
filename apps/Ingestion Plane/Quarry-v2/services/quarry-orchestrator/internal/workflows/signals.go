// Package workflows — pause/resume/cancel signals for crawl jobs.
//
// Operators send signals to a running CrawlJobWF via Temporal client to
// suspend the BFS loop at a safe boundary (after the current page) and
// resume or cancel later. Signal definitions match the Rust
// `crawl_signals::CrawlSignal` consumer side, so an external operator UI
// can drive both planes uniformly.
//
// Signal names (canonical, do not rename without bumping the workflow
// schema version):
//
//   - "pause"   → flips state to Paused; loop blocks before next pop
//   - "resume"  → flips state back to Running
//   - "cancel"  → flips state to Cancelled; loop exits on next iteration
//
// Query handlers expose the current signal state to the operator API
// without forcing them to dig through Temporal history.

package workflows

import (
	"time"

	"go.temporal.io/sdk/workflow"
)

// sleepDuringPause is how long waitWhilePaused naps between state checks.
// Short enough to feel responsive when the operator resumes, long enough
// not to spin Temporal history.
const sleepDuringPause = 5 * time.Second

const (
	// SignalPause asks the workflow to pause at the next safe boundary.
	SignalPause = "pause"
	// SignalResume asks the workflow to resume from a paused state.
	SignalResume = "resume"
	// SignalCancel asks the workflow to terminate gracefully (exits BFS
	// loop, then emits run_cancelled).
	SignalCancel = "cancel"

	// QueryState returns the current control state ("running"/"paused"/"cancelled").
	QueryState = "state"
	// QueryProgress returns visited/failed page counts mid-flight.
	QueryProgress = "progress"
)

type controlState int

const (
	stateRunning controlState = iota
	statePaused
	stateCancelled
)

func (s controlState) String() string {
	switch s {
	case statePaused:
		return "paused"
	case stateCancelled:
		return "cancelled"
	default:
		return "running"
	}
}

// crawlControl tracks signal state across a single CrawlJobWF execution.
// Created at workflow start; signal handlers mutate it via workflow.Go
// scheduling (cooperative scheduling — Temporal is single-threaded
// per workflow).
type crawlControl struct {
	state    controlState
	progress crawlProgress
}

type crawlProgress struct {
	Visited uint32 `json:"visited"`
	Failed  uint32 `json:"failed"`
	Pending uint32 `json:"pending"`
}

func newCrawlControl() *crawlControl {
	return &crawlControl{state: stateRunning}
}

// installControlHandlers wires signal + query handlers onto the workflow
// context. Returns the control struct so the workflow body can poll it.
func installControlHandlers(ctx workflow.Context) (*crawlControl, error) {
	ctrl := newCrawlControl()

	// Signal channels — non-blocking receivers in a goroutine.
	pauseCh := workflow.GetSignalChannel(ctx, SignalPause)
	resumeCh := workflow.GetSignalChannel(ctx, SignalResume)
	cancelCh := workflow.GetSignalChannel(ctx, SignalCancel)

	workflow.Go(ctx, func(gctx workflow.Context) {
		for {
			selector := workflow.NewSelector(gctx)
			selector.AddReceive(pauseCh, func(c workflow.ReceiveChannel, _ bool) {
				var v any
				c.Receive(gctx, &v)
				if ctrl.state != stateCancelled {
					ctrl.state = statePaused
				}
			})
			selector.AddReceive(resumeCh, func(c workflow.ReceiveChannel, _ bool) {
				var v any
				c.Receive(gctx, &v)
				if ctrl.state != stateCancelled {
					ctrl.state = stateRunning
				}
			})
			selector.AddReceive(cancelCh, func(c workflow.ReceiveChannel, _ bool) {
				var v any
				c.Receive(gctx, &v)
				ctrl.state = stateCancelled
			})
			selector.Select(gctx)
			if ctrl.state == stateCancelled {
				return
			}
		}
	})

	if err := workflow.SetQueryHandler(ctx, QueryState, func() (string, error) {
		return ctrl.state.String(), nil
	}); err != nil {
		return nil, err
	}
	if err := workflow.SetQueryHandler(ctx, QueryProgress, func() (crawlProgress, error) {
		return ctrl.progress, nil
	}); err != nil {
		return nil, err
	}

	return ctrl, nil
}

// waitWhilePaused blocks the workflow when state == Paused. Returns true
// when the workflow should continue, false when cancelled.
func (c *crawlControl) waitWhilePaused(ctx workflow.Context) bool {
	for c.state == statePaused {
		// Sleep is cancellable and survives worker restarts.
		_ = workflow.Sleep(ctx, sleepDuringPause)
	}
	return c.state != stateCancelled
}
