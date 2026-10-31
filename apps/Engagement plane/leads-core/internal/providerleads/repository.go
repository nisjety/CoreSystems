package providerleads

import (
	"context"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// PGRepository persists provider leads in the provider_leads table — the ONE
// deliberately person-data-carrying table in leads-core (see the package doc
// and migration 002 for the containment rules).
type PGRepository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	return &PGRepository{pool: pool}
}

// UpsertLeads inserts or refreshes leads, deduped on
// (org_id, provider_key, provider_lead_id) so re-syncs are idempotent.
func (r *PGRepository) UpsertLeads(ctx context.Context, leads []ProviderLead) (int, error) {
	if len(leads) == 0 {
		return 0, nil
	}
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return 0, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	upserted := 0
	for _, lead := range leads {
		if strings.TrimSpace(lead.OrgID) == "" || strings.TrimSpace(lead.ProviderLeadID) == "" {
			continue
		}
		fields := lead.Fields
		if len(fields) == 0 {
			fields = []byte("[]")
		}
		tag, err := tx.Exec(ctx, `
INSERT INTO provider_leads (
	id, org_id, connection_id, provider_key, provider_lead_id,
	form_id, form_name, submitted_at, fields
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
ON CONFLICT (org_id, provider_key, provider_lead_id) DO UPDATE SET
	connection_id = EXCLUDED.connection_id,
	form_id       = EXCLUDED.form_id,
	form_name     = EXCLUDED.form_name,
	submitted_at  = EXCLUDED.submitted_at,
	fields        = EXCLUDED.fields`,
			newID("plead"), lead.OrgID, lead.ConnectionID, lead.ProviderKey, lead.ProviderLeadID,
			lead.FormID, lead.FormName, lead.SubmittedAt, string(fields))
		if err != nil {
			return 0, fmt.Errorf("upsert provider_leads: %w", err)
		}
		upserted += int(tag.RowsAffected())
	}

	if err := tx.Commit(ctx); err != nil {
		return 0, err
	}
	committed = true
	return upserted, nil
}

// DeleteByOrg is the GDPR erasure path: it removes every provider lead an org
// holds, in one org-scoped statement.
func (r *PGRepository) DeleteByOrg(ctx context.Context, orgID string) (int64, error) {
	orgID = strings.TrimSpace(orgID)
	if orgID == "" {
		return 0, fmt.Errorf("org_id is required")
	}
	tag, err := r.pool.Exec(ctx, `DELETE FROM provider_leads WHERE org_id = $1`, orgID)
	if err != nil {
		return 0, fmt.Errorf("delete provider_leads: %w", err)
	}
	return tag.RowsAffected(), nil
}
