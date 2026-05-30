package pipeline

import (
	"context"
	"encoding/json"
	"strconv"
	"time"
)

type StatsPipeline struct{}

func NewStatsPipeline() *StatsPipeline {
	return &StatsPipeline{}
}

func (p *StatsPipeline) Name() string {
	return "stats"
}

func (p *StatsPipeline) Process(ctx context.Context, job *JobContext) error {
	if job == nil {
		return nil
	}
	if !job.StartedAt.IsZero() {
		durationMs := time.Since(job.StartedAt).Milliseconds()
		job.Meta["durationMs"] = strconv.FormatInt(durationMs, 10)
	}
	payload, err := json.Marshal(job.Result)
	if err != nil {
		return err
	}
	job.Meta["resultBytes"] = strconv.Itoa(len(payload))
	return nil
}
