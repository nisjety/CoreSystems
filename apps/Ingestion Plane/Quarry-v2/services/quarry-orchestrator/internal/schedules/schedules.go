package schedules

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
	"go.temporal.io/sdk/client"
)

// Config configures the schedule reconciler.
type Config struct {
	ControlBaseURL   string
	ControlAuthToken string
	TaskQueue        string
	HTTPClient       *http.Client
	Interval         time.Duration
}

// Manager reconciles schedules from Control Plane into Temporal.
type Manager struct {
	cfg Config
	tc  client.Client
	log zerolog.Logger
}

// ScheduleSpec is the wire payload from Control Plane.
type ScheduleSpec struct {
	ID       string        `json:"id"`
	Name     string        `json:"name"`
	Cron     string        `json:"cron"`
	Workflow string        `json:"workflow"`
	Args     []interface{} `json:"args"`
	Paused   bool          `json:"paused"`
}

// New builds a Manager.
func New(tc client.Client, cfg Config) *Manager {
	if cfg.Interval <= 0 {
		cfg.Interval = 30 * time.Second
	}
	if cfg.HTTPClient == nil {
		cfg.HTTPClient = &http.Client{Timeout: 30 * time.Second}
	}
	return &Manager{
		cfg: cfg,
		tc:  tc,
		log: log.With().Str("component", "schedules").Logger(),
	}
}

// Run starts the reconciliation loop; returns when ctx is cancelled.
func (m *Manager) Run(ctx context.Context) error {
	t := time.NewTicker(m.cfg.Interval)
	defer t.Stop()

	// Initial reconcile.
	if err := m.reconcile(ctx); err != nil {
		m.log.Warn().Err(err).Msg("initial reconcile failed")
	}

	for {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-t.C:
			if err := m.reconcile(ctx); err != nil {
				m.log.Warn().Err(err).Msg("reconcile failed")
			}
		}
	}
}

func (m *Manager) reconcile(ctx context.Context) error {
	desired, err := m.fetchDesired(ctx)
	if err != nil {
		return fmt.Errorf("fetch desired: %w", err)
	}

	existing, err := m.listExisting(ctx)
	if err != nil {
		return fmt.Errorf("list existing: %w", err)
	}

	desiredSet := make(map[string]ScheduleSpec, len(desired))
	for _, s := range desired {
		desiredSet[s.ID] = s
	}

	sc := m.tc.ScheduleClient()

	// Create or update.
	for id, spec := range desiredSet {
		if _, ok := existing[id]; !ok {
			if err := m.create(ctx, sc, spec); err != nil {
				m.log.Error().Err(err).Str("id", id).Msg("create schedule failed")
			} else {
				m.log.Info().Str("id", id).Msg("schedule created")
			}
			continue
		}
		if err := m.update(ctx, sc, spec); err != nil {
			m.log.Error().Err(err).Str("id", id).Msg("update schedule failed")
		}
	}

	// Delete orphans.
	for id := range existing {
		if _, ok := desiredSet[id]; ok {
			continue
		}
		h := sc.GetHandle(ctx, id)
		if err := h.Delete(ctx); err != nil {
			m.log.Error().Err(err).Str("id", id).Msg("delete schedule failed")
		} else {
			m.log.Info().Str("id", id).Msg("schedule deleted")
		}
	}

	return nil
}

func (m *Manager) fetchDesired(ctx context.Context) ([]ScheduleSpec, error) {
	url := m.cfg.ControlBaseURL + "/v1/schedules"
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, url, nil)
	if err != nil {
		return nil, err
	}
	if m.cfg.ControlAuthToken != "" {
		req.Header.Set("Authorization", "Bearer "+m.cfg.ControlAuthToken)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := m.cfg.HTTPClient.Do(req)
	if err != nil {
		return nil, err
	}
	defer resp.Body.Close()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		body, _ := io.ReadAll(resp.Body)
		return nil, fmt.Errorf("control plane returned %d: %s", resp.StatusCode, string(body))
	}

	// quarry-control wraps list responses in the project-standard envelope
	// `{"data": [...], "meta": {...}, "error": null}`. Decode the envelope
	// and return the inner array. We also tolerate a bare-array response
	// (older fixtures + the unit tests use that shape) by inspecting the
	// first non-whitespace byte before choosing a decoder.
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return nil, fmt.Errorf("read body: %w", err)
	}

	trimmed := bytes.TrimLeft(body, " \t\r\n")
	if len(trimmed) > 0 && trimmed[0] == '[' {
		var bare []ScheduleSpec
		if err := json.Unmarshal(body, &bare); err != nil {
			return nil, fmt.Errorf("decode bare array: %w", err)
		}
		return bare, nil
	}

	var envelope struct {
		Data  []ScheduleSpec `json:"data"`
		Error *string        `json:"error"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		return nil, fmt.Errorf("decode: %w", err)
	}
	if envelope.Error != nil && *envelope.Error != "" {
		return nil, fmt.Errorf("control plane error: %s", *envelope.Error)
	}
	return envelope.Data, nil
}

func (m *Manager) listExisting(ctx context.Context) (map[string]struct{}, error) {
	sc := m.tc.ScheduleClient()
	it, err := sc.List(ctx, client.ScheduleListOptions{})
	if err != nil {
		return nil, err
	}
	out := make(map[string]struct{})
	for it.HasNext() {
		entry, err := it.Next()
		if err != nil {
			return nil, err
		}
		out[entry.ID] = struct{}{}
	}
	return out, nil
}

func (m *Manager) create(ctx context.Context, sc client.ScheduleClient, spec ScheduleSpec) error {
	_, err := sc.Create(ctx, client.ScheduleOptions{
		ID: spec.ID,
		Spec: client.ScheduleSpec{
			CronExpressions: []string{spec.Cron},
		},
		Action: &client.ScheduleWorkflowAction{
			ID:        spec.ID + "-wf",
			Workflow:  spec.Workflow,
			Args:      spec.Args,
			TaskQueue: m.cfg.TaskQueue,
		},
		Paused: spec.Paused,
		Note:   spec.Name,
	})
	return err
}

func (m *Manager) update(ctx context.Context, sc client.ScheduleClient, spec ScheduleSpec) error {
	h := sc.GetHandle(ctx, spec.ID)

	err := h.Update(ctx, client.ScheduleUpdateOptions{
		DoUpdate: func(in client.ScheduleUpdateInput) (*client.ScheduleUpdate, error) {
			sched := in.Description.Schedule
			sched.Spec = &client.ScheduleSpec{
				CronExpressions: []string{spec.Cron},
			}
			sched.Action = &client.ScheduleWorkflowAction{
				ID:        spec.ID + "-wf",
				Workflow:  spec.Workflow,
				Args:      spec.Args,
				TaskQueue: m.cfg.TaskQueue,
			}
			return &client.ScheduleUpdate{Schedule: &sched}, nil
		},
	})
	if err != nil {
		return fmt.Errorf("update: %w", err)
	}

	// Reconcile pause state explicitly (Update does not alter State by default).
	if spec.Paused {
		if err := h.Pause(ctx, client.SchedulePauseOptions{Note: "reconciled: paused"}); err != nil {
			return fmt.Errorf("pause: %w", err)
		}
	} else {
		if err := h.Unpause(ctx, client.ScheduleUnpauseOptions{Note: "reconciled: unpaused"}); err != nil {
			return fmt.Errorf("unpause: %w", err)
		}
	}
	return nil
}
