package org

import (
	"context"
	"strings"
	"testing"
)

func TestPlanChangeBoundariesFailClosedBeforeStorage(t *testing.T) {
	service := &Service{}
	if _, err := service.UpdatePlan(context.Background(), "", "pro", "", ""); err == nil {
		t.Fatal("missing organization id was accepted")
	}
	if _, err := service.UpdatePlan(context.Background(), "org-a", "root", "", ""); err == nil {
		t.Fatal("unknown plan was accepted")
	}
	if _, err := service.FlushPlanChangeOutbox(context.Background(), 10); err == nil || !strings.Contains(err.Error(), "publisher") {
		t.Fatalf("missing plan publisher error=%v", err)
	}

	service = NewService(&Repository{}, &planChangeTestPublisher{})
	if _, err := service.FlushPlanChangeOutbox(context.Background(), 10); err == nil || !strings.Contains(err.Error(), "shared plan event publisher") {
		t.Fatalf("missing shared plan publisher error=%v", err)
	}

	for _, limit := range []int{0, 1001} {
		if _, err := service.FlushPlanChangeOutbox(context.Background(), limit); err == nil || !strings.Contains(err.Error(), "limit") {
			t.Fatalf("outbox limit %d error=%v; want validation failure", limit, err)
		}
	}
}
