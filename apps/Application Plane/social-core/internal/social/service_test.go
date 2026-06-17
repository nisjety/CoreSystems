package social

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

type fakeRepository struct {
	accounts        []Account
	approvals       map[string]*Approval
	campaigns       map[string]*Campaign
	posts           map[string]*Post
	queuedJobs      []EnqueuePublishInput
	claimedJobs     []PublishJob
	finishedJob     *FinishPublishJobInput
	nextApprovalID  string
	nextCampaignID  string
	nextPostID      string
	nextJobID       string
	lastSchedule    *SchedulePostInput
	lastCreateInput *CreatePostInput
}

func newFakeRepository() *fakeRepository {
	return &fakeRepository{
		approvals:      map[string]*Approval{},
		campaigns:      map[string]*Campaign{},
		posts:          map[string]*Post{},
		nextApprovalID: "socapr_test",
		nextCampaignID: "soccamp_test",
		nextPostID:     "socpost_test",
		nextJobID:      "socjob_test",
	}
}

func (r *fakeRepository) ListAccounts(ctx context.Context, orgID string) ([]Account, error) {
	return r.accounts, nil
}

func (r *fakeRepository) UpsertAccounts(ctx context.Context, orgID string, accounts []Account) error {
	r.accounts = append([]Account{}, accounts...)
	return nil
}

func (r *fakeRepository) ListCampaigns(ctx context.Context, filter ListCampaignsFilter) ([]Campaign, error) {
	campaigns := []Campaign{}
	for _, campaign := range r.campaigns {
		if campaign.OrgID != filter.OrgID {
			continue
		}
		if filter.Status != "" && campaign.Status != filter.Status {
			continue
		}
		campaigns = append(campaigns, *campaign)
	}
	return campaigns, nil
}

func (r *fakeRepository) CreateCampaign(ctx context.Context, input CreateCampaignInput) (*Campaign, error) {
	campaign := &Campaign{
		ID:          r.nextCampaignID,
		OrgID:       input.OrgID,
		Name:        input.Name,
		Brief:       input.Brief,
		Goal:        input.Goal,
		Status:      input.Status,
		Platforms:   input.Platforms,
		StartsAt:    input.StartsAt,
		EndsAt:      input.EndsAt,
		Source:      input.Source,
		Metadata:    input.Metadata,
		OwnerUserID: input.ActorUserID,
	}
	r.campaigns[campaign.ID] = campaign
	return campaign, nil
}

func (r *fakeRepository) ListPosts(ctx context.Context, filter ListPostsFilter) ([]Post, error) {
	posts := []Post{}
	for _, post := range r.posts {
		if post.OrgID == filter.OrgID {
			posts = append(posts, *post)
		}
	}
	return posts, nil
}

func (r *fakeRepository) CreatePost(ctx context.Context, input CreatePostInput) (*Post, error) {
	r.lastCreateInput = &input
	status := PostStatusDraft
	if input.ScheduledAt != nil {
		status = PostStatusScheduled
	}
	approvalRequired := true
	if input.ApprovalRequired != nil {
		approvalRequired = *input.ApprovalRequired
	}
	approvalState := ApprovalPending
	if !approvalRequired {
		approvalState = ApprovalNotRequired
	}
	post := &Post{
		ID:               r.nextPostID,
		OrgID:            input.OrgID,
		Title:            input.Title,
		Body:             input.Body,
		Status:           status,
		Platforms:        input.Platforms,
		Media:            input.Media,
		Source:           input.Source,
		Previews:         BuildPreviews(input.Platforms, input.Body, input.Media),
		AIContext:        input.AIContext,
		ApprovalRequired: approvalRequired,
		ApprovalState:    approvalState,
		ScheduledAt:      input.ScheduledAt,
		CreatedByUserID:  input.ActorUserID,
		UpdatedByUserID:  input.ActorUserID,
	}
	r.posts[post.ID] = post
	if approvalRequired {
		approval := &Approval{
			ID:                r.nextApprovalID,
			OrgID:             input.OrgID,
			PostID:            post.ID,
			State:             ApprovalPending,
			RequestedByUserID: input.ActorUserID,
			Metadata:          map[string]any{"source": "post_create"},
			Post:              post,
		}
		r.approvals[approval.ID] = approval
	}
	return post, nil
}

func (r *fakeRepository) ListApprovals(ctx context.Context, filter ListApprovalsFilter) ([]Approval, error) {
	approvals := []Approval{}
	for _, approval := range r.approvals {
		if approval.OrgID != filter.OrgID {
			continue
		}
		if filter.State != "" && approval.State != filter.State {
			continue
		}
		if filter.PostID != "" && approval.PostID != filter.PostID {
			continue
		}
		if filter.CampaignID != "" && approval.CampaignID != filter.CampaignID {
			continue
		}
		next := *approval
		if next.PostID != "" {
			next.Post = r.posts[next.PostID]
		}
		approvals = append(approvals, next)
	}
	return approvals, nil
}

func (r *fakeRepository) DecideApproval(ctx context.Context, input DecideApprovalInput) (*Approval, error) {
	approval := r.approvals[input.ApprovalID]
	if approval == nil || approval.OrgID != input.OrgID {
		return nil, ErrNotFound
	}
	if approval.State != ApprovalPending {
		return nil, ErrInvalidInput
	}
	next := *approval
	next.State = input.Decision
	next.DecisionReason = input.DecisionReason
	next.DecidedByUserID = input.ActorUserID
	next.DecidedAt = &input.DecidedAt
	if post := r.posts[next.PostID]; post != nil {
		updatedPost := *post
		updatedPost.ApprovalState = input.Decision
		updatedPost.UpdatedByUserID = input.ActorUserID
		r.posts[post.ID] = &updatedPost
		next.Post = &updatedPost
	}
	r.approvals[input.ApprovalID] = &next
	return &next, nil
}

func (r *fakeRepository) UpdatePostSchedule(ctx context.Context, input SchedulePostInput) (*Post, error) {
	r.lastSchedule = &input
	post := r.posts[input.PostID]
	if post == nil {
		return nil, ErrNotFound
	}
	next := *post
	next.Status = PostStatusScheduled
	next.ScheduledAt = &input.ScheduledAt
	next.UpdatedByUserID = input.ActorUserID
	r.posts[input.PostID] = &next
	return &next, nil
}

func (r *fakeRepository) EnqueuePublishJob(ctx context.Context, input EnqueuePublishInput) (*PublishJob, error) {
	r.queuedJobs = append(r.queuedJobs, input)
	return &PublishJob{
		ID:                r.nextJobID,
		OrgID:             input.OrgID,
		PostID:            input.PostID,
		Status:            JobStatusQueued,
		IdempotencyKey:    input.IdempotencyKey,
		RequestedByUserID: input.RequestedByUserID,
		ScheduledFor:      input.ScheduledFor,
	}, nil
}

func (r *fakeRepository) GetPublishJob(ctx context.Context, orgID, jobID string) (*PublishJob, error) {
	return nil, ErrNotFound
}

func (r *fakeRepository) ClaimDuePublishJobs(ctx context.Context, now time.Time, workerID string, limit int) ([]PublishJob, error) {
	return r.claimedJobs, nil
}

func (r *fakeRepository) FinishPublishJob(ctx context.Context, input FinishPublishJobInput) (*PublishJob, error) {
	r.finishedJob = &input
	return &PublishJob{ID: input.JobID, OrgID: input.OrgID, Status: input.Status, LastError: input.LastError}, nil
}

type fakeAccountSource struct {
	accounts []Account
}

func (s fakeAccountSource) ListSocialAccounts(ctx context.Context, orgID string) ([]Account, error) {
	return s.accounts, nil
}

type fakeTokenBroker struct {
	request TokenRequest
	token   *TokenLease
	err     error
}

func (b *fakeTokenBroker) AccessToken(ctx context.Context, request TokenRequest) (*TokenLease, error) {
	b.request = request
	return b.token, b.err
}

type publishedEvent struct {
	subject string
	payload any
}

type fakeEventPublisher struct {
	events []publishedEvent
	err    error
}

func (p *fakeEventPublisher) Publish(ctx context.Context, subject string, payload any) error {
	p.events = append(p.events, publishedEvent{subject: subject, payload: payload})
	return p.err
}

func TestCreatePostNormalizesPlatformsAndBuildsPreviews(t *testing.T) {
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithEventPublisher(events))

	post, err := service.CreatePost(context.Background(), CreatePostInput{
		OrgID:       "org_1",
		Title:       "Launch",
		Body:        "New release today",
		Platforms:   []string{"LinkedIn", "twitter", "x", "instagram"},
		AIContext:   map[string]any{"agent_run_id": "run_1"},
		ActorUserID: "user_1",
	})
	if err != nil {
		t.Fatalf("CreatePost returned error: %v", err)
	}
	if got, want := post.Platforms, []string{"linkedin", "x", "instagram"}; !equalStrings(got, want) {
		t.Fatalf("platforms = %#v, want %#v", got, want)
	}
	if len(post.Previews) != 3 {
		t.Fatalf("preview count = %d, want 3", len(post.Previews))
	}
	if len(post.Previews[2].Warnings) != 1 {
		t.Fatalf("instagram preview warnings = %#v, want media warning", post.Previews[2].Warnings)
	}
	if repo.lastCreateInput == nil || repo.lastCreateInput.Media == nil {
		t.Fatalf("media was not normalized to an empty slice before persistence")
	}
	if got, want := events.events[0].subject, SubjectPostCreated; got != want {
		t.Fatalf("event subject = %s, want %s", got, want)
	}
	event, ok := events.events[0].payload.(LifecycleEvent)
	if !ok {
		t.Fatalf("event payload = %T, want LifecycleEvent", events.events[0].payload)
	}
	if event.OrgID != "org_1" || event.PostID != post.ID || event.ActorUserID != "user_1" {
		t.Fatalf("event scope = %#v, want org/post/actor", event)
	}
}

func TestListAccountsSyncsIntegrationAccounts(t *testing.T) {
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithAccountSource(fakeAccountSource{accounts: []Account{{
		ID:           "acct_1",
		OrgID:        "org_1",
		ProviderKey:  "x",
		ConnectionID: "conn_1",
		DisplayName:  "Brand account",
		Status:       AccountStatusConnected,
		TokenState:   "available",
	}}}), WithEventPublisher(events))

	accounts, err := service.ListAccounts(context.Background(), "org_1")
	if err != nil {
		t.Fatalf("ListAccounts returned error: %v", err)
	}
	if len(accounts) != 1 {
		t.Fatalf("accounts = %d, want 1", len(accounts))
	}
	if accounts[0].ConnectionID != "conn_1" {
		t.Fatalf("connection id = %q, want conn_1", accounts[0].ConnectionID)
	}
	if got, want := events.events[0].subject, SubjectAccountSynced; got != want {
		t.Fatalf("event subject = %s, want %s", got, want)
	}
}

func TestCreatePostCreatesPendingApprovalWhenRequired(t *testing.T) {
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithEventPublisher(events))

	post, err := service.CreatePost(context.Background(), CreatePostInput{
		OrgID:       "org_1",
		Title:       "Approval post",
		Body:        "Needs human review",
		Platforms:   []string{"linkedin"},
		ActorUserID: "user_1",
	})
	if err != nil {
		t.Fatalf("CreatePost returned error: %v", err)
	}
	approvals, err := service.ListApprovals(context.Background(), ListApprovalsFilter{
		OrgID: "org_1",
		State: "requested",
	})
	if err != nil {
		t.Fatalf("ListApprovals returned error: %v", err)
	}
	if len(approvals) != 1 {
		t.Fatalf("approvals = %d, want 1", len(approvals))
	}
	if approvals[0].PostID != post.ID || approvals[0].State != ApprovalPending {
		t.Fatalf("approval = %#v, want pending approval for post", approvals[0])
	}
	if approvals[0].Post == nil || approvals[0].Post.ID != post.ID {
		t.Fatalf("approval post = %#v, want embedded post", approvals[0].Post)
	}
	if got, want := events.events[1].subject, SubjectApprovalRequested; got != want {
		t.Fatalf("second event subject = %s, want %s", got, want)
	}
}

func TestDecideApprovalUpdatesPostApprovalStateAndPublishesEvent(t *testing.T) {
	now := time.Date(2026, 6, 15, 12, 0, 0, 0, time.UTC)
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithNow(func() time.Time { return now }), WithEventPublisher(events))
	post, err := service.CreatePost(context.Background(), CreatePostInput{
		OrgID:       "org_1",
		Body:        "Approve this",
		Platforms:   []string{"linkedin"},
		ActorUserID: "requester_1",
	})
	if err != nil {
		t.Fatalf("CreatePost returned error: %v", err)
	}

	approval, err := service.DecideApproval(context.Background(), DecideApprovalInput{
		OrgID:          "org_1",
		ApprovalID:     "socapr_test",
		Decision:       "approve",
		DecisionReason: "Looks good",
		ActorUserID:    "approver_1",
	})
	if err != nil {
		t.Fatalf("DecideApproval returned error: %v", err)
	}
	if approval.State != ApprovalApproved {
		t.Fatalf("approval state = %s, want approved", approval.State)
	}
	if approval.DecidedAt == nil || !approval.DecidedAt.Equal(now) {
		t.Fatalf("decided_at = %v, want %v", approval.DecidedAt, now)
	}
	updated := repo.posts[post.ID]
	if updated == nil || updated.ApprovalState != ApprovalApproved || updated.UpdatedByUserID != "approver_1" {
		t.Fatalf("updated post = %#v, want approved by approver", updated)
	}
	if got, want := events.events[len(events.events)-1].subject, SubjectApprovalDecided; got != want {
		t.Fatalf("last event subject = %s, want %s", got, want)
	}
}

func TestCreateCampaignNormalizesPlatformsAndPublishesEvent(t *testing.T) {
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithEventPublisher(events))

	campaign, err := service.CreateCampaign(context.Background(), CreateCampaignInput{
		OrgID:       " org_1 ",
		Name:        " Launch ",
		Brief:       "Campaign brief",
		Platforms:   []string{"LinkedIn", "twitter", "x"},
		ActorUserID: "owner_1",
	})
	if err != nil {
		t.Fatalf("CreateCampaign returned error: %v", err)
	}
	if campaign.OrgID != "org_1" || campaign.Name != "Launch" {
		t.Fatalf("campaign scope/name = %#v, want trimmed values", campaign)
	}
	if campaign.Status != CampaignStatusDraft {
		t.Fatalf("campaign status = %s, want draft", campaign.Status)
	}
	if got, want := campaign.Platforms, []string{"linkedin", "x"}; !equalStrings(got, want) {
		t.Fatalf("platforms = %#v, want %#v", got, want)
	}
	if campaign.Metadata == nil {
		t.Fatalf("metadata = nil, want empty map")
	}
	if got, want := events.events[0].subject, SubjectCampaignCreated; got != want {
		t.Fatalf("event subject = %s, want %s", got, want)
	}
}

func TestSchedulePostPersistsScheduleAndQueuesPublishJob(t *testing.T) {
	now := time.Date(2026, 6, 15, 12, 0, 0, 0, time.UTC)
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithNow(func() time.Time { return now }), WithEventPublisher(events))
	post, err := repo.CreatePost(context.Background(), CreatePostInput{
		OrgID:     "org_1",
		Body:      "Scheduled copy",
		Platforms: []string{"linkedin"},
	})
	if err != nil {
		t.Fatalf("seed post: %v", err)
	}

	scheduledAt := now.Add(2 * time.Hour)
	result, err := service.SchedulePost(context.Background(), SchedulePostInput{
		OrgID:       "org_1",
		PostID:      post.ID,
		ScheduledAt: scheduledAt,
		ActorUserID: "user_1",
	})
	if err != nil {
		t.Fatalf("SchedulePost returned error: %v", err)
	}
	if result.Post.Status != PostStatusScheduled {
		t.Fatalf("post status = %s, want scheduled", result.Post.Status)
	}
	if len(repo.queuedJobs) != 1 {
		t.Fatalf("queued jobs = %d, want 1", len(repo.queuedJobs))
	}
	if repo.queuedJobs[0].ScheduledFor != scheduledAt {
		t.Fatalf("queued scheduled_for = %s, want %s", repo.queuedJobs[0].ScheduledFor, scheduledAt)
	}
	if result.Job.IdempotencyKey == "" {
		t.Fatal("schedule job idempotency key was empty")
	}
	if got, want := eventSubjects(events.events), []string{SubjectPostScheduled, SubjectPublishJobQueued}; !equalStrings(got, want) {
		t.Fatalf("event subjects = %#v, want %#v", got, want)
	}
}

func TestSchedulePostRejectsPastTimes(t *testing.T) {
	now := time.Date(2026, 6, 15, 12, 0, 0, 0, time.UTC)
	service := NewService(newFakeRepository(), WithNow(func() time.Time { return now }))

	_, err := service.SchedulePost(context.Background(), SchedulePostInput{
		OrgID:       "org_1",
		PostID:      "post_1",
		ScheduledAt: now.Add(-10 * time.Minute),
	})
	if !errors.Is(err, ErrInvalidInput) {
		t.Fatalf("error = %v, want ErrInvalidInput", err)
	}
}

func TestProcessDuePublishJobsBlocksWhenAccountsAreMissing(t *testing.T) {
	now := time.Date(2026, 6, 15, 12, 0, 0, 0, time.UTC)
	repo := newFakeRepository()
	events := &fakeEventPublisher{}
	service := NewService(repo, WithNow(func() time.Time { return now }), WithEventPublisher(events))
	repo.claimedJobs = []PublishJob{{
		ID:     "job_1",
		OrgID:  "org_1",
		PostID: "post_1",
		Status: JobStatusRunning,
		Post: &Post{
			ID:        "post_1",
			OrgID:     "org_1",
			Body:      "Publish me",
			Platforms: []string{"linkedin", "x"},
		},
	}}

	processed, err := service.ProcessDuePublishJobs(context.Background(), "worker_1", 10)
	if err != nil {
		t.Fatalf("ProcessDuePublishJobs returned error: %v", err)
	}
	if processed != 1 {
		t.Fatalf("processed = %d, want 1", processed)
	}
	if repo.finishedJob == nil {
		t.Fatal("FinishPublishJob was not called")
	}
	if repo.finishedJob.Status != JobStatusBlocked {
		t.Fatalf("finished status = %s, want blocked", repo.finishedJob.Status)
	}
	if len(repo.finishedJob.Attempts) != 2 {
		t.Fatalf("attempts = %d, want 2", len(repo.finishedJob.Attempts))
	}
	if got, want := events.events[0].subject, SubjectPublishJobBlocked; got != want {
		t.Fatalf("event subject = %s, want %s", got, want)
	}
}

func TestHTTPPublisherPostsXWithLeasedToken(t *testing.T) {
	var gotAuth string
	var gotPayload map[string]any
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/2/tweets" {
			t.Fatalf("path = %s, want /2/tweets", r.URL.Path)
		}
		gotAuth = r.Header.Get("Authorization")
		if err := json.NewDecoder(r.Body).Decode(&gotPayload); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"data":{"id":"tweet_1"}}`))
	}))
	defer server.Close()

	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, server.Client(), PublisherConfig{XAPIBaseURL: server.URL})
	attempt := publisher.Publish(context.Background(), PublishJob{
		ID:     "job_1",
		OrgID:  "org_1",
		PostID: "post_1",
	}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "Fallback copy",
		Platforms: []string{"x"},
		Previews:  []PlatformPreview{{Platform: "x", Content: "Native X copy"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "x",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write"},
	})

	if attempt.Status != AttemptStatusSucceeded {
		t.Fatalf("attempt status = %s, want succeeded: %#v", attempt.Status, attempt)
	}
	if attempt.ExternalID != "tweet_1" {
		t.Fatalf("external id = %q, want tweet_1", attempt.ExternalID)
	}
	if gotAuth != "Bearer leased-token" {
		t.Fatalf("Authorization = %q, want leased token", gotAuth)
	}
	if gotPayload["text"] != "Native X copy" {
		t.Fatalf("text payload = %#v, want Native X copy", gotPayload["text"])
	}
	if broker.request.ConnectionID != "conn_1" || broker.request.Consumer != socialPublisherConsumer {
		t.Fatalf("token request = %#v, want connection lease for social publisher", broker.request)
	}
}

func TestHTTPPublisherBlocksWithoutPublishCapability(t *testing.T) {
	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, nil, PublisherConfig{})

	attempt := publisher.Publish(context.Background(), PublishJob{
		ID:     "job_1",
		OrgID:  "org_1",
		PostID: "post_1",
	}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "Publish me",
		Platforms: []string{"snapchat"},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "snapchat",
		ConnectionID: "conn_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.ads.manage"},
	})

	if attempt.Status != AttemptStatusBlocked {
		t.Fatalf("attempt status = %s, want blocked", attempt.Status)
	}
	if broker.request.ConnectionID != "" {
		t.Fatalf("token broker should not be called, got %#v", broker.request)
	}
}

func TestHTTPPublisherPostsFacebookPageFeedWithLeasedToken(t *testing.T) {
	var gotAuth string
	var gotBody string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/page_1/feed" {
			t.Fatalf("path = %s, want /page_1/feed", r.URL.Path)
		}
		gotAuth = r.Header.Get("Authorization")
		data, err := io.ReadAll(r.Body)
		if err != nil {
			t.Fatalf("read request: %v", err)
		}
		gotBody = string(data)
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"id":"page_1_post_1"}`))
	}))
	defer server.Close()

	broker := &fakeTokenBroker{token: &TokenLease{AccessToken: "leased-token"}}
	publisher := NewHTTPPublisher(broker, server.Client(), PublisherConfig{FacebookGraphAPIBaseURL: server.URL})
	attempt := publisher.Publish(context.Background(), PublishJob{
		ID:     "job_1",
		OrgID:  "org_1",
		PostID: "post_1",
	}, Post{
		ID:        "post_1",
		OrgID:     "org_1",
		Body:      "Facebook copy",
		Platforms: []string{"facebook"},
		Previews:  []PlatformPreview{{Platform: "facebook", Content: "Native Facebook copy"}},
	}, Account{
		OrgID:        "org_1",
		ProviderKey:  "facebook",
		ConnectionID: "conn_1",
		Handle:       "page_1",
		Status:       AccountStatusConnected,
		Capabilities: []string{"social.post.write"},
	})

	if attempt.Status != AttemptStatusSucceeded {
		t.Fatalf("attempt status = %s, want succeeded: %#v", attempt.Status, attempt)
	}
	if attempt.ExternalID != "page_1_post_1" {
		t.Fatalf("external id = %q, want page_1_post_1", attempt.ExternalID)
	}
	if gotAuth != "Bearer leased-token" {
		t.Fatalf("Authorization = %q, want leased token", gotAuth)
	}
	if !strings.Contains(gotBody, "message=Native+Facebook+copy") {
		t.Fatalf("request body = %q, want encoded Facebook message", gotBody)
	}
}

func equalStrings(left, right []string) bool {
	if len(left) != len(right) {
		return false
	}
	for idx := range left {
		if left[idx] != right[idx] {
			return false
		}
	}
	return true
}

func eventSubjects(events []publishedEvent) []string {
	subjects := make([]string, 0, len(events))
	for _, event := range events {
		subjects = append(subjects, event.subject)
	}
	return subjects
}
