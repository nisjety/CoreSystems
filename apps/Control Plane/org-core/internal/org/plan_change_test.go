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

// TestPlanChangeCanTurnEffectiveZDROnWithoutAToggle pins the X-9 premise the
// org-core nats plan-change ZDR signal is built on: a plan change does not
// touch the stored intent (UpdatePlanWithOutbox and UpdatePlanFromBillingSync
// write only plan and plan_revision), so an org whose intent survived a
// downgrade gets effective ZDR back on the upgrade, with no toggle and so no
// interactive-retention outbox row. The nats package emits the ZDR-enabled
// event for exactly this transition (emitZDREnabledOnPlanChange).
func TestPlanChangeCanTurnEffectiveZDROnWithoutAToggle(t *testing.T) {
	const intent = true // set while the org was on pro, kept through the downgrade
	onPro := NewRetentionPosture("pro", intent)
	afterDowngrade := NewRetentionPosture("standard", intent)
	afterUpgrade := NewRetentionPosture("enterprise", intent)

	if !onPro.Effective() {
		t.Fatal("pro with intent is not zero retention")
	}
	if afterDowngrade.Effective() || !afterDowngrade.ZDRIntent {
		t.Fatalf("downgrade posture=%+v; want intent kept and ZDR not in effect", afterDowngrade)
	}
	if !afterUpgrade.Effective() {
		t.Fatalf("upgrade posture=%+v; want ZDR back in effect", afterUpgrade)
	}
	if NewRetentionPosture("enterprise", false).Effective() {
		t.Fatal("an upgrade without stored intent turned ZDR on")
	}
}
