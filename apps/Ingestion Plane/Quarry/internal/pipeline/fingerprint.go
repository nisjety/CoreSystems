package pipeline

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
)

type FingerprintPipeline struct{}

func NewFingerprintPipeline() *FingerprintPipeline {
	return &FingerprintPipeline{}
}

func (p *FingerprintPipeline) Name() string {
	return "fingerprint"
}

func (p *FingerprintPipeline) Process(ctx context.Context, job *JobContext) error {
	if job == nil {
		return nil
	}
	payload, err := json.Marshal(job.Result)
	if err != nil {
		return err
	}
	sum := sha256.Sum256(payload)
	job.Meta["fingerprint"] = hex.EncodeToString(sum[:])
	return nil
}
