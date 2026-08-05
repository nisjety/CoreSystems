package social

import (
	"context"
	"errors"
	"fmt"
	"log"
	"strings"
	"time"
)

type Service struct {
	repository     Repository
	accountSource  AccountSource
	publisher      Publisher
	eventPublisher EventPublisher
	actionExecutor ActionExecutor
	metricsStore   MetricsStore
	now            func() time.Time
}

type Option func(*Service)

func WithNow(now func() time.Time) Option {
	return func(s *Service) {
		if now != nil {
			s.now = now
		}
	}
}

func WithAccountSource(source AccountSource) Option {
	return func(s *Service) {
		s.accountSource = source
	}
}

func WithPublisher(publisher Publisher) Option {
	return func(s *Service) {
		s.publisher = publisher
	}
}

func WithEventPublisher(publisher EventPublisher) Option {
	return func(s *Service) {
		s.eventPublisher = publisher
	}
}

func WithActionExecutor(executor ActionExecutor) Option {
	return func(s *Service) {
		s.actionExecutor = executor
	}
}

func WithMetricsStore(store MetricsStore) Option {
	return func(s *Service) {
		s.metricsStore = store
	}
}

func NewService(repository Repository, opts ...Option) *Service {
	service := &Service{
		repository: repository,
		now:        time.Now,
	}
	for _, opt := range opts {
		opt(service)
	}
	return service
}

func (s *Service) ListAccounts(ctx context.Context, orgID string) ([]Account, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if s.accountSource != nil {
		accounts, err := s.accountSource.ListSocialAccounts(ctx, orgID)
		if err == nil {
			if upsertErr := s.repository.UpsertAccounts(ctx, orgID, accounts); upsertErr != nil {
				return nil, upsertErr
			}
			s.publish(ctx, SubjectAccountSynced, orgID, "", nil, nil, nil, map[string]any{
				"synced":   len(accounts),
				"accounts": accounts,
			})
		}
	}
	return s.repository.ListAccounts(ctx, orgID)
}

func (s *Service) ListPosts(ctx context.Context, filter ListPostsFilter) ([]Post, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Status = normalizePostStatus(filter.Status)
	filter.Platform = normalizePlatform(filter.Platform)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListPosts(ctx, filter)
}

func (s *Service) ListCampaigns(ctx context.Context, filter ListCampaignsFilter) ([]Campaign, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.Status = normalizeCampaignStatusFilter(filter.Status)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListCampaigns(ctx, filter)
}

func (s *Service) CreateCampaign(ctx context.Context, input CreateCampaignInput) (*Campaign, error) {
	input = normalizeCreateCampaignInput(input)
	if input.OrgID == "" || input.Name == "" {
		return nil, fmt.Errorf("%w: org_id and name are required", ErrInvalidInput)
	}
	if len(input.Platforms) == 0 {
		return nil, fmt.Errorf("%w: at least one platform is required", ErrInvalidInput)
	}
	if !isCampaignStatus(input.Status) {
		return nil, fmt.Errorf("%w: campaign status is invalid", ErrInvalidInput)
	}
	if input.StartsAt != nil && input.EndsAt != nil && input.EndsAt.Before(*input.StartsAt) {
		return nil, fmt.Errorf("%w: ends_at must be after starts_at", ErrInvalidInput)
	}
	campaign, err := s.repository.CreateCampaign(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectCampaignCreated, campaign.OrgID, input.ActorUserID, nil, nil, nil, map[string]any{
		"campaign": campaign,
	})
	return campaign, nil
}

func (s *Service) CreatePost(ctx context.Context, input CreatePostInput) (*Post, error) {
	input = normalizeCreatePostInput(input)
	if input.OrgID == "" || input.Body == "" {
		return nil, fmt.Errorf("%w: org_id and body are required", ErrInvalidInput)
	}
	if len(input.Platforms) == 0 {
		return nil, fmt.Errorf("%w: at least one platform is required", ErrInvalidInput)
	}
	input.AIContext = ensureMap(input.AIContext)
	input.Source.Metadata = ensureMap(input.Source.Metadata)
	input.ApprovalRequired = approvalDefault(input.ApprovalRequired)
	post, err := s.repository.CreatePost(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectPostCreated, post.OrgID, input.ActorUserID, post, nil, nil, nil)
	if post.ApprovalRequired && post.ApprovalState == ApprovalPending {
		s.publish(ctx, SubjectApprovalRequested, post.OrgID, input.ActorUserID, post, nil, nil, map[string]any{
			"approval_state": post.ApprovalState,
		})
	}
	return post, nil
}

func (s *Service) ListApprovals(ctx context.Context, filter ListApprovalsFilter) ([]Approval, error) {
	filter.OrgID = strings.TrimSpace(filter.OrgID)
	filter.State = normalizeApprovalStateFilter(filter.State)
	filter.PostID = strings.TrimSpace(filter.PostID)
	filter.CampaignID = strings.TrimSpace(filter.CampaignID)
	if filter.OrgID == "" {
		return nil, fmt.Errorf("%w: org_id is required", ErrInvalidInput)
	}
	if filter.Limit < 1 || filter.Limit > 100 {
		filter.Limit = 50
	}
	return s.repository.ListApprovals(ctx, filter)
}

func (s *Service) DecideApproval(ctx context.Context, input DecideApprovalInput) (*Approval, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.ApprovalID = strings.TrimSpace(input.ApprovalID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.DecisionReason = strings.TrimSpace(input.DecisionReason)
	input.Decision = normalizeApprovalDecision(input.Decision)
	if input.DecidedAt.IsZero() {
		input.DecidedAt = s.now().UTC()
	} else {
		input.DecidedAt = input.DecidedAt.UTC()
	}
	if input.OrgID == "" || input.ApprovalID == "" || input.Decision == "" {
		return nil, fmt.Errorf("%w: org_id, approval_id, and decision are required", ErrInvalidInput)
	}
	approval, err := s.repository.DecideApproval(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectApprovalDecided, approval.OrgID, input.ActorUserID, approval.Post, nil, nil, map[string]any{
		"approval": approval,
		"decision": input.Decision,
	})
	return approval, nil
}

func (s *Service) SchedulePost(ctx context.Context, input SchedulePostInput) (*ScheduleResult, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.PostID = strings.TrimSpace(input.PostID)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	if input.OrgID == "" || input.PostID == "" || input.ScheduledAt.IsZero() {
		return nil, fmt.Errorf("%w: org_id, post_id, and scheduled_at are required", ErrInvalidInput)
	}
	scheduledAt := input.ScheduledAt.UTC()
	if scheduledAt.Before(s.now().UTC().Add(-1 * time.Minute)) {
		return nil, fmt.Errorf("%w: scheduled_at must not be in the past", ErrInvalidInput)
	}
	input.ScheduledAt = scheduledAt
	if err := s.ensurePublishApproved(ctx, input.OrgID, input.PostID); err != nil {
		return nil, err
	}
	post, err := s.repository.UpdatePostSchedule(ctx, input)
	if err != nil {
		return nil, err
	}
	job, err := s.repository.EnqueuePublishJob(ctx, EnqueuePublishInput{
		OrgID:             input.OrgID,
		PostID:            input.PostID,
		IdempotencyKey:    scheduleIdempotencyKey(input.OrgID, input.PostID, scheduledAt),
		RequestedByUserID: input.ActorUserID,
		ScheduledFor:      scheduledAt,
	})
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectPostScheduled, post.OrgID, input.ActorUserID, post, job, nil, map[string]any{
		"scheduled_at": scheduledAt,
	})
	s.publish(ctx, SubjectPublishJobQueued, job.OrgID, input.ActorUserID, post, job, nil, map[string]any{
		"scheduled_for": job.ScheduledFor,
		"source":        "schedule",
	})
	return &ScheduleResult{Post: post, Job: job}, nil
}

func (s *Service) EnqueuePublish(ctx context.Context, input EnqueuePublishInput) (*PublishJob, error) {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.PostID = strings.TrimSpace(input.PostID)
	input.IdempotencyKey = strings.TrimSpace(input.IdempotencyKey)
	input.RequestedByUserID = strings.TrimSpace(input.RequestedByUserID)
	if input.ScheduledFor.IsZero() {
		input.ScheduledFor = s.now().UTC()
	} else {
		input.ScheduledFor = input.ScheduledFor.UTC()
	}
	if input.OrgID == "" || input.PostID == "" {
		return nil, fmt.Errorf("%w: org_id and post_id are required", ErrInvalidInput)
	}
	if input.IdempotencyKey == "" {
		input.IdempotencyKey = manualIdempotencyKey(input.OrgID, input.PostID, input.RequestedByUserID, input.ScheduledFor)
	}
	if err := s.ensurePublishApproved(ctx, input.OrgID, input.PostID); err != nil {
		return nil, err
	}
	job, err := s.repository.EnqueuePublishJob(ctx, input)
	if err != nil {
		return nil, err
	}
	s.publish(ctx, SubjectPublishJobQueued, job.OrgID, input.RequestedByUserID, job.Post, job, nil, map[string]any{
		"scheduled_for": job.ScheduledFor,
		"source":        "manual",
	})
	return job, nil
}

func (s *Service) GetPublishJob(ctx context.Context, orgID, jobID string) (*PublishJob, error) {
	orgID = strings.TrimSpace(orgID)
	jobID = strings.TrimSpace(jobID)
	if orgID == "" || jobID == "" {
		return nil, fmt.Errorf("%w: org_id and job_id are required", ErrInvalidInput)
	}
	return s.repository.GetPublishJob(ctx, orgID, jobID)
}

func (s *Service) ProcessDuePublishJobs(ctx context.Context, workerID string, limit int) (int, error) {
	workerID = strings.TrimSpace(workerID)
	if workerID == "" {
		workerID = "social-core-worker"
	}
	if limit < 1 || limit > 100 {
		limit = 10
	}
	now := s.now().UTC()
	jobs, err := s.repository.ClaimDuePublishJobs(ctx, now, workerID, limit)
	if err != nil {
		return 0, err
	}
	for _, job := range jobs {
		if err := s.processPublishJob(ctx, job, now); err != nil {
			return 0, err
		}
	}
	return len(jobs), nil
}

func (s *Service) processPublishJob(ctx context.Context, job PublishJob, now time.Time) error {
	if job.Post == nil {
		return s.finishJob(ctx, job, JobStatusFailed, "post payload was not loaded", nil, now)
	}
	// Defense in depth: re-verify approval at execution time. A job could have
	// been queued while approved and then had its approval revoked (or a stale
	// job could exist) — never publish to a real external account for a post
	// that is not currently approved.
	if err := s.ensurePublishApproved(ctx, job.OrgID, job.PostID); err != nil {
		if IsApprovalRequired(err) {
			return s.finishJob(ctx, job, JobStatusBlocked, "post is not approved for publishing", nil, now)
		}
		return err
	}
	accounts, err := s.ListAccounts(ctx, job.OrgID)
	if err != nil {
		return err
	}
	attempts := make([]PublishAttempt, 0, len(job.Post.Platforms))
	connected := connectedAccountsByProvider(accounts)
	for _, platform := range job.Post.Platforms {
		attempt := PublishAttempt{
			OrgID:       job.OrgID,
			JobID:       job.ID,
			PostID:      job.PostID,
			ProviderKey: platform,
			Status:      AttemptStatusBlocked,
			Mode:        "api",
			Warnings:    []string{},
			Response:    map[string]any{},
			AttemptedAt: now,
		}
		if _, ok := connected[platform]; !ok {
			attempt.Message = "No connected social account is available for this provider."
		} else if s.publisher != nil {
			attempt = s.publisher.Publish(ctx, job, *job.Post, connected[platform])
		} else {
			attempt.Message = "Provider publish adapter is not configured."
		}
		attempts = append(attempts, attempt)
	}
	if len(attempts) == 0 {
		return s.finishJob(ctx, job, JobStatusFailed, "post has no target platforms", nil, now)
	}
	status, lastError := finishStatusForAttempts(attempts)
	return s.finishJob(ctx, job, status, lastError, attempts, now)
}

func (s *Service) finishJob(ctx context.Context, job PublishJob, status, lastError string, attempts []PublishAttempt, completedAt time.Time) error {
	finished, err := s.repository.FinishPublishJob(ctx, FinishPublishJobInput{
		OrgID:       job.OrgID,
		JobID:       job.ID,
		Status:      status,
		LastError:   lastError,
		Attempts:    attempts,
		CompletedAt: completedAt,
	})
	if err != nil {
		return err
	}
	subject := publishJobSubject(status)
	s.publish(ctx, subject, job.OrgID, job.RequestedByUserID, job.Post, finished, attempts, map[string]any{
		"completed_at": completedAt,
		"last_error":   lastError,
	})
	return nil
}

func (s *Service) publish(
	ctx context.Context,
	subject string,
	orgID string,
	actorUserID string,
	post *Post,
	job *PublishJob,
	attempts []PublishAttempt,
	data map[string]any,
) {
	if s.eventPublisher == nil {
		return
	}
	payloadData := map[string]any{}
	for key, value := range data {
		payloadData[key] = value
	}
	postID := ""
	if post != nil {
		postID = post.ID
		if orgID == "" {
			orgID = post.OrgID
		}
		payloadData["post"] = post
	}
	jobID := ""
	if job != nil {
		jobID = job.ID
		if postID == "" {
			postID = job.PostID
		}
		if orgID == "" {
			orgID = job.OrgID
		}
		if actorUserID == "" {
			actorUserID = job.RequestedByUserID
		}
		payloadData["publish_job"] = job
	}
	if len(attempts) > 0 {
		payloadData["attempts"] = attempts
	}
	eventType := strings.TrimPrefix(subject, "verevon.application.social.")
	if err := s.eventPublisher.Publish(ctx, subject, LifecycleEvent{
		ID:          newID("evt"),
		Type:        eventType,
		OrgID:       strings.TrimSpace(orgID),
		PostID:      postID,
		JobID:       jobID,
		ActorUserID: strings.TrimSpace(actorUserID),
		Data:        payloadData,
		OccurredAt:  s.now().UTC(),
	}); err != nil {
		log.Printf("social-core: publish %s: %v", subject, err)
	}
}

func publishJobSubject(status string) string {
	switch status {
	case JobStatusCompleted:
		return SubjectPublishJobCompleted
	case JobStatusBlocked:
		return SubjectPublishJobBlocked
	default:
		return SubjectPublishJobFailed
	}
}

func normalizeCreatePostInput(input CreatePostInput) CreatePostInput {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Title = strings.TrimSpace(input.Title)
	input.Body = strings.TrimSpace(input.Body)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.Platforms = normalizePlatforms(input.Platforms)
	if input.Media == nil {
		input.Media = []MediaRef{}
	}
	input.Source.Kind = strings.TrimSpace(input.Source.Kind)
	input.Source.Label = strings.TrimSpace(input.Source.Label)
	input.Source.Href = strings.TrimSpace(input.Source.Href)
	if input.ScheduledAt != nil {
		scheduledAt := input.ScheduledAt.UTC()
		input.ScheduledAt = &scheduledAt
	}
	return input
}

func normalizeCreateCampaignInput(input CreateCampaignInput) CreateCampaignInput {
	input.OrgID = strings.TrimSpace(input.OrgID)
	input.Name = strings.TrimSpace(input.Name)
	input.Brief = strings.TrimSpace(input.Brief)
	input.Goal = strings.TrimSpace(input.Goal)
	input.Status = normalizeCampaignStatus(input.Status)
	input.ActorUserID = strings.TrimSpace(input.ActorUserID)
	input.Platforms = normalizePlatforms(input.Platforms)
	input.Metadata = ensureMap(input.Metadata)
	input.Source.Metadata = ensureMap(input.Source.Metadata)
	input.Source.Kind = strings.TrimSpace(input.Source.Kind)
	input.Source.Label = strings.TrimSpace(input.Source.Label)
	input.Source.Href = strings.TrimSpace(input.Source.Href)
	if input.StartsAt != nil {
		startsAt := input.StartsAt.UTC()
		input.StartsAt = &startsAt
	}
	if input.EndsAt != nil {
		endsAt := input.EndsAt.UTC()
		input.EndsAt = &endsAt
	}
	return input
}

func normalizePlatforms(platforms []string) []string {
	seen := map[string]bool{}
	normalized := []string{}
	for _, platform := range platforms {
		key := normalizePlatform(platform)
		if key == "" || seen[key] {
			continue
		}
		seen[key] = true
		normalized = append(normalized, key)
	}
	return normalized
}

func normalizePlatform(platform string) string {
	switch strings.ToLower(strings.TrimSpace(platform)) {
	case "linkedin", "linked_in":
		return "linkedin"
	case "x", "twitter":
		return "x"
	case "instagram", "ig":
		return "instagram"
	case "facebook", "fb", "facebook-page":
		return "facebook"
	case "tiktok", "tik_tok":
		return "tiktok"
	case "snapchat", "snap":
		return "snapchat"
	default:
		return strings.ToLower(strings.TrimSpace(platform))
	}
}

func normalizePostStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "", "all":
		return ""
	case "queued":
		return PostStatusPublishing
	default:
		return strings.ToLower(strings.TrimSpace(status))
	}
}

func normalizeCampaignStatus(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "", "draft":
		return CampaignStatusDraft
	case "active":
		return CampaignStatusActive
	case "completed", "complete":
		return CampaignStatusCompleted
	case "archived", "archive":
		return CampaignStatusArchived
	default:
		return strings.ToLower(strings.TrimSpace(status))
	}
}

func normalizeCampaignStatusFilter(status string) string {
	switch strings.ToLower(strings.TrimSpace(status)) {
	case "", "all":
		return ""
	default:
		return normalizeCampaignStatus(status)
	}
}

func isCampaignStatus(status string) bool {
	switch status {
	case CampaignStatusDraft, CampaignStatusActive, CampaignStatusCompleted, CampaignStatusArchived:
		return true
	default:
		return false
	}
}

func normalizeApprovalStateFilter(state string) string {
	switch strings.ToLower(strings.TrimSpace(state)) {
	case "", "all":
		return ""
	case "requested":
		return ApprovalPending
	case "approved":
		return ApprovalApproved
	case "rejected":
		return ApprovalRejected
	case "not_requested", "not-required", "not_required":
		return ApprovalNotRequired
	default:
		return strings.ToLower(strings.TrimSpace(state))
	}
}

func normalizeApprovalDecision(decision string) string {
	switch strings.ToLower(strings.TrimSpace(decision)) {
	case "approve", "approved", "grant", "granted":
		return ApprovalApproved
	case "reject", "rejected", "deny", "denied":
		return ApprovalRejected
	default:
		return ""
	}
}

func BuildPreviews(platforms []string, body string, media []MediaRef) []PlatformPreview {
	previews := make([]PlatformPreview, 0, len(platforms))
	for _, platform := range platforms {
		preview := PlatformPreview{
			Platform:       platform,
			Mode:           previewMode(platform),
			Content:        body,
			CharacterLimit: characterLimit(platform),
			Warnings:       previewWarnings(platform, body, media),
			MediaRequired:  mediaRequired(platform),
		}
		previews = append(previews, preview)
	}
	return previews
}

func previewMode(platform string) string {
	switch platform {
	case "x":
		return "short_form"
	case "instagram", "tiktok", "snapchat":
		return "media_first"
	default:
		return "professional"
	}
}

func characterLimit(platform string) int {
	switch platform {
	case "x":
		return 280
	case "linkedin":
		return 3000
	case "instagram":
		return 2200
	case "facebook":
		return 63206
	case "tiktok":
		return 2200
	case "snapchat":
		return 250
	default:
		return 2000
	}
}

func mediaRequired(platform string) bool {
	return platform == "instagram" || platform == "tiktok" || platform == "snapchat"
}

func previewWarnings(platform, body string, media []MediaRef) []string {
	warnings := []string{}
	if len([]rune(body)) > characterLimit(platform) {
		warnings = append(warnings, "content exceeds platform character limit")
	}
	if mediaRequired(platform) && len(media) == 0 {
		warnings = append(warnings, "platform requires at least one media asset")
	}
	if platform == "snapchat" {
		warnings = append(warnings, "Snapchat posts as a Public Profile Story/Spotlight; live posting is allowlist-gated and requires SNAPCHAT_LIVE_PUBLISHING to be enabled")
	}
	return warnings
}

func connectedAccountsByProvider(accounts []Account) map[string]Account {
	connected := map[string]Account{}
	for _, account := range accounts {
		if account.Status == AccountStatusConnected {
			connected[normalizePlatform(account.ProviderKey)] = account
		}
	}
	return connected
}

func finishStatusForAttempts(attempts []PublishAttempt) (string, string) {
	if len(attempts) == 0 {
		return JobStatusFailed, "no publish attempts were created"
	}
	succeeded := 0
	blocked := 0
	failed := 0
	messages := []string{}
	for _, attempt := range attempts {
		switch attempt.Status {
		case AttemptStatusSucceeded:
			succeeded++
		case AttemptStatusBlocked:
			blocked++
		case AttemptStatusFailed:
			failed++
		}
		if strings.TrimSpace(attempt.Message) != "" {
			messages = append(messages, attempt.ProviderKey+": "+attempt.Message)
		}
	}
	if succeeded == len(attempts) {
		return JobStatusCompleted, ""
	}
	if failed > 0 {
		return JobStatusFailed, strings.Join(messages, "; ")
	}
	if blocked > 0 {
		return JobStatusBlocked, strings.Join(messages, "; ")
	}
	return JobStatusFailed, "publish attempts did not complete"
}

func approvalDefault(value *bool) *bool {
	if value != nil {
		return value
	}
	enabled := true
	return &enabled
}

func ensureMap(value map[string]any) map[string]any {
	if value == nil {
		return map[string]any{}
	}
	return value
}

func scheduleIdempotencyKey(orgID, postID string, scheduledAt time.Time) string {
	return strings.Join([]string{"schedule", orgID, postID, scheduledAt.UTC().Format(time.RFC3339)}, ":")
}

func manualIdempotencyKey(orgID, postID, userID string, scheduledFor time.Time) string {
	return strings.Join([]string{"manual", orgID, postID, userID, scheduledFor.UTC().Format(time.RFC3339Nano)}, ":")
}

func IsInvalidInput(err error) bool {
	return errors.Is(err, ErrInvalidInput)
}

func IsApprovalRequired(err error) bool {
	return errors.Is(err, ErrApprovalRequired)
}

// ensurePublishApproved is the server-side enforcement of the "publish with
// approval" contract. A post that requires human approval may only be scheduled
// or published once a genuine approved approval record exists (the same
// social_approvals HITL record DecideApproval writes). Posts created without an
// approval requirement (approval_required = false) pass through. This never
// trusts a client-supplied flag — it re-reads the authoritative record.
func (s *Service) ensurePublishApproved(ctx context.Context, orgID, postID string) error {
	requiresApproval, approved, err := s.repository.PostApprovalStatus(ctx, orgID, postID)
	if err != nil {
		return err
	}
	if requiresApproval && !approved {
		return fmt.Errorf("%w: approve the post before scheduling or publishing", ErrApprovalRequired)
	}
	return nil
}
