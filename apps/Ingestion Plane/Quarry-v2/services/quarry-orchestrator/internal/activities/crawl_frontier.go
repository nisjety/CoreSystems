package activities

import (
	"context"
	"encoding/json"
	"net/http"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

// frontierBatchMax is the most items one enqueue_batch call takes. It mirrors
// ENQUEUE_BATCH_MAX in quarry-edge's queue_routes.rs.
const frontierBatchMax = 500

// FrontierBatchItem is one child link of a crawl page (WS14.S3).
type FrontierBatchItem struct {
	RequestID string `json:"request_id"`
	URL       string `json:"url"`
	Depth     uint32 `json:"depth"`
}

// FrontierEnqueueBatchInput is the children of one page, for one queue.
type FrontierEnqueueBatchInput struct {
	Queue string
	OrgID string
	Items []FrontierBatchItem
}

// FrontierEnqueueBatch enqueues a page's children (WS14.S3). It sends at most
// 500 items per call, in order, and returns how many the queue took. A
// re-sent item keeps its request id, so the queue does not take it twice.
func (a *Activities) FrontierEnqueueBatch(ctx context.Context, in FrontierEnqueueBatchInput) (uint32, error) {
	const op = "activities.FrontierEnqueueBatch"
	var accepted uint32
	for start := 0; start < len(in.Items); start += frontierBatchMax {
		end := start + frontierBatchMax
		if end > len(in.Items) {
			end = len(in.Items)
		}
		body, err := json.Marshal(map[string]any{"org_id": in.OrgID, "items": in.Items[start:end]})
		if err != nil {
			return accepted, errs.New(errs.CategoryValidation, op, err).Temporal()
		}
		var envelope quarrycontracts.RESTEnvelope[struct {
			Accepted uint32 `json:"accepted"`
		}]
		if err := a.frontierCall(ctx, http.MethodPost, "/v1/internal/queues/"+in.Queue+"/enqueue_batch", in.OrgID, in.Queue, body, &envelope); err != nil {
			return accepted, err
		}
		if envelope.Data != nil {
			accepted += envelope.Data.Accepted
		}
	}
	return accepted, nil
}
