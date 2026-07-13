package jobs

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

type memoryJobStore struct {
	jobs map[string]*model.Job
}

func (s *memoryJobStore) Create(_ context.Context, job model.Job) (*model.Job, bool, error) {
	if s.jobs == nil {
		s.jobs = make(map[string]*model.Job)
	}
	key := job.OrgID + "/" + job.IdempotencyKey
	if existing := s.jobs[key]; existing != nil {
		copy := *existing
		return &copy, false, nil
	}
	copy := job
	s.jobs[key] = &copy
	s.jobs[job.OrgID+"/"+job.JobID] = &copy
	return &copy, true, nil
}

func (s *memoryJobStore) Get(_ context.Context, orgID, jobID string) (*model.Job, error) {
	job := s.jobs[orgID+"/"+jobID]
	if job == nil {
		return nil, ErrJobNotFound
	}
	copy := *job
	return &copy, nil
}

func (s *memoryJobStore) Start(_ context.Context, orgID, jobID string) (*model.Job, error) {
	job := s.jobs[orgID+"/"+jobID]
	if job == nil || job.Status != model.StatusPending {
		return nil, ErrInvalidJobTransition
	}
	now := time.Now()
	job.Status, job.StartedAt, job.UpdatedAt = model.StatusRunning, &now, now
	copy := *job
	return &copy, nil
}

func (s *memoryJobStore) SetProgress(_ context.Context, orgID, jobID string, progress int) (*model.Job, error) {
	job := s.jobs[orgID+"/"+jobID]
	if job == nil || job.Status != model.StatusRunning || progress < job.Progress || progress > job.Total {
		return nil, ErrInvalidJobTransition
	}
	job.Progress = progress
	copy := *job
	return &copy, nil
}

func (s *memoryJobStore) Complete(_ context.Context, orgID, jobID string, result json.RawMessage) (*model.Job, error) {
	job := s.jobs[orgID+"/"+jobID]
	if job == nil || job.Status != model.StatusRunning {
		return nil, ErrInvalidJobTransition
	}
	now := time.Now()
	job.Status, job.CompletedAt, job.UpdatedAt = model.StatusCompleted, &now, now
	job.Result = append(json.RawMessage(nil), result...)
	copy := *job
	return &copy, nil
}

func (s *memoryJobStore) Fail(_ context.Context, orgID, jobID, message string) (*model.Job, error) {
	job := s.jobs[orgID+"/"+jobID]
	if job == nil || (job.Status != model.StatusPending && job.Status != model.StatusRunning) {
		return nil, ErrInvalidJobTransition
	}
	now := time.Now()
	job.Status, job.CompletedAt, job.UpdatedAt = model.StatusFailed, &now, now
	job.ErrorMessage = &message
	copy := *job
	return &copy, nil
}

type recordingPublisher struct {
	subjects []string
	failAt   int
}

func (p *recordingPublisher) Publish(subject string, _ []byte) error {
	p.subjects = append(p.subjects, subject)
	if p.failAt > 0 && len(p.subjects) == p.failAt {
		return errors.New("publisher unavailable")
	}
	return nil
}

func TestProductionExecutorCannotPublishUnsignedTenantEvents(t *testing.T) {
	executor := NewExecutorWithDependencies(&memoryJobStore{}, disabledEventPublisher{})
	_, _, err := executor.CreateJob(context.Background(), model.CreateJobInput{
		OrgID: "org-a", JobType: model.JobReindex, DocumentIDs: []string{"doc-1"},
	}, "secure-containment")
	if !errors.Is(err, ErrExecutionUnavailable) {
		t.Fatalf("CreateJob error=%v, want ErrExecutionUnavailable", err)
	}
}

func TestExecutorPersistsLifecycleProgressAndResult(t *testing.T) {
	store := &memoryJobStore{}
	publisher := &recordingPublisher{}
	executor := NewExecutorWithDependencies(store, publisher)
	job, created, err := executor.CreateJob(context.Background(), model.CreateJobInput{
		OrgID: "org-a", JobType: model.JobReindex, DocumentIDs: []string{"doc-1", "doc-2"},
	}, "reindex-request-1")
	if err != nil || !created {
		t.Fatalf("CreateJob: created=%v err=%v", created, err)
	}
	if err := executor.Run(context.Background(), *job); err != nil {
		t.Fatalf("Run: %v", err)
	}
	persisted, err := executor.GetJob(context.Background(), "org-a", job.JobID)
	if err != nil {
		t.Fatalf("GetJob: %v", err)
	}
	if persisted.Status != model.StatusCompleted || persisted.Progress != 2 || persisted.StartedAt == nil || persisted.CompletedAt == nil {
		t.Fatalf("persisted job = %+v", persisted)
	}
	var result map[string]any
	if err := json.Unmarshal(persisted.Result, &result); err != nil {
		t.Fatalf("decode result: %v", err)
	}
	if result["published"] != float64(2) {
		t.Fatalf("result = %#v", result)
	}
	if len(publisher.subjects) != 2 {
		t.Fatalf("published subjects = %v", publisher.subjects)
	}
}

func TestExecutorPersistsFailureAndRejectsTerminalReplay(t *testing.T) {
	store := &memoryJobStore{}
	executor := NewExecutorWithDependencies(store, &recordingPublisher{failAt: 1})
	job, _, err := executor.CreateJob(context.Background(), model.CreateJobInput{
		OrgID: "org-a", JobType: model.JobGraphBuild, DocumentIDs: []string{"doc-1"},
	}, "graph-request-1")
	if err != nil {
		t.Fatalf("CreateJob: %v", err)
	}
	if err := executor.Run(context.Background(), *job); err == nil {
		t.Fatal("Run error = nil, want publish failure")
	}
	persisted, err := executor.GetJob(context.Background(), "org-a", job.JobID)
	if err != nil || persisted.Status != model.StatusFailed || persisted.ErrorMessage == nil {
		t.Fatalf("failed job = %+v, err=%v", persisted, err)
	}
	if err := executor.Run(context.Background(), *job); !errors.Is(err, ErrInvalidJobTransition) {
		t.Fatalf("terminal replay error = %v, want ErrInvalidJobTransition", err)
	}
}

func TestExecutorIdempotencyAndTenantLookup(t *testing.T) {
	store := &memoryJobStore{}
	executor := NewExecutorWithDependencies(store, &recordingPublisher{})
	input := model.CreateJobInput{OrgID: "org-a", JobType: model.JobReindex}
	first, firstCreated, err := executor.CreateJob(context.Background(), input, "same-request")
	if err != nil || !firstCreated {
		t.Fatalf("first create: created=%v err=%v", firstCreated, err)
	}
	second, secondCreated, err := executor.CreateJob(context.Background(), input, "same-request")
	if err != nil || secondCreated || second.JobID != first.JobID {
		t.Fatalf("second create = %+v created=%v err=%v", second, secondCreated, err)
	}
	if _, err := executor.GetJob(context.Background(), "org-b", first.JobID); !errors.Is(err, ErrJobNotFound) {
		t.Fatalf("cross-tenant lookup error = %v", err)
	}
}
