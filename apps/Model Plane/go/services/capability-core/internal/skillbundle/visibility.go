package skillbundle

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
)

// ScopeRulesEnv is SKILL_BUNDLE_SCOPE_RULES (F-f03-08; registers D2-379, D2-381, D2-382). Only the
// literal "true" turns it on. On, a person reads only the bundles of skills they may see, an upload
// needs its skill to exist, and a skill delete removes the skill's bundles whenever the bundle surface
// is on. Off, every read, write and delete is as it was.
const ScopeRulesEnv = "SKILL_BUNDLE_SCOPE_RULES"

// ScopeRulesFromLookup reads SKILL_BUNDLE_SCOPE_RULES strictly.
func ScopeRulesFromLookup(lookup func(string) string) bool {
	return lookup(ScopeRulesEnv) == "true"
}

// SkillVisibilitySQL is the visibility rule of a skill (SKILL-1) as a predicate over the agent_skills
// row of the statement it sits in: an org skill, the caller's own, an accepted skill shared with the
// caller and, for an admin, an accepted skill that some person shares. A proposal is its owner's alone
// until it is accepted. callerArg and adminArg name the statement's placeholders for the caller id and
// the admin flag. The skill listing, get, the history route and the bundle reads all take this one
// text, so a bundle is visible exactly when its skill is.
func SkillVisibilitySQL(callerArg, adminArg string) string {
	return "(scope = 'org' OR owner_user_id = " + callerArg +
		" OR (review_state = 'accepted' AND (shared_with ? " + callerArg +
		" OR (" + adminArg + " AND jsonb_array_length(shared_with) > 0))))"
}

// ListVisible is List for a person under SKILL_BUNDLE_SCOPE_RULES: the org's live bundle summaries,
// narrowed to skillIDs when any are given and to the skills the caller may see. A caller with no id
// sees none. Its DeckTemplate column is the theme file AND the deck stamp (deck_validated_at, F-f09-08), as in
// List: an unstamped bundle is never listed as a deck template here either. Keep that predicate when this file is
// rewritten; visibility_deck_test.go and visibility_deck_integration_test.go pin it (P2-go-13-rel1).
func (s *Store) ListVisible(ctx context.Context, orgID, callerID string, isAdmin bool, skillIDs []string) ([]Summary, error) {
	if callerID == "" {
		return []Summary{}, nil
	}
	var filter []string
	if len(skillIDs) > 0 {
		filter = skillIDs
	}
	rows, err := s.db.Query(ctx, `
		SELECT b.skill_id, b.manifest_sha256, b.file_count, b.total_bytes, b.created_by, b.created_at,
		       (EXISTS (SELECT 1 FROM skill_bundle_files f WHERE f.bundle_id = b.id AND f.path = $3)
		        AND b.deck_validated_at IS NOT NULL)
		FROM skill_bundles b
		WHERE b.org_id = $1 AND ($2::text[] IS NULL OR b.skill_id = ANY($2::text[])) AND b.state = 'live'
		  AND EXISTS (
		      SELECT 1 FROM agent_skills s
		      WHERE s.org_id = b.org_id AND s.id = b.skill_id AND `+SkillVisibilitySQL("$4", "$5")+`
		  )
		ORDER BY b.skill_id
	`, orgID, filter, ThemePath, callerID, isAdmin)
	if err != nil {
		return nil, fmt.Errorf("list skill bundles: %w", err)
	}
	return scanSummaries(rows)
}

// scanSummaries gathers the summary rows of a list statement and closes them.
func scanSummaries(rows pgx.Rows) ([]Summary, error) {
	defer rows.Close()
	summaries := []Summary{}
	for rows.Next() {
		var summary Summary
		if err := rows.Scan(&summary.SkillID, &summary.ManifestSHA256, &summary.FileCount,
			&summary.TotalBytes, &summary.CreatedBy, &summary.CreatedAt, &summary.DeckTemplate); err != nil {
			return nil, fmt.Errorf("scan skill bundle: %w", err)
		}
		summaries = append(summaries, summary)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("list skill bundles: %w", err)
	}
	return summaries, nil
}

// ManifestVisible is Manifest, or ManifestAt when manifestSHA256 is named, for a person under
// SKILL_BUNDLE_SCOPE_RULES. The bundle is read only when the caller may see the skill, by the rule the
// skill listing applies. An empty manifestSHA256 reads the live bundle; a digest reads the bundle that
// holds it, live or kept, never a draft. A skill the caller may not see answers ErrNotFound, exactly as
// a missing bundle does, and so does a caller with no id.
func (s *Store) ManifestVisible(ctx context.Context, orgID, callerID string, isAdmin bool, skillID, manifestSHA256 string) (Manifest, error) {
	if callerID == "" {
		return Manifest{}, ErrNotFound
	}
	return s.readManifest(ctx, skillID, `
		SELECT b.manifest_sha256, f.path, f.sha256, f.size_bytes, f.executable, f.is_binary
		FROM skill_bundles b
		JOIN skill_bundle_files f ON f.bundle_id = b.id
		WHERE b.org_id = $1 AND b.skill_id = $2
		  AND (($5::text = '' AND b.state = 'live') OR ($5::text <> '' AND b.manifest_sha256 = $5::text AND b.state <> 'draft'))
		  AND EXISTS (
		      SELECT 1 FROM agent_skills s
		      WHERE s.org_id = b.org_id AND s.id = b.skill_id AND `+SkillVisibilitySQL("$3", "$4")+`
		  )
		ORDER BY f.path
	`, orgID, skillID, callerID, isAdmin, manifestSHA256)
}
