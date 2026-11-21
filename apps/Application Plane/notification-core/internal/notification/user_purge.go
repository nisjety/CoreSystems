package notification

import (
	"context"
	"errors"
	"fmt"
	"strings"
)

var errUserErasureRefused = errors.New("user erasure needs a real user id")

// userPurgeExempt names the tables the migrations create that a user erasure does not touch, with the reason each is exempt.
// notification_channel_configs holds organization policy keyed by organization and event type, and it names no user. The
// callback replay nonces carry no user either, and they expire on their own, as the organization erasure notes.
var userPurgeExempt = map[string]string{
	"notification_channel_configs":           "organization channel policy is keyed by organization and event type and names no user",
	"notification_delivery_callback_replays": "nonces carry no user and expire on their own",
}

// userPurgeSteps lists every statement that removes one user's rows in every organization, children before parents. Each
// takes the user id as its only parameter. Together with userPurgeExempt it names every table the migrations create, and a
// unit test pins that.
var userPurgeSteps = []struct {
	table     string
	statement string
}{
	{"notification_feed_projection_attempts", `DELETE FROM notification_feed_projection_attempts WHERE notification_id IN (SELECT id FROM notification_requests WHERE recipient_id = $1)`},
	{"notification_delivery_attempts", `DELETE FROM notification_delivery_attempts WHERE notification_id IN (SELECT id FROM notification_requests WHERE recipient_id = $1)`},
	{"notification_requests", `DELETE FROM notification_requests WHERE recipient_id = $1`},
	{"notification_feed_items", `DELETE FROM notification_feed_items WHERE recipient_id = $1`},
	{"notification_preferences", `DELETE FROM notification_preferences WHERE user_id = $1`},
	// Hard delete, not revoke: a sealed endpoint and its keys must not survive the erasure, revoked or not.
	{"notification_web_push_subscriptions", `DELETE FROM notification_web_push_subscriptions WHERE user_id = $1`},
	{"notification_subscriber_memberships", `DELETE FROM notification_subscriber_memberships WHERE user_id = $1`},
	{"notification_subscribers", `DELETE FROM notification_subscribers WHERE user_id = $1`},
}

// PurgeUser hard-deletes every row this store keeps for one user, in every organization, in one transaction, and returns the
// rows removed per table. Web Push subscriptions go with the rest, sealed endpoints included. Organization channel policy and
// the catalogue are left alone, and a replayed erasure deletes zero rows. Rows with no organization that name the user as their
// recipient are removed too, because they are the user's own data.
func (r *PGRepository) PurgeUser(ctx context.Context, userID string) (map[string]int64, error) {
	userID = strings.TrimSpace(userID)
	if userID == "" || userID == catalogueOrganizationID {
		return nil, errUserErasureRefused
	}
	if r == nil || r.pool == nil {
		return nil, fmt.Errorf("notification repository is not configured")
	}

	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin user purge: %w", err)
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	removed := make(map[string]int64, len(userPurgeSteps))
	for _, step := range userPurgeSteps {
		tag, err := tx.Exec(ctx, step.statement, userID)
		if err != nil {
			return nil, fmt.Errorf("purge %s: %w", step.table, err)
		}
		removed[step.table] = tag.RowsAffected()
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit user purge: %w", err)
	}
	committed = true
	return removed, nil
}
