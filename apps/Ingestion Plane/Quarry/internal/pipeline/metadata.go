package pipeline

import (
	"context"
	"time"
)

type MetadataPipeline struct{}

func NewMetadataPipeline() *MetadataPipeline {
	return &MetadataPipeline{}
}

func (p *MetadataPipeline) Name() string {
	return "metadata"
}

func (p *MetadataPipeline) Process(ctx context.Context, job *JobContext) error {
	if job == nil {
		return nil
	}
	now := time.Now().UTC()
	job.Meta["jobId"] = job.JobID
	job.Meta["url"] = job.URL
	job.Meta["mode"] = job.Mode
	job.Meta["module"] = job.Module
	job.Meta["processedAt"] = now.Format(time.RFC3339)
	if !job.StartedAt.IsZero() {
		job.Meta["startedAt"] = job.StartedAt.UTC().Format(time.RFC3339)
	}
	return nil
}
