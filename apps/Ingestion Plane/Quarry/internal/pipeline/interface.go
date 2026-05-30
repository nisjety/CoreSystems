package pipeline

import (
	"context"
	"time"
)

type JobContext struct {
	JobID     string
	URL       string
	Mode      string
	Module    string
	StartedAt time.Time
	Result    map[string]any
	Meta      map[string]string
}

type Step interface {
	Name() string
	Process(ctx context.Context, job *JobContext) error
}

type Chain struct {
	steps []Step
}

func NewChain(steps ...Step) *Chain {
	copySteps := make([]Step, 0, len(steps))
	for _, step := range steps {
		if step == nil {
			continue
		}
		copySteps = append(copySteps, step)
	}
	return &Chain{steps: copySteps}
}

func (c *Chain) Run(ctx context.Context, job *JobContext) error {
	if c == nil || job == nil {
		return nil
	}
	if job.Result == nil {
		job.Result = map[string]any{}
	}
	if job.Meta == nil {
		job.Meta = map[string]string{}
	}
	for _, step := range c.steps {
		if err := step.Process(ctx, job); err != nil {
			return err
		}
	}
	return nil
}
