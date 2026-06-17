package social

import (
	"context"
	"crypto/sha1"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

type PGRepository struct {
	pool *pgxpool.Pool
}

type rowScanner interface {
	Scan(dest ...any) error
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	return &PGRepository{pool: pool}
}

func (r *PGRepository) ListAccounts(ctx context.Context, orgID string) ([]Account, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, provider_key, connection_id, display_name, handle, status,
	capabilities, token_state, token_expires_at, metadata, created_at, updated_at
FROM social_accounts
WHERE org_id = $1
ORDER BY provider_key ASC, display_name ASC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	accounts := []Account{}
	for rows.Next() {
		account, err := scanAccount(rows)
		if err != nil {
			return nil, err
		}
		accounts = append(accounts, *account)
	}
	return accounts, rows.Err()
}

func (r *PGRepository) UpsertAccounts(ctx context.Context, orgID string, accounts []Account) error {
	if err := r.ensureConfigured(); err != nil {
		return err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	for _, account := range accounts {
		account.OrgID = strings.TrimSpace(firstNonEmpty(account.OrgID, orgID))
		account.ProviderKey = normalizePlatform(account.ProviderKey)
		account.ConnectionID = strings.TrimSpace(account.ConnectionID)
		account.DisplayName = strings.TrimSpace(account.DisplayName)
		account.Handle = strings.TrimSpace(account.Handle)
		account.Status = fallback(account.Status, AccountStatusDisconnected)
		account.TokenState = fallback(account.TokenState, "missing")
		account.Metadata = ensureMap(account.Metadata)
		if account.ID == "" {
			account.ID = stableAccountID(account.OrgID, account.ProviderKey, account.ConnectionID)
		}
		if account.OrgID == "" || account.ProviderKey == "" {
			continue
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO social_accounts (
	id, org_id, provider_key, connection_id, display_name, handle, status,
	capabilities, token_state, token_expires_at, metadata, updated_at
) VALUES (
	$1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9, $10, $11::jsonb, NOW()
)
ON CONFLICT (id)
DO UPDATE SET
	connection_id = EXCLUDED.connection_id,
	display_name = EXCLUDED.display_name,
	handle = EXCLUDED.handle,
	status = EXCLUDED.status,
	capabilities = EXCLUDED.capabilities,
	token_state = EXCLUDED.token_state,
	token_expires_at = EXCLUDED.token_expires_at,
	metadata = EXCLUDED.metadata,
	updated_at = NOW()`,
			account.ID, account.OrgID, account.ProviderKey, account.ConnectionID, account.DisplayName,
			account.Handle, account.Status, mustJSON(account.Capabilities), account.TokenState,
			account.TokenExpiresAt, mustJSON(account.Metadata)); err != nil {
			return err
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return err
	}
	committed = true
	return nil
}

func (r *PGRepository) ListCampaigns(ctx context.Context, filter ListCampaignsFilter) ([]Campaign, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, name, brief, goal, status, platforms, starts_at, ends_at,
	source, metadata, owner_user_id, created_at, updated_at
FROM social_campaigns
WHERE org_id = $1
  AND ($2 = '' OR status = $2)
ORDER BY COALESCE(starts_at, created_at) ASC, updated_at DESC
LIMIT $3`, filter.OrgID, filter.Status, filter.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	campaigns := []Campaign{}
	for rows.Next() {
		campaign, err := scanCampaign(rows)
		if err != nil {
			return nil, err
		}
		campaigns = append(campaigns, *campaign)
	}
	return campaigns, rows.Err()
}

func (r *PGRepository) CreateCampaign(ctx context.Context, input CreateCampaignInput) (*Campaign, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, `
INSERT INTO social_campaigns (
	id, org_id, name, brief, goal, status, platforms, starts_at, ends_at,
	source, metadata, owner_user_id
) VALUES (
	$1, $2, $3, $4, $5, $6, $7::jsonb, $8, $9, $10::jsonb, $11::jsonb, $12
)
RETURNING id, org_id, name, brief, goal, status, platforms, starts_at, ends_at,
	source, metadata, owner_user_id, created_at, updated_at`,
		newID("soccamp"), input.OrgID, input.Name, input.Brief, input.Goal, input.Status,
		mustJSON(input.Platforms), input.StartsAt, input.EndsAt, mustJSON(input.Source),
		mustJSON(input.Metadata), input.ActorUserID)
	return scanCampaign(row)
}

func (r *PGRepository) ListPosts(ctx context.Context, filter ListPostsFilter) ([]Post, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, title, body, status, platforms, media, source, previews, ai_context,
	approval_required, approval_state, scheduled_at, created_by_user_id, updated_by_user_id,
	created_at, updated_at
FROM social_posts
WHERE org_id = $1
  AND ($2 = '' OR status = $2)
  AND ($3 = '' OR platforms ? $3)
ORDER BY COALESCE(scheduled_at, updated_at) ASC, updated_at DESC
LIMIT $4`, filter.OrgID, filter.Status, filter.Platform, filter.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	posts := []Post{}
	for rows.Next() {
		post, err := scanPost(rows)
		if err != nil {
			return nil, err
		}
		posts = append(posts, *post)
	}
	return posts, rows.Err()
}

func (r *PGRepository) CreatePost(ctx context.Context, input CreatePostInput) (*Post, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	approvalRequired := true
	if input.ApprovalRequired != nil {
		approvalRequired = *input.ApprovalRequired
	}
	approvalState := ApprovalPending
	if !approvalRequired {
		approvalState = ApprovalNotRequired
	}
	status := PostStatusDraft
	if input.ScheduledAt != nil {
		status = PostStatusScheduled
	}
	previews := BuildPreviews(input.Platforms, input.Body, input.Media)

	row := tx.QueryRow(ctx, `
INSERT INTO social_posts (
	id, org_id, title, body, status, platforms, media, source, previews, ai_context,
	approval_required, approval_state, scheduled_at, created_by_user_id, updated_by_user_id
) VALUES (
	$1, $2, $3, $4, $5, $6::jsonb, $7::jsonb, $8::jsonb, $9::jsonb, $10::jsonb,
	$11, $12, $13, $14, $14
)
RETURNING id, org_id, title, body, status, platforms, media, source, previews, ai_context,
	approval_required, approval_state, scheduled_at, created_by_user_id, updated_by_user_id,
	created_at, updated_at`,
		newID("socpost"), input.OrgID, input.Title, input.Body, status,
		mustJSON(input.Platforms), mustJSON(input.Media), mustJSON(input.Source), mustJSON(previews),
		mustJSON(input.AIContext), approvalRequired, approvalState, input.ScheduledAt, input.ActorUserID)
	post, err := scanPost(row)
	if err != nil {
		return nil, err
	}
	if approvalRequired {
		if _, err := tx.Exec(ctx, `
INSERT INTO social_approvals (
	id, org_id, post_id, state, requested_by_user_id, metadata
) VALUES (
	$1, $2, $3, 'pending', $4, $5::jsonb
)
ON CONFLICT DO NOTHING`,
			newID("socapr"), input.OrgID, post.ID, input.ActorUserID, mustJSON(map[string]any{
				"source": "post_create",
			})); err != nil {
			return nil, err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true
	return post, nil
}

func (r *PGRepository) ListApprovals(ctx context.Context, filter ListApprovalsFilter) ([]Approval, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	rows, err := r.pool.Query(ctx, `
SELECT id, org_id, post_id, campaign_id, state, requested_by_user_id, requested_of_user_id,
	decided_by_user_id, decision_reason, due_at, decided_at, metadata, created_at, updated_at
FROM social_approvals
WHERE org_id = $1
  AND ($2 = '' OR state = $2)
  AND ($3 = '' OR post_id = $3)
  AND ($4 = '' OR campaign_id = $4)
ORDER BY created_at ASC
LIMIT $5`, filter.OrgID, filter.State, filter.PostID, filter.CampaignID, filter.Limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	approvals := []Approval{}
	for rows.Next() {
		approval, err := scanApproval(rows)
		if err != nil {
			return nil, err
		}
		approvals = append(approvals, *approval)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	for idx := range approvals {
		if approvals[idx].PostID == "" {
			continue
		}
		if post, err := r.getPost(ctx, approvals[idx].OrgID, approvals[idx].PostID); err == nil {
			approvals[idx].Post = post
		}
	}
	return approvals, nil
}

func (r *PGRepository) DecideApproval(ctx context.Context, input DecideApprovalInput) (*Approval, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	approval, err := r.getApprovalTx(ctx, tx, input.OrgID, input.ApprovalID)
	if err != nil {
		return nil, err
	}
	if approval.State != ApprovalPending {
		return nil, fmt.Errorf("%w: approval is already decided", ErrInvalidInput)
	}

	row := tx.QueryRow(ctx, `
UPDATE social_approvals
SET state = $3,
	decided_by_user_id = $4,
	decision_reason = $5,
	decided_at = $6,
	updated_at = $6
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, post_id, campaign_id, state, requested_by_user_id, requested_of_user_id,
	decided_by_user_id, decision_reason, due_at, decided_at, metadata, created_at, updated_at`,
		input.OrgID, input.ApprovalID, input.Decision, input.ActorUserID, input.DecisionReason, input.DecidedAt)
	updated, err := scanApproval(row)
	if err != nil {
		return nil, err
	}
	if updated.PostID != "" {
		if _, err := tx.Exec(ctx, `
UPDATE social_posts
SET approval_state = $3,
	updated_by_user_id = $4,
	updated_at = $5
WHERE org_id = $1 AND id = $2`, input.OrgID, updated.PostID, input.Decision, input.ActorUserID, input.DecidedAt); err != nil {
			return nil, err
		}
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true
	if updated.PostID != "" {
		if post, err := r.getPost(ctx, updated.OrgID, updated.PostID); err == nil {
			updated.Post = post
		}
	}
	return updated, nil
}

func (r *PGRepository) UpdatePostSchedule(ctx context.Context, input SchedulePostInput) (*Post, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	row := r.pool.QueryRow(ctx, `
UPDATE social_posts
SET status = 'scheduled',
	scheduled_at = $3,
	updated_by_user_id = $4,
	updated_at = NOW()
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, title, body, status, platforms, media, source, previews, ai_context,
	approval_required, approval_state, scheduled_at, created_by_user_id, updated_by_user_id,
	created_at, updated_at`, input.OrgID, input.PostID, input.ScheduledAt, input.ActorUserID)
	post, err := scanPost(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return post, err
}

func (r *PGRepository) EnqueuePublishJob(ctx context.Context, input EnqueuePublishInput) (*PublishJob, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	var postID string
	if err := tx.QueryRow(ctx, `
UPDATE social_posts
SET status = CASE WHEN $3 > NOW() THEN 'scheduled' ELSE 'publishing' END,
	updated_by_user_id = $4,
	updated_at = NOW()
WHERE org_id = $1 AND id = $2
RETURNING id`, input.OrgID, input.PostID, input.ScheduledFor, input.RequestedByUserID).Scan(&postID); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}

	row := tx.QueryRow(ctx, `
INSERT INTO social_publish_jobs (
	id, org_id, post_id, status, idempotency_key, requested_by_user_id, scheduled_for
) VALUES (
	$1, $2, $3, 'queued', $4, $5, $6
)
ON CONFLICT (org_id, idempotency_key)
DO UPDATE SET updated_at = social_publish_jobs.updated_at
RETURNING id, org_id, post_id, status, idempotency_key, requested_by_user_id,
	scheduled_for, locked_at, locked_by, attempts, last_error, created_at, updated_at`,
		newID("socjob"), input.OrgID, input.PostID, input.IdempotencyKey, input.RequestedByUserID, input.ScheduledFor)
	job, err := scanPublishJob(row)
	if err != nil {
		return nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true
	if post, err := r.getPost(ctx, input.OrgID, input.PostID); err == nil {
		job.Post = post
	}
	return job, nil
}

func (r *PGRepository) GetPublishJob(ctx context.Context, orgID, jobID string) (*PublishJob, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	job, err := r.getPublishJob(ctx, orgID, jobID)
	if err != nil {
		return nil, err
	}
	post, err := r.getPost(ctx, orgID, job.PostID)
	if err == nil {
		job.Post = post
	}
	return job, nil
}

func (r *PGRepository) ClaimDuePublishJobs(ctx context.Context, now time.Time, workerID string, limit int) ([]PublishJob, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	if limit < 1 || limit > 100 {
		limit = 10
	}
	rows, err := r.pool.Query(ctx, `
WITH due AS (
	SELECT id
	FROM social_publish_jobs
	WHERE status = 'queued'
	  AND scheduled_for <= $1
	ORDER BY scheduled_for ASC, created_at ASC
	LIMIT $3
	FOR UPDATE SKIP LOCKED
)
UPDATE social_publish_jobs job
SET status = 'running',
	locked_at = $1,
	locked_by = $2,
	attempts = attempts + 1,
	updated_at = $1
FROM due
WHERE job.id = due.id
RETURNING job.id, job.org_id, job.post_id, job.status, job.idempotency_key, job.requested_by_user_id,
	job.scheduled_for, job.locked_at, job.locked_by, job.attempts, job.last_error, job.created_at, job.updated_at`, now, workerID, limit)
	if err != nil {
		return nil, err
	}
	jobs := []PublishJob{}
	for rows.Next() {
		job, err := scanPublishJob(rows)
		if err != nil {
			rows.Close()
			return nil, err
		}
		jobs = append(jobs, *job)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return nil, err
	}
	rows.Close()

	for idx := range jobs {
		post, err := r.getPost(ctx, jobs[idx].OrgID, jobs[idx].PostID)
		if err != nil {
			return nil, err
		}
		jobs[idx].Post = post
	}
	return jobs, nil
}

func (r *PGRepository) FinishPublishJob(ctx context.Context, input FinishPublishJobInput) (*PublishJob, error) {
	if err := r.ensureConfigured(); err != nil {
		return nil, err
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	job, err := r.getPublishJobTx(ctx, tx, input.OrgID, input.JobID)
	if err != nil {
		return nil, err
	}
	for _, attempt := range input.Attempts {
		if strings.TrimSpace(attempt.ID) == "" {
			attempt.ID = newID("socatt")
		}
		if attempt.AttemptedAt.IsZero() {
			attempt.AttemptedAt = input.CompletedAt
		}
		if attempt.Warnings == nil {
			attempt.Warnings = []string{}
		}
		if attempt.Response == nil {
			attempt.Response = map[string]any{}
		}
		if _, err := tx.Exec(ctx, `
INSERT INTO social_publish_attempts (
	id, org_id, job_id, post_id, provider_key, status, mode, endpoint, external_id,
	message, warnings, response, attempted_at
) VALUES (
	$1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11::jsonb, $12::jsonb, $13
)`, attempt.ID, input.OrgID, input.JobID, job.PostID, normalizePlatform(attempt.ProviderKey),
			attempt.Status, fallback(attempt.Mode, "api"), attempt.Endpoint, attempt.ExternalID,
			attempt.Message, mustJSON(attempt.Warnings), mustJSON(attempt.Response), attempt.AttemptedAt); err != nil {
			return nil, err
		}
	}

	row := tx.QueryRow(ctx, `
UPDATE social_publish_jobs
SET status = $3,
	last_error = $4,
	locked_at = NULL,
	locked_by = '',
	updated_at = $5
WHERE org_id = $1 AND id = $2
RETURNING id, org_id, post_id, status, idempotency_key, requested_by_user_id,
	scheduled_for, locked_at, locked_by, attempts, last_error, created_at, updated_at`,
		input.OrgID, input.JobID, input.Status, input.LastError, input.CompletedAt)
	updated, err := scanPublishJob(row)
	if err != nil {
		return nil, err
	}
	if _, err := tx.Exec(ctx, `
UPDATE social_posts
SET status = $3,
	updated_at = $4
WHERE org_id = $1 AND id = $2`, input.OrgID, job.PostID, postStatusForJobStatus(input.Status), input.CompletedAt); err != nil {
		return nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true
	return updated, nil
}

func (r *PGRepository) ensureConfigured() error {
	if r == nil || r.pool == nil {
		return fmt.Errorf("social repository is not configured")
	}
	return nil
}

func (r *PGRepository) getPost(ctx context.Context, orgID, postID string) (*Post, error) {
	row := r.pool.QueryRow(ctx, `
SELECT id, org_id, title, body, status, platforms, media, source, previews, ai_context,
	approval_required, approval_state, scheduled_at, created_by_user_id, updated_by_user_id,
	created_at, updated_at
FROM social_posts
WHERE org_id = $1 AND id = $2`, orgID, postID)
	post, err := scanPost(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return post, err
}

func (r *PGRepository) getPublishJob(ctx context.Context, orgID, jobID string) (*PublishJob, error) {
	row := r.pool.QueryRow(ctx, `
SELECT id, org_id, post_id, status, idempotency_key, requested_by_user_id,
	scheduled_for, locked_at, locked_by, attempts, last_error, created_at, updated_at
FROM social_publish_jobs
WHERE org_id = $1 AND id = $2`, orgID, jobID)
	job, err := scanPublishJob(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return job, err
}

func (r *PGRepository) getPublishJobTx(ctx context.Context, tx pgx.Tx, orgID, jobID string) (*PublishJob, error) {
	row := tx.QueryRow(ctx, `
SELECT id, org_id, post_id, status, idempotency_key, requested_by_user_id,
	scheduled_for, locked_at, locked_by, attempts, last_error, created_at, updated_at
FROM social_publish_jobs
WHERE org_id = $1 AND id = $2
FOR UPDATE`, orgID, jobID)
	job, err := scanPublishJob(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return job, err
}

func (r *PGRepository) getApprovalTx(ctx context.Context, tx pgx.Tx, orgID, approvalID string) (*Approval, error) {
	row := tx.QueryRow(ctx, `
SELECT id, org_id, post_id, campaign_id, state, requested_by_user_id, requested_of_user_id,
	decided_by_user_id, decision_reason, due_at, decided_at, metadata, created_at, updated_at
FROM social_approvals
WHERE org_id = $1 AND id = $2
FOR UPDATE`, orgID, approvalID)
	approval, err := scanApproval(row)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, ErrNotFound
	}
	return approval, err
}

func scanAccount(row rowScanner) (*Account, error) {
	var account Account
	var capabilitiesRaw []byte
	var metadataRaw []byte
	if err := row.Scan(&account.ID, &account.OrgID, &account.ProviderKey, &account.ConnectionID,
		&account.DisplayName, &account.Handle, &account.Status, &capabilitiesRaw, &account.TokenState,
		&account.TokenExpiresAt, &metadataRaw, &account.CreatedAt, &account.UpdatedAt); err != nil {
		return nil, err
	}
	if err := decodeJSON(capabilitiesRaw, &account.Capabilities); err != nil {
		return nil, err
	}
	if err := decodeJSON(metadataRaw, &account.Metadata); err != nil {
		return nil, err
	}
	account.ProviderKey = normalizePlatform(account.ProviderKey)
	account.Metadata = ensureMap(account.Metadata)
	return &account, nil
}

func scanCampaign(row rowScanner) (*Campaign, error) {
	var campaign Campaign
	var platformsRaw []byte
	var sourceRaw []byte
	var metadataRaw []byte
	if err := row.Scan(&campaign.ID, &campaign.OrgID, &campaign.Name, &campaign.Brief,
		&campaign.Goal, &campaign.Status, &platformsRaw, &campaign.StartsAt, &campaign.EndsAt,
		&sourceRaw, &metadataRaw, &campaign.OwnerUserID, &campaign.CreatedAt,
		&campaign.UpdatedAt); err != nil {
		return nil, err
	}
	if err := decodeJSON(platformsRaw, &campaign.Platforms); err != nil {
		return nil, err
	}
	if err := decodeJSON(sourceRaw, &campaign.Source); err != nil {
		return nil, err
	}
	if err := decodeJSON(metadataRaw, &campaign.Metadata); err != nil {
		return nil, err
	}
	campaign.Platforms = normalizePlatforms(campaign.Platforms)
	campaign.Metadata = ensureMap(campaign.Metadata)
	campaign.Source.Metadata = ensureMap(campaign.Source.Metadata)
	return &campaign, nil
}

func scanPost(row rowScanner) (*Post, error) {
	var post Post
	var platformsRaw []byte
	var mediaRaw []byte
	var sourceRaw []byte
	var previewsRaw []byte
	var aiContextRaw []byte
	if err := row.Scan(&post.ID, &post.OrgID, &post.Title, &post.Body, &post.Status,
		&platformsRaw, &mediaRaw, &sourceRaw, &previewsRaw, &aiContextRaw,
		&post.ApprovalRequired, &post.ApprovalState, &post.ScheduledAt,
		&post.CreatedByUserID, &post.UpdatedByUserID, &post.CreatedAt, &post.UpdatedAt); err != nil {
		return nil, err
	}
	if err := decodeJSON(platformsRaw, &post.Platforms); err != nil {
		return nil, err
	}
	if err := decodeJSON(mediaRaw, &post.Media); err != nil {
		return nil, err
	}
	if err := decodeJSON(sourceRaw, &post.Source); err != nil {
		return nil, err
	}
	if err := decodeJSON(previewsRaw, &post.Previews); err != nil {
		return nil, err
	}
	if err := decodeJSON(aiContextRaw, &post.AIContext); err != nil {
		return nil, err
	}
	post.Platforms = normalizePlatforms(post.Platforms)
	post.AIContext = ensureMap(post.AIContext)
	post.Source.Metadata = ensureMap(post.Source.Metadata)
	return &post, nil
}

func scanApproval(row rowScanner) (*Approval, error) {
	var approval Approval
	var metadataRaw []byte
	if err := row.Scan(&approval.ID, &approval.OrgID, &approval.PostID, &approval.CampaignID,
		&approval.State, &approval.RequestedByUserID, &approval.RequestedOfUserID,
		&approval.DecidedByUserID, &approval.DecisionReason, &approval.DueAt,
		&approval.DecidedAt, &metadataRaw, &approval.CreatedAt, &approval.UpdatedAt); err != nil {
		return nil, err
	}
	if err := decodeJSON(metadataRaw, &approval.Metadata); err != nil {
		return nil, err
	}
	approval.Metadata = ensureMap(approval.Metadata)
	return &approval, nil
}

func scanPublishJob(row rowScanner) (*PublishJob, error) {
	var job PublishJob
	if err := row.Scan(&job.ID, &job.OrgID, &job.PostID, &job.Status, &job.IdempotencyKey,
		&job.RequestedByUserID, &job.ScheduledFor, &job.LockedAt, &job.LockedBy, &job.Attempts,
		&job.LastError, &job.CreatedAt, &job.UpdatedAt); err != nil {
		return nil, err
	}
	return &job, nil
}

func decodeJSON(raw []byte, target any) error {
	if len(raw) == 0 {
		return nil
	}
	return json.Unmarshal(raw, target)
}

func mustJSON(value any) string {
	bytes, err := json.Marshal(value)
	if err != nil {
		return "{}"
	}
	return string(bytes)
}

func fallback(value, defaultValue string) string {
	value = strings.TrimSpace(value)
	if value == "" {
		return defaultValue
	}
	return value
}

func firstNonEmpty(values ...string) string {
	for _, value := range values {
		if strings.TrimSpace(value) != "" {
			return value
		}
	}
	return ""
}

func stableAccountID(orgID, providerKey, connectionID string) string {
	seed := strings.Join([]string{orgID, providerKey, connectionID}, ":")
	if strings.TrimSpace(connectionID) == "" {
		seed = strings.Join([]string{orgID, providerKey}, ":")
	}
	return "socacct_" + strings.ToLower(hexLower(seed))
}

func hexLower(value string) string {
	sum := sha1.Sum([]byte(strings.ToLower(strings.TrimSpace(value))))
	return hex.EncodeToString(sum[:])
}

func postStatusForJobStatus(status string) string {
	switch status {
	case JobStatusCompleted:
		return PostStatusPublished
	case JobStatusBlocked:
		return PostStatusBlocked
	case JobStatusFailed:
		return PostStatusFailed
	case JobStatusCanceled:
		return PostStatusScheduled
	default:
		return PostStatusPublishing
	}
}
