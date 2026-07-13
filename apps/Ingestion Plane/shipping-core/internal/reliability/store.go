// Package reliability computes F8's per-carrier on-time delivery score
// from shipping-core's own booking/tracking data — no external dependency,
// no fabricated numbers. A carrier only gets a score once it has at least
// MinSample bookings with both an estimated_delivery (snapshotted at
// booking time from the quote) and an actual_delivered_at (observed by
// carrier tracking); below that threshold it is simply absent from the
// result, matching this codebase's established "honest skip over fake
// pass" convention (see eval-lab's EVAL_CAPABILITIES gating).
package reliability

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// MinSample is the fewest scored bookings a carrier needs before Scores
// reports a value.
const MinSample = 5

// Window bounds how far back scoring looks, so a carrier's reliability
// reflects recent performance rather than its entire history.
const Window = 180 * 24 * time.Hour

// CarrierScore is one carrier's on-time delivery performance.
type CarrierScore struct {
	CarrierCode string  `json:"carrier_code"`
	OnTimeRate  float64 `json:"on_time_rate"`
	SampleSize  int     `json:"sample_size"`
}

// Store computes reliability scores from the bookings table.
type Store struct {
	pool *pgxpool.Pool
}

func NewStore(pool *pgxpool.Pool) *Store { return &Store{pool: pool} }

// Scores aggregates on-time-rate per carrier over Window, for carriers
// with at least MinSample scored bookings.
func (s *Store) Scores(ctx context.Context) ([]CarrierScore, error) {
	rows, err := s.pool.Query(ctx, `
		SELECT carrier_code,
		       count(*) AS sample_size,
		       count(*) FILTER (WHERE actual_delivered_at::date <= estimated_delivery) AS on_time_count
		FROM bookings
		WHERE estimated_delivery IS NOT NULL
		  AND actual_delivered_at IS NOT NULL
		  AND booked_at > now() - $1::interval
		GROUP BY carrier_code
		HAVING count(*) >= $2
		ORDER BY carrier_code`,
		fmt.Sprintf("%d seconds", int(Window.Seconds())), MinSample)
	if err != nil {
		return nil, fmt.Errorf("query reliability scores: %w", err)
	}
	defer rows.Close()

	out := []CarrierScore{}
	for rows.Next() {
		var (
			code           string
			sample, onTime int
		)
		if err := rows.Scan(&code, &sample, &onTime); err != nil {
			return nil, fmt.Errorf("scan reliability score: %w", err)
		}
		out = append(out, CarrierScore{
			CarrierCode: code,
			SampleSize:  sample,
			OnTimeRate:  float64(onTime) / float64(sample),
		})
	}
	return out, rows.Err()
}

// ScoreMap is Scores() keyed by carrier_code, for O(1) lookup while
// annotating a quote list.
func (s *Store) ScoreMap(ctx context.Context) (map[string]float64, error) {
	scores, err := s.Scores(ctx)
	if err != nil {
		return nil, err
	}
	out := make(map[string]float64, len(scores))
	for _, sc := range scores {
		out[sc.CarrierCode] = sc.OnTimeRate
	}
	return out, nil
}
