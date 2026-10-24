package llmreviewer

import (
	"encoding/json"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/learning"
)

// The forced tool asks for a reason of the length the learning package keeps,
// and does not make it required: a review without one is still a review.
func TestReviewSchemaAsksForABoundedOptionalReason(t *testing.T) {
	var schema struct {
		Properties struct {
			Skills struct {
				Items struct {
					Properties map[string]struct {
						MaxLength int `json:"maxLength"`
					} `json:"properties"`
					Required []string `json:"required"`
				} `json:"items"`
			} `json:"skills"`
		} `json:"properties"`
	}
	if err := json.Unmarshal([]byte(reviewSchema), &schema); err != nil {
		t.Fatalf("review schema is not JSON: %v", err)
	}
	items := schema.Properties.Skills.Items
	reason, ok := items.Properties["reason"]
	if !ok || reason.MaxLength != learning.MaxReasonChars {
		t.Fatalf("reason property = %+v (present %v), want maxLength %d", reason, ok, learning.MaxReasonChars)
	}
	for _, required := range items.Required {
		if required == "reason" {
			t.Fatal("reason must stay optional")
		}
	}
}
