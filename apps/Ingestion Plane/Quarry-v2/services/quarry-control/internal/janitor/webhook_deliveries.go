// The webhook delivery retention sweep (INNH-5): ages out terminal webhook_deliveries rows.
//
// A delivery row stores the whole event payload (page URLs, fingerprints), so it must not
// live forever. Only rows that reached a terminal status (success, failed, dlq) are deleted;
// pending and in-flight rows are never touched. The sweep is started only when the
// subscription API is on (QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS), the only way new rows
// appear for tenants.
//
//   - QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS: days to keep a terminal delivery.
//     Unset or empty: 14. A value that is not an integer from 1 to 3650 logs and falls
//     back to 14; it never disables the sweep.
package janitor

import (
	"context"
	"strconv"
	"strings"
	"time"

	"github.com/rs/zerolog"

	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// DefaultWebhookDeliveryRetentionDays is the retention when nothing valid is configured.
const DefaultWebhookDeliveryRetentionDays = 14

// WebhookDeliveryRetentionDaysFromEnv reads the retention through getenv. The second result
// is true when a value was set but invalid and the default was used instead.
func WebhookDeliveryRetentionDaysFromEnv(getenv func(string) string) (int, bool) {
	raw := strings.TrimSpace(getenv("QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS"))
	if raw == "" {
		return DefaultWebhookDeliveryRetentionDays, false
	}
	n, err := strconv.Atoi(raw)
	if err != nil || n < 1 || n > 3650 {
		return DefaultWebhookDeliveryRetentionDays, true
	}
	return n, false
}

// SweepWebhookDeliveries deletes terminal deliveries created more than retentionDays before
// now and returns how many it removed. A store error is returned; the loop logs it and tries
// again on the next tick.
func SweepWebhookDeliveries(db store.DB, retentionDays int, now time.Time) (int64, error) {
	cutoff := now.UTC().Add(-time.Duration(retentionDays) * 24 * time.Hour).Unix()
	return db.WebhookDeliveries().DeleteTerminalBefore(cutoff)
}

// RunWebhookDeliveries is the sweep loop; it honours ctx cancellation. Logs carry counts only.
func RunWebhookDeliveries(ctx context.Context, db store.DB, retentionDays int, interval time.Duration, logger zerolog.Logger) {
	if interval <= 0 {
		interval = time.Hour
	}
	logger.Info().Int("retention_days", retentionDays).Dur("interval", interval).Msg("webhook delivery retention sweep started")
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		if n, err := SweepWebhookDeliveries(db, retentionDays, time.Now()); err != nil {
			logger.Warn().Err(err).Msg("webhook delivery retention sweep failed")
		} else if n > 0 {
			logger.Info().Int64("deleted", n).Msg("webhook delivery retention sweep")
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
