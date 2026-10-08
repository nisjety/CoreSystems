package billing

import (
	"context"
	"errors"
	"fmt"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/testfixture"
	"os"
	"sync"
	"testing"
	"time"
)

func TestAllowancePostgresConcurrentUsageIsolationAndReplay(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("disposable billing fixture not configured")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if err = testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "billing_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatal(err)
	}
	if err = database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatal(err)
	}
	repo := NewRepository(db.Pool)
	org := fmt.Sprintf("allowance-%d", time.Now().UnixNano())
	defer func() {
		for _, table := range []string{"billing_allowance_revisions", "billing_allowance_snapshots", "billing_allowance_entries", "billing_allowance_policies", "billing_allowance_pauses", "billing_usage_events", "billing_usage_dedup", "billing_organization_tombstones", "billing_accounts"} {
			_, _ = db.Pool.Exec(context.Background(), "DELETE FROM "+table+" WHERE org_id=$1", org)
		}
		_, _ = db.Pool.Exec(context.Background(), `DELETE FROM billing_retry_jobs WHERE payload->>'org_id'=$1`, org)
	}()
	if err = repo.SaveAccountStateCAS(ctx, NewDefaultAccount(org)); err != nil {
		t.Fatal(err)
	}
	p := allowanceFixture()
	p.StartsAt = time.Now().UTC().Truncate(time.Second).Add(time.Hour)
	p.Metric = "ai.metered.input_tokens"
	p.IncludedUnits = 1000
	if err = repo.ConfigureAllowance(ctx, org, p, time.Now()); err != nil {
		t.Fatal(err)
	}
	if err = NewService(repo, nil, nil).RefreshAllowances(ctx); err != nil {
		t.Fatal("scheduled allowance refresh", err)
	}
	change := AllowanceRevision{Version: "next-plan", EffectiveAt: allowanceBoundary(p, 2), IncludedUnits: 2000, RolloverCap: 100, RolloverCycles: 1, Reason: "Fixture plan upgrade"}
	for i := 0; i < 2; i++ {
		if err = repo.ScheduleAllowanceRevision(ctx, org, p.Metric, change, time.Now()); err != nil {
			t.Fatal("schedule retry", err)
		}
	}
	badChange := change
	badChange.IncludedUnits++
	if err = repo.ScheduleAllowanceRevision(ctx, org, p.Metric, badChange, time.Now()); !errors.Is(err, ErrAllowanceConflict) {
		t.Fatal("revision overwrite accepted", err)
	}
	future, err := repo.Allowances(ctx, org, change.EffectiveAt)
	if err != nil || len(future) != 1 || future[0].Current.PolicyVersion != "next-plan" || future[0].Current.Included != 2000 {
		t.Fatal("scheduled change not loaded", future, err)
	}
	if err = repo.ConfigureAllowance(ctx, org, p, p.StartsAt.Add(time.Hour)); err != nil {
		t.Fatal("retry after activation", err)
	}
	changed := p
	changed.IncludedUnits = 900
	if err = repo.ConfigureAllowance(ctx, org, changed, time.Now()); !errors.Is(err, ErrAllowanceConflict) {
		t.Fatal("immutable policy overwritten", err)
	}
	var wg sync.WaitGroup
	failures := make(chan error, 32)
	for i := 0; i < 32; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			_, err := repo.RecordUsage(ctx, UsageEvent{EventID: fmt.Sprintf("%s-request-%d", org, i%16), OrgID: org, Metric: p.Metric, Quantity: 10, Source: "cost-core", OccurredAt: p.StartsAt.Add(time.Minute)})
			failures <- err
		}(i)
	}
	wg.Wait()
	close(failures)
	for err := range failures {
		if err != nil {
			t.Fatal(err)
		}
	}
	reports, err := repo.Allowances(ctx, org, p.StartsAt.Add(time.Hour))
	if err != nil {
		t.Fatal(err)
	}
	if len(reports) != 1 || reports[0].Current.Used != 160 || reports[0].Current.Remaining != 840 {
		t.Fatalf("double consumption: %+v", reports)
	}
	var invoiceJobs int
	if err = db.Pool.QueryRow(ctx, `SELECT COUNT(*) FROM billing_retry_jobs WHERE payload->>'org_id'=$1`, org).Scan(&invoiceJobs); err != nil || invoiceJobs != 0 {
		t.Fatalf("raw AI counters must not trigger invoice delivery: jobs=%d err=%v", invoiceJobs, err)
	}
	other, err := repo.Allowances(ctx, org+"-other", p.StartsAt.Add(time.Hour))
	if err != nil || len(other) != 0 {
		t.Fatal("tenant data leaked", other, err)
	}
	grant := AllowanceEntry{ID: "manual-credit", Kind: "grant", Units: 100, OccurredAt: p.StartsAt, Reason: "fixture grant"}
	for i := 0; i < 2; i++ {
		if err = repo.GrantAllowance(ctx, org, p.Metric, grant); err != nil {
			t.Fatal(err)
		}
	}
	grant.Units = 200
	if err = repo.GrantAllowance(ctx, org, p.Metric, grant); !errors.Is(err, ErrAllowanceConflict) {
		t.Fatal("grant replay conflict ignored", err)
	}
	reports, err = repo.Allowances(ctx, org, allowanceBoundary(p, 1))
	if err != nil {
		t.Fatal(err)
	}
	if reports[0].Current.Carried != 100 || reports[0].Current.Remaining != 1200 {
		t.Fatalf("purchased credit or rollover lost: %+v", reports[0])
	}
	// Exercise the public quota path, not just the projection: paused accounts
	// must not expose their historical balance as spendable quota.
	activePolicy := p
	activePolicy.Metric = "ai.subscription.requests"
	activePolicy.StartsAt = time.Now().UTC().Add(-2 * time.Hour)
	if err = repo.ConfigureAllowance(ctx, org, activePolicy, time.Now().Add(-3*time.Hour)); err != nil {
		t.Fatal(err)
	}
	service := NewService(repo, nil, nil)
	service.orgServiceURL = "" // Keep this fixture independent of live Org Core.
	quota, err := service.GetQuotaStatus(ctx, org, activePolicy.Metric)
	if err != nil || quota.Remaining != 1000 || quota.IsExceeded {
		t.Fatalf("active quota: %+v %v", quota, err)
	}
	if _, err = db.Pool.Exec(ctx, `UPDATE billing_accounts SET subscription_state='canceled' WHERE org_id=$1`, org); err != nil {
		t.Fatal(err)
	}
	quota, err = service.GetQuotaStatus(ctx, org, activePolicy.Metric)
	if err != nil || quota.Remaining != 0 || !quota.IsExceeded {
		t.Fatalf("paused quota: %+v %v", quota, err)
	}
	if err = repo.TombstoneOrganization(ctx, org, "fixture"); err != nil {
		t.Fatal(err)
	}
	if _, err = repo.Allowances(ctx, org, time.Now()); !errors.Is(err, ErrOrganizationDeleted) {
		t.Fatal("deleted tenant allowed", err)
	}
}
