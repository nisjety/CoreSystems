package skillbundle

import (
	"context"
	"fmt"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// PutImportedDraft stores the bundle of a skill that is being imported (EP-11 S8) as that skill's
// draft bundle, inside the import's transaction tx, so the skill row and its files are written
// together or not at all. The skill is new, so there is no live bundle to supersede and nothing to
// collect. The bundle becomes live only when the draft is published (PublishDraftBundle); every
// read leaves a draft bundle out until then. The audit row names the digest and the totals, never
// a path or a body.
func PutImportedDraft(ctx context.Context, tx pgx.Tx, orgID, skillID, actor string, bundle Bundle) error {
	if _, err := tx.Exec(ctx, lockOrgSQL, orgID); err != nil {
		return fmt.Errorf("lock skill bundles: %w", err)
	}
	bundleID := "skb_" + uuid.NewString()
	if _, err := tx.Exec(ctx, `
		INSERT INTO skill_bundles (id, org_id, skill_id, manifest_sha256, file_count, total_bytes, created_by, state)
		VALUES ($1, $2, $3, $4, $5, $6, $7, 'draft')
	`, bundleID, orgID, skillID, bundle.ManifestSHA256, len(bundle.Files), bundle.TotalBytes, actor); err != nil {
		return fmt.Errorf("insert imported skill bundle: %w", err)
	}
	if err := storeFiles(ctx, tx, orgID, bundleID, bundle); err != nil {
		return err
	}
	return appendAudit(ctx, tx, "skill_bundle.imported", orgID, skillID, actor, bundle.ManifestSHA256,
		len(bundle.Files), bundle.TotalBytes)
}
