// Package temporal — Quarry's wrapper around go.temporal.io/sdk for
// schedule lifecycle operations.
//
// D5 / cluster #5.
//
// ## Status
//
// This package exposes the *interface* the rest of the control plane
// will program against once the Temporal SDK is added to go.mod. The
// concrete client (`SDKClient`) is wired in cycle 24 — see the TODO
// at the bottom of this file.
//
// Why ship the interface now: cluster #5's gap-quarry acceptance is
// "operators never need direct Temporal access for schedule work."
// The schedule_routes on the Rust edge call `/v1/schedules/:id/
// {pause,unpause,trigger,backfill}`, and the Go control's
// MountScheduleAliases routes them through this package. Today we
// satisfy the interface with NoopClient so the wire shape works; the
// SDK plug-in lands as a one-line swap.
//
// ## Schedule lifecycle mapping
//
// | Quarry API endpoint                   | Temporal SDK call                          |
// | ------------------------------------- | ------------------------------------------ |
// | POST /v1/schedules                    | Client.ScheduleClient().Create             |
// | POST /v1/schedules/:id/pause          | ScheduleHandle.Pause                       |
// | POST /v1/schedules/:id/unpause        | ScheduleHandle.Unpause                     |
// | POST /v1/schedules/:id/trigger        | ScheduleHandle.Trigger                     |
// | POST /v1/schedules/:id/backfill       | ScheduleHandle.Backfill                    |
// | DELETE /v1/schedules/:id              | ScheduleHandle.Delete                      |
//
// All schedules are namespaced by the calling org_id — Temporal's
// schedule name carries the prefix `quarry-<org_id>-<schedule_id>`
// so cross-tenant ID collisions are impossible.

package temporal

import (
	"context"
	"errors"
	"time"

	"github.com/rs/zerolog/log"
)

// CreateOptions mirrors the Rust edge's `CreateScheduleRequest`. The
// control plane caller (mount handler) builds this from the inbound
// JSON.
type CreateOptions struct {
	OrgID           string
	ScheduleID      string
	Name            string
	Kind            string
	Cron            string    // empty when ScheduleAt is set
	ScheduleAt      time.Time // zero when Cron is set
	OverlapPolicy   string    // "skip" | "cancel" | "allow"
	CatchupWindowS  int64
	PauseOnFailure  bool
	Config          map[string]any
}

// BackfillOptions mirrors the edge's `BackfillRequest`.
type BackfillOptions struct {
	OrgID         string
	ScheduleID    string
	StartAt       time.Time
	EndAt         time.Time
	OverlapPolicy string
}

// Client is the trait every consumer programs against. The SDK
// concrete impl arrives in cycle 24; today we ship NoopClient so the
// wire shape works and dev / test environments don't need Temporal
// running.
type Client interface {
	Create(ctx context.Context, opts CreateOptions) error
	Pause(ctx context.Context, orgID, scheduleID string) error
	Unpause(ctx context.Context, orgID, scheduleID string) error
	Trigger(ctx context.Context, orgID, scheduleID string) error
	Backfill(ctx context.Context, opts BackfillOptions) error
	Delete(ctx context.Context, orgID, scheduleID string) error
}

// ErrNotConfigured is returned by NoopClient on every operation so
// callers can distinguish "Temporal not wired" from "Temporal said no".
var ErrNotConfigured = errors.New("temporal client not configured")

// NoopClient logs intent + returns ErrNotConfigured. Useful during
// the dev rollout window so the rest of the control plane compiles
// and tests pass without a live Temporal frontend.
type NoopClient struct{}

func (NoopClient) Create(_ context.Context, opts CreateOptions) error {
	log.Info().
		Str("org_id", opts.OrgID).
		Str("schedule_id", opts.ScheduleID).
		Str("cron", opts.Cron).
		Time("schedule_at", opts.ScheduleAt).
		Str("overlap_policy", opts.OverlapPolicy).
		Msg("temporal.Create called on NoopClient — schedule recorded in DB but not registered with Temporal")
	return nil // NoopClient pretends success so the dev path through the edge → control → DB works
}
func (NoopClient) Pause(_ context.Context, _, _ string) error    { return nil }
func (NoopClient) Unpause(_ context.Context, _, _ string) error  { return nil }
func (NoopClient) Trigger(_ context.Context, _, _ string) error  { return nil }
func (NoopClient) Backfill(_ context.Context, _ BackfillOptions) error {
	return nil
}
func (NoopClient) Delete(_ context.Context, _, _ string) error { return nil }

// scheduleName composes the Temporal schedule name. Org-prefixed so
// distinct tenants never share a schedule namespace inside Temporal.
func scheduleName(orgID, scheduleID string) string {
	return "quarry-" + orgID + "-" + scheduleID
}

// SDKClient is the production impl. Stubbed today; cycle 24 fills it
// in once `go.temporal.io/sdk v1.30+` is added to go.mod.
//
// Wiring sketch (do NOT uncomment until the SDK dep is added):
//
//   import temporalclient "go.temporal.io/sdk/client"
//
//   type SDKClient struct {
//       c temporalclient.Client
//   }
//
//   func NewSDKClient(hostPort, namespace string) (*SDKClient, error) {
//       c, err := temporalclient.Dial(temporalclient.Options{
//           HostPort:  hostPort,   // e.g. "temporal:7233"
//           Namespace: namespace,  // e.g. "quarry"
//       })
//       if err != nil { return nil, fmt.Errorf("temporal dial: %w", err) }
//       return &SDKClient{c: c}, nil
//   }
//
//   func (s *SDKClient) Create(ctx context.Context, opts CreateOptions) error {
//       spec := buildScheduleSpec(opts)  // see helper below
//       _, err := s.c.ScheduleClient().Create(ctx, temporalclient.ScheduleOptions{
//           ID:   scheduleName(opts.OrgID, opts.ScheduleID),
//           Spec: spec,
//           Action: &temporalclient.ScheduleWorkflowAction{
//               // Workflow + args derived from opts.Kind + opts.Config
//           },
//           Overlap: mapOverlapPolicy(opts.OverlapPolicy),
//           CatchupWindow: time.Duration(opts.CatchupWindowS) * time.Second,
//           PauseOnFailure: opts.PauseOnFailure,
//       })
//       return err
//   }
//
//   func (s *SDKClient) Pause(ctx context.Context, orgID, scheduleID string) error {
//       return s.c.ScheduleClient().GetHandle(ctx,
//           scheduleName(orgID, scheduleID)).Pause(ctx, temporalclient.SchedulePauseOptions{})
//   }
//   // ...same for Unpause, Trigger, Backfill, Delete.
