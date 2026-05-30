package pipeline

import (
	"context"
	"fmt"
	"os"
	"strings"
)

// ResultPersister stores pipeline results durably.
type ResultPersister interface {
	PersistResult(ctx context.Context, jobID string, result map[string]any, meta map[string]string) error
}

type StoragePipeline struct {
	persister ResultPersister
}

func NewStoragePipeline() *StoragePipeline {
	return &StoragePipeline{}
}

// NewStoragePipelineWithPersister creates a storage pipeline that actually writes results.
func NewStoragePipelineWithPersister(p ResultPersister) *StoragePipeline {
	return &StoragePipeline{persister: p}
}

func (p *StoragePipeline) Name() string {
	return "storage"
}

func (p *StoragePipeline) Process(ctx context.Context, job *JobContext) error {
	if job == nil {
		return nil
	}
	storageTarget := strings.TrimSpace(os.Getenv("QUARRY_STORAGE_BACKEND"))
	if storageTarget == "" {
		storageTarget = "job-store"
	}
	job.Meta["storageTarget"] = storageTarget

	if p.persister != nil {
		if err := p.persister.PersistResult(ctx, job.JobID, job.Result, job.Meta); err != nil {
			job.Meta["persisted"] = "false"
			return fmt.Errorf("pipeline storage: %w", err)
		}
		job.Meta["persisted"] = "true"
		return nil
	}

	job.Meta["persisted"] = "false"
	return nil
}
