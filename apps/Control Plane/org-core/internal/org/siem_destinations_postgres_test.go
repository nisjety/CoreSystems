package org

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/testfixture"
	"github.com/jackc/pgx/v5"
)

const (
	siemPGSecret = "jjjjjjjjjjjjjjjjjjjjjjjjjjjj"
	siemPGQuery  = "kkkkkkkkkkkkkkkkkkkkkkkkkkkkkkk"
)

type recordingAuditPublisher struct {
	mu       sync.Mutex
	subjects []string
	events   []map[string]any
	err      error
}

func (p *recordingAuditPublisher) PublishAudit(_ context.Context, subject, _ string, payload map[string]any) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.err != nil {
		return p.err
	}
	p.subjects = append(p.subjects, subject)
	p.events = append(p.events, payload)
	return nil
}

// siemPostgres opens the disposable lifecycle database the other org-core
// Postgres tests use, applies the migrations and seeds two organisations: one
// on the enterprise plan, one on pro.
func siemPostgres(t *testing.T) (context.Context, *database.DB, *Repository, *Service, *recordingAuditPublisher) {
	t.Helper()
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	t.Cleanup(cancel)
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	t.Cleanup(db.Close)
	if err := testfixture.VerifyLifecycleMarker(ctx, db.Pool, dsn, "org_lifecycle", os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID")); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply org-core migrations: %v", err)
	}

	repo := NewRepository(db)
	plans := map[string]string{"org-siem-enterprise": "enterprise", "org-siem-other": "enterprise", "org-siem-pro": "pro"}
	for orgID, plan := range plans {
		if err := repo.ProvisionOrganizationWithOwner(ctx, Organization{ID: orgID, Name: orgID, Plan: plan, Status: "active"}, "owner-"+orgID); err != nil {
			t.Fatalf("seed %s: %v", orgID, err)
		}
		// Reset what a previous test left behind: rows, and a plan it lapsed.
		if _, err := db.Pool.Exec(ctx, `DELETE FROM siem_destinations WHERE org_id = $1`, orgID); err != nil {
			t.Fatalf("reset destinations: %v", err)
		}
		if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET plan = $2 WHERE id = $1`, orgID, plan); err != nil {
			t.Fatalf("reset plan: %v", err)
		}
	}
	if _, err := db.Pool.Exec(ctx, `DELETE FROM organization_siem_audit_outbox WHERE org_id LIKE 'org-siem-%'`); err != nil {
		t.Fatalf("reset outbox: %v", err)
	}

	publisher := &recordingAuditPublisher{}
	service := NewService(repo, nil)
	service.EnableSIEMDestinations(mustSealer(t, "pg-v1", 0x44))
	service.SetAuditPublisher(publisher)
	return ctx, db, repo, service, publisher
}

func siemCreate(t *testing.T, ctx context.Context, service *Service, orgID string, secret string) *SIEMDestination {
	t.Helper()
	created, err := service.CreateSIEMDestination(ctx, orgID, "u-admin", "admin", CreateSIEMDestinationRequest{
		Kind: SIEMKindNDJSONWebhook, URL: "https://siem.example.com/ingest?" + siemPGQuery, Secret: secret,
	})
	if err != nil {
		t.Fatalf("create: %v", err)
	}
	return created
}

func TestControlLifecycleSIEMSecretIsStoredSealedAndAuditedWithoutContent(t *testing.T) {
	ctx, db, _, service, publisher := siemPostgres(t)
	created := siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)
	if !created.SecretSet || created.Revision != 1 || !created.Enabled {
		t.Fatalf("created = %+v", created)
	}

	var ciphertext []byte
	var keyID string
	if err := db.Pool.QueryRow(ctx, `SELECT secret_ciphertext, secret_key_id FROM siem_destinations WHERE id = $1`, created.ID).Scan(&ciphertext, &keyID); err != nil {
		t.Fatalf("read row: %v", err)
	}
	if keyID != "pg-v1" || strings.Contains(string(ciphertext), siemPGSecret) {
		t.Fatalf("row holds key id %q and ciphertext containing the secret: %v", keyID, strings.Contains(string(ciphertext), siemPGSecret))
	}
	// psql would show only ciphertext: no column of the row carries the secret.
	var dump string
	if err := db.Pool.QueryRow(ctx, `SELECT row_to_json(d)::text FROM siem_destinations d WHERE id = $1`, created.ID).Scan(&dump); err != nil {
		t.Fatalf("dump row: %v", err)
	}
	if strings.Contains(dump, siemPGSecret) {
		t.Fatalf("the row carries the secret in the clear: %s", dump)
	}

	// Update rotates the secret, bumps the revision and audits it.
	rotated := "pg-fixture-rotated-0123456789"
	updated, err := service.UpdateSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u-owner", "owner", UpdateSIEMDestinationRequest{ExpectedRevision: 1, Secret: &rotated})
	if err != nil || updated.Revision != 2 {
		t.Fatalf("update = %+v, %v", updated, err)
	}
	if err := service.DeleteSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u-owner", "owner"); err != nil {
		t.Fatalf("delete: %v", err)
	}

	result, err := service.FlushSIEMAuditOutbox(ctx, 100)
	if err != nil || result.Published != 3 || result.DeadLettered != 0 {
		t.Fatalf("flush = %+v, %v; want 3 published (created, updated, deleted)", result, err)
	}
	if len(publisher.events) != 3 {
		t.Fatalf("published %d events", len(publisher.events))
	}
	actions := []string{}
	for index, event := range publisher.events {
		if publisher.subjects[index] != SIEMAuditSubject {
			t.Fatalf("subject = %q", publisher.subjects[index])
		}
		encoded, _ := json.Marshal(event)
		for _, leaked := range []string{siemPGSecret, rotated, "QUERY-VALUE", "/ingest", "token="} {
			if strings.Contains(string(encoded), leaked) {
				t.Fatalf("audit event %d carries %q: %s", index, leaked, encoded)
			}
		}
		details, _ := event["details"].(map[string]any)
		actions = append(actions, details["action"].(string))
		if details["host"] != "siem.example.com" || event["org_id"] != "org-siem-enterprise" {
			t.Fatalf("event %d = %v", index, event)
		}
	}
	if strings.Join(actions, ",") != "created,updated,deleted" {
		t.Fatalf("actions = %v", actions)
	}
	// A second flush has nothing left: the rows are marked published.
	if again, err := service.FlushSIEMAuditOutbox(ctx, 100); err != nil || again.Published != 0 {
		t.Fatalf("second flush = %+v, %v", again, err)
	}
}

func TestControlLifecycleSIEMAuditOutboxRetriesAFailedPublish(t *testing.T) {
	ctx, db, _, service, publisher := siemPostgres(t)
	siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)

	publisher.err = errors.New("nats down")
	if result, err := service.FlushSIEMAuditOutbox(ctx, 10); err == nil || result.Published != 0 {
		t.Fatalf("flush with a failing publisher = %+v, %v", result, err)
	}
	var attempts int
	var publishedAt *time.Time
	if err := db.Pool.QueryRow(ctx, `SELECT attempts, published_at FROM organization_siem_audit_outbox WHERE org_id = 'org-siem-enterprise'`).Scan(&attempts, &publishedAt); err != nil {
		t.Fatalf("read outbox: %v", err)
	}
	if attempts != 1 || publishedAt != nil {
		t.Fatalf("after a failure attempts=%d published=%v; want 1 and unpublished", attempts, publishedAt)
	}
	// The retry waits out its backoff: pull the row forward, then it publishes.
	if _, err := db.Pool.Exec(ctx, `UPDATE organization_siem_audit_outbox SET next_attempt_at = NOW() WHERE org_id = 'org-siem-enterprise'`); err != nil {
		t.Fatalf("expire backoff: %v", err)
	}
	publisher.err = nil
	if result, err := service.FlushSIEMAuditOutbox(ctx, 10); err != nil || result.Published != 1 {
		t.Fatalf("retry flush = %+v, %v", result, err)
	}
}

func TestControlLifecycleSIEMAtMostThreeDestinationsPerOrganization(t *testing.T) {
	ctx, db, _, service, _ := siemPostgres(t)
	for i := 0; i < MaxSIEMDestinationsPerOrg; i++ {
		siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)
	}
	_, err := service.CreateSIEMDestination(ctx, "org-siem-enterprise", "u-admin", "admin", CreateSIEMDestinationRequest{
		Kind: SIEMKindSplunkHEC, URL: "https://hec.example.com/x", Secret: siemPGSecret,
	})
	if !errors.Is(err, ErrSIEMDestinationLimit) {
		t.Fatalf("fourth destination err = %v; want ErrSIEMDestinationLimit", err)
	}
	// The trigger is the backstop for a writer that is not the service.
	_, trigErr := db.Pool.Exec(ctx, `
INSERT INTO siem_destinations (org_id, kind, url, secret_ciphertext, secret_key_id, created_by)
VALUES ('org-siem-enterprise', 'splunk_hec', 'https://hec.example.com/x', decode(repeat('ab', 40), 'hex'), 'v1', 'raw-writer')`)
	if trigErr == nil {
		t.Fatalf("a raw insert beat the table limit")
	}
	// Another organisation is unaffected by this one's count.
	siemCreate(t, ctx, service, "org-siem-other", siemPGSecret)

	// Concurrent creates cannot overshoot the cap.
	if _, err := db.Pool.Exec(ctx, `DELETE FROM siem_destinations WHERE org_id = 'org-siem-enterprise'`); err != nil {
		t.Fatalf("reset: %v", err)
	}
	var wg sync.WaitGroup
	results := make(chan error, 8)
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, err := service.CreateSIEMDestination(ctx, "org-siem-enterprise", "u-admin", "admin", CreateSIEMDestinationRequest{
				Kind: SIEMKindNDJSONWebhook, URL: "https://siem.example.com/x", Secret: siemPGSecret,
			})
			results <- err
		}()
	}
	wg.Wait()
	close(results)
	created, limited := 0, 0
	for err := range results {
		switch {
		case err == nil:
			created++
		case errors.Is(err, ErrSIEMDestinationLimit):
			limited++
		default:
			t.Fatalf("unexpected error: %v", err)
		}
	}
	if created != MaxSIEMDestinationsPerOrg || limited != 8-MaxSIEMDestinationsPerOrg {
		t.Fatalf("8 racing creates gave %d created, %d limited; want 3 and 5", created, limited)
	}
}

func TestControlLifecycleSIEMRowLevelSecurityIsolatesOrganizations(t *testing.T) {
	ctx, db, _, service, _ := siemPostgres(t)
	siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)

	// Another tenant's scope sees none of it and cannot write into it.
	err := db.WithOrgScope(ctx, "org-siem-other", func(tx pgx.Tx) error {
		var count int
		if err := tx.QueryRow(ctx, `SELECT COUNT(*) FROM siem_destinations`).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			t.Fatalf("org-siem-other sees %d destinations of another organisation", count)
		}
		return nil
	})
	if err != nil {
		t.Fatalf("cross-tenant read: %v", err)
	}
	crossWrite := db.WithOrgScope(ctx, "org-siem-other", func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
INSERT INTO siem_destinations (org_id, kind, url, secret_ciphertext, secret_key_id, created_by)
VALUES ('org-siem-enterprise', 'splunk_hec', 'https://hec.example.com/x', decode(repeat('ab', 40), 'hex'), 'v1', 'attacker')`)
		return err
	})
	if crossWrite == nil {
		t.Fatalf("a scoped transaction wrote a destination into another organisation")
	}
	crossAudit := db.WithOrgScope(ctx, "org-siem-other", func(tx pgx.Tx) error {
		_, err := tx.Exec(ctx, `
INSERT INTO organization_siem_audit_outbox (event_id, org_id, subject, payload)
VALUES ('forged', 'org-siem-enterprise', $1, '{"event_id":"forged","org_id":"org-siem-enterprise"}')`, SIEMAuditSubject)
		return err
	})
	if crossAudit == nil {
		t.Fatalf("a scoped transaction forged an audit row for another organisation")
	}

	// The service-level paths agree: another organisation's id does not resolve.
	if _, err := service.ListSIEMDestinations(ctx, "org-siem-other"); err != nil {
		t.Fatalf("list other: %v", err)
	}
	created := siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)
	if _, err := service.UpdateSIEMDestination(ctx, "org-siem-other", created.ID, "u", "admin", UpdateSIEMDestinationRequest{ExpectedRevision: 1, Enabled: ptr(false)}); !errors.Is(err, ErrSIEMDestinationNotFound) {
		t.Fatalf("cross-org update err = %v; want not found", err)
	}
	if err := service.DeleteSIEMDestination(ctx, "org-siem-other", created.ID, "u", "admin"); !errors.Is(err, ErrSIEMDestinationNotFound) {
		t.Fatalf("cross-org delete err = %v; want not found", err)
	}
}

func TestControlLifecycleSIEMRevisionConflictAndPlanGate(t *testing.T) {
	ctx, db, _, service, _ := siemPostgres(t)
	created := siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)

	if _, err := service.UpdateSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u", "admin", UpdateSIEMDestinationRequest{ExpectedRevision: 1, Enabled: ptr(false)}); err != nil {
		t.Fatalf("first update: %v", err)
	}
	_, err := service.UpdateSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u", "admin", UpdateSIEMDestinationRequest{ExpectedRevision: 1, Enabled: ptr(true)})
	var conflict *SIEMRevisionConflictError
	if !errors.As(err, &conflict) || conflict.Current != 2 {
		t.Fatalf("stale update err = %v; want a conflict at revision 2", err)
	}

	// A plan that no longer allows SIEM export cannot create or switch on, but
	// can always switch off and delete.
	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET plan = 'pro' WHERE id = 'org-siem-enterprise'`); err != nil {
		t.Fatalf("lapse plan: %v", err)
	}
	if _, err := service.UpdateSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u", "admin", UpdateSIEMDestinationRequest{ExpectedRevision: 2, Enabled: ptr(true)}); !errors.Is(err, ErrPlanUpgradeRequired) {
		t.Fatalf("enabling on a lapsed plan err = %v; want ErrPlanUpgradeRequired", err)
	}
	if _, err := service.CreateSIEMDestination(ctx, "org-siem-pro", "u", "admin", CreateSIEMDestinationRequest{Kind: SIEMKindSplunkHEC, URL: "https://hec.example.com/x", Secret: siemPGSecret}); !errors.Is(err, ErrPlanUpgradeRequired) {
		t.Fatalf("create on a pro plan err = %v; want ErrPlanUpgradeRequired", err)
	}
	if _, err := service.UpdateSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u", "admin", UpdateSIEMDestinationRequest{ExpectedRevision: 2, URL: ptr("https://other.example.com/x")}); err != nil {
		t.Fatalf("editing on a lapsed plan: %v", err)
	}
	if err := service.DeleteSIEMDestination(ctx, "org-siem-enterprise", created.ID, "u", "admin"); err != nil {
		t.Fatalf("deleting on a lapsed plan: %v", err)
	}
}

func TestControlLifecycleSIEMExporterReadsEnabledEntitledDestinationsWithTheirSecret(t *testing.T) {
	ctx, db, _, service, _ := siemPostgres(t)
	enabled := siemCreate(t, ctx, service, "org-siem-enterprise", siemPGSecret)
	disabled := siemCreate(t, ctx, service, "org-siem-enterprise", "pg-fixture-disabled-0123456789")
	if _, err := service.UpdateSIEMDestination(ctx, "org-siem-enterprise", disabled.ID, "u", "admin", UpdateSIEMDestinationRequest{ExpectedRevision: 1, Enabled: ptr(false)}); err != nil {
		t.Fatalf("disable: %v", err)
	}
	other := siemCreate(t, ctx, service, "org-siem-other", "lllllllllllllllllllllllllll")

	rows, err := service.ListSIEMDestinationsForExport(ctx, nil)
	if err != nil {
		t.Fatalf("export: %v", err)
	}
	got := map[string]SIEMExporterDestination{}
	for _, row := range rows {
		got[row.ID] = row
	}
	if len(got) != 2 || got[enabled.ID].Secret != siemPGSecret || got[other.ID].Secret != "lllllllllllllllllllllllllll" {
		t.Fatalf("export = %+v; want the two enabled destinations of both organisations, secrets unsealed", rows)
	}
	if _, present := got[disabled.ID]; present {
		t.Fatalf("a disabled destination was exported")
	}

	// updated_since narrows by row change time.
	cut := time.Now().UTC().Add(time.Hour)
	if rows, err := service.ListSIEMDestinationsForExport(ctx, &cut); err != nil || len(rows) != 0 {
		t.Fatalf("export since the future = %v, %v; want none", rows, err)
	}
	past := time.Now().UTC().Add(-time.Hour)
	if rows, err := service.ListSIEMDestinationsForExport(ctx, &past); err != nil || len(rows) != 2 {
		t.Fatalf("export since the past = %d rows, %v; want 2", len(rows), err)
	}

	// An organisation whose plan lapsed drops out of the export without its
	// rows being touched.
	if _, err := db.Pool.Exec(ctx, `UPDATE organizations SET plan = 'pro' WHERE id = 'org-siem-other'`); err != nil {
		t.Fatalf("lapse plan: %v", err)
	}
	if rows, _ := service.ListSIEMDestinationsForExport(ctx, nil); len(rows) != 1 || rows[0].OrgID != "org-siem-enterprise" {
		t.Fatalf("export after a plan lapse = %+v", rows)
	}

	// A row sealed under another key is left out rather than exported unsigned.
	if _, err := db.Pool.Exec(ctx, `UPDATE siem_destinations SET secret_key_id = 'old-key' WHERE id = $1`, enabled.ID); err != nil {
		t.Fatalf("relabel key: %v", err)
	}
	if rows, _ := service.ListSIEMDestinationsForExport(ctx, nil); len(rows) != 0 {
		t.Fatalf("a destination whose secret does not open was exported: %+v", rows)
	}
}

func TestControlLifecycleSIEMEntitlementIsComputedOnlyWhenEnabled(t *testing.T) {
	ctx, _, repo, service, _ := siemPostgres(t)
	has := func(s *Service, orgID string) (present, enabled bool) {
		ents, err := s.GetEntitlements(ctx, orgID)
		if err != nil {
			t.Fatalf("entitlements: %v", err)
		}
		for _, e := range ents {
			if e.Key == SIEMExportEntitlementKey {
				return true, e.Enabled
			}
		}
		return false, false
	}
	if present, enabled := has(service, "org-siem-enterprise"); !present || !enabled {
		t.Fatalf("enterprise entitlement present=%v enabled=%v", present, enabled)
	}
	if present, enabled := has(service, "org-siem-pro"); !present || enabled {
		t.Fatalf("pro entitlement present=%v enabled=%v; want present and locked", present, enabled)
	}
	// A service without the feature reports nothing new.
	if present, _ := has(NewService(repo, nil), "org-siem-enterprise"); present {
		t.Fatalf("feature.siem_export is reported while SIEM destinations are off")
	}
}
