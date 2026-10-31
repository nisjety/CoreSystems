package store

import (
	"context"
	"errors"
	"fmt"
	"time"
)

// exporterActiveWindow is how recently the SIEM exporter must have tried a
// destination for the org to count it as running. The exporter polls every 30
// seconds and backs off at most 15 minutes, so a longer silence means it is
// stopped (or its profile is off), not waiting.
const exporterActiveWindow = 20 * time.Minute

// ExportDestinationStatus is what the exporter's cursor says about one
// destination of an org. It carries no URL, secret or response text: those are
// org-core's and never stored here.
type ExportDestinationStatus struct {
	DestinationID       string     `json:"destination_id"`
	Kind                string     `json:"kind"`
	CursorSeq           int64      `json:"cursor_seq"`
	DeliveredTotal      int64      `json:"delivered_total"`
	ConsecutiveFailures int        `json:"consecutive_failures"`
	LastError           string     `json:"last_error"`
	LastAttemptAt       *time.Time `json:"last_attempt_at"`
	LastSuccessAt       *time.Time `json:"last_success_at"`
}

// ExportStatus is the org's view of the SIEM export. Active is true when the
// exporter attempted any of the org's destinations recently; HeadSeq is the
// org's chain head, so a reader sees how far each cursor is behind it.
type ExportStatus struct {
	Active       bool
	HeadSeq      int64
	Destinations []ExportDestinationStatus
}

// ExportStatus reads the exporter's cursors for one org.
func (s *Store) ExportStatus(ctx context.Context, orgID string) (ExportStatus, error) {
	if orgID == "" {
		return ExportStatus{}, errors.New("export status: org id is required")
	}
	status := ExportStatus{Destinations: []ExportDestinationStatus{}}
	var err error
	if status.HeadSeq, err = readHeadSeq(ctx, s.pool, orgID); err != nil {
		return ExportStatus{}, err
	}
	rows, err := s.pool.Query(ctx, `
		SELECT destination_id, kind, cursor_seq, delivered_total, consecutive_failures,
		       last_error, last_attempt_at, last_success_at
		FROM audit_export_cursors
		WHERE org_id = $1
		ORDER BY destination_id
	`, orgID)
	if err != nil {
		return ExportStatus{}, fmt.Errorf("read export cursors: %w", err)
	}
	defer rows.Close()
	cutoff := time.Now().Add(-exporterActiveWindow)
	for rows.Next() {
		var d ExportDestinationStatus
		if err := rows.Scan(&d.DestinationID, &d.Kind, &d.CursorSeq, &d.DeliveredTotal,
			&d.ConsecutiveFailures, &d.LastError, &d.LastAttemptAt, &d.LastSuccessAt); err != nil {
			return ExportStatus{}, fmt.Errorf("scan export cursor: %w", err)
		}
		if d.LastAttemptAt != nil && d.LastAttemptAt.After(cutoff) {
			status.Active = true
		}
		status.Destinations = append(status.Destinations, d)
	}
	if err := rows.Err(); err != nil {
		return ExportStatus{}, fmt.Errorf("read export cursors: %w", err)
	}
	return status, nil
}
