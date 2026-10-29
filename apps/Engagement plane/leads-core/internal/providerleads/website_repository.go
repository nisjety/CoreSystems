package providerleads

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

func (r *PGRepository) ReceiveWebsiteInquiry(ctx context.Context, b WebsiteInstallation, v WebsiteInquiry, digest []byte) (WebsiteReceipt, error) {
	fields, err := json.Marshal(v)
	if err != nil {
		return WebsiteReceipt{}, err
	}
	// One atomic statement commits both the person-data record and its outcome.
	// Replays return the original receipt, and changed content cannot overwrite it.
	providerID := fmt.Sprintf("%s/%d/%s/%s", b.WebsiteID, b.WebsiteRevision, b.Environment, v.SubmissionID)
	var id string
	var accepted time.Time
	err = r.pool.QueryRow(ctx, `INSERT INTO provider_leads
 (id,org_id,connection_id,provider_key,provider_lead_id,form_id,form_name,submitted_at,fields,website_id,website_revision,website_site,website_environment,submission_digest)
 VALUES ($1,$2,$3,'website_inquiry',$4,'contact','Website inquiry',now(),$5,$3,$6,$7,$8,$9)
 ON CONFLICT (org_id,provider_key,provider_lead_id) DO UPDATE SET id=provider_leads.id
 WHERE provider_leads.submission_digest=EXCLUDED.submission_digest
 RETURNING id,created_at`, newID("plead"), b.OrganizationID, b.WebsiteID, providerID, string(fields), b.WebsiteRevision, b.Site, b.Environment, digest).Scan(&id, &accepted)
	if errors.Is(err, pgx.ErrNoRows) {
		return WebsiteReceipt{}, ErrWebsiteConflict
	}
	if err != nil {
		return WebsiteReceipt{}, errors.New("website inquiry storage unavailable")
	}
	return WebsiteReceipt{ReceiptID: id, AcceptedAt: accepted.UTC().Format(time.RFC3339Nano)}, nil
}
func (r *PGRepository) WebsiteOutcomes(ctx context.Context, b WebsiteInstallation, start, end string) (WebsiteOutcomes, error) {
	result := WebsiteOutcomes{WebsiteID: b.WebsiteID, WebsiteRevision: b.WebsiteRevision, Site: b.Site, Environment: b.Environment, Source: "leads_core", Status: "connected", StartDate: start, EndDate: end, TimeZone: "Europe/Oslo", AsOf: time.Now().UTC().Format(time.RFC3339Nano)}
	var last *time.Time
	err := r.pool.QueryRow(ctx, `SELECT count(*),max(created_at) FROM provider_leads
 WHERE org_id=$1 AND provider_key='website_inquiry' AND website_id=$2 AND website_revision=$3 AND website_site=$4 AND website_environment=$5
 AND created_at >= ($6::date::timestamp AT TIME ZONE 'Europe/Oslo')
 AND created_at < (($7::date+1)::timestamp AT TIME ZONE 'Europe/Oslo')`, b.OrganizationID, b.WebsiteID, b.WebsiteRevision, b.Site, b.Environment, start, end).Scan(&result.Inquiries, &last)
	if err != nil {
		return WebsiteOutcomes{}, errors.New("website outcome storage unavailable")
	}
	result.Leads = result.Inquiries
	if last != nil {
		v := last.UTC().Format(time.RFC3339Nano)
		result.LastReceivedAt = &v
	}
	return result, nil
}
