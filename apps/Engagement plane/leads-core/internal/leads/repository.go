package leads

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/leads-core/internal/brreg"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// PGRepository persists saved lead lists in Postgres. Only company data is
// stored — lead_list_companies has no person/role/birth-number column.
type PGRepository struct {
	pool *pgxpool.Pool
}

func NewRepository(pool *pgxpool.Pool) *PGRepository {
	return &PGRepository{pool: pool}
}

func (r *PGRepository) CreateList(ctx context.Context, input CreateListInput) (*SavedList, error) {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return nil, err
	}
	committed := false
	defer func() {
		if !committed {
			_ = tx.Rollback(ctx)
		}
	}()

	listID := newID("list")
	now := time.Now().UTC()
	if _, err := tx.Exec(ctx, `
INSERT INTO lead_lists (id, org_id, name, created_by, created_at, updated_at)
VALUES ($1, $2, $3, $4, $5, $5)`, listID, input.OrgID, input.Name, input.CreatedBy, now); err != nil {
		return nil, fmt.Errorf("insert lead_lists: %w", err)
	}

	for _, c := range input.Companies {
		if _, err := tx.Exec(ctx, `
INSERT INTO lead_list_companies (
	id, list_id, org_id, organisasjonsnummer, navn, organisasjonsform, naeringskode,
	naering_beskrivelse, kommunenummer, poststed, antall_ansatte, registreringsdato,
	hjemmeside, konkurs, under_avvikling, created_at
) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)
ON CONFLICT (list_id, organisasjonsnummer) DO NOTHING`,
			newID("lc"), listID, input.OrgID, c.Organisasjonsnummer, c.Navn, c.Organisasjonsform,
			c.Naeringskode, c.NaeringBeskrivelse, c.Kommunenummer, c.Poststed, c.AntallAnsatte,
			c.Registreringsdato, c.Hjemmeside, c.Konkurs, c.UnderAvvikling, now); err != nil {
			return nil, fmt.Errorf("insert lead_list_companies: %w", err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return nil, err
	}
	committed = true

	return &SavedList{
		ID:           listID,
		OrgID:        input.OrgID,
		Name:         input.Name,
		CreatedBy:    input.CreatedBy,
		CompanyCount: len(input.Companies),
		CreatedAt:    now,
		UpdatedAt:    now,
	}, nil
}

func (r *PGRepository) ListLists(ctx context.Context, orgID string) ([]SavedList, error) {
	rows, err := r.pool.Query(ctx, `
SELECT l.id, l.org_id, l.name, l.created_by, l.created_at, l.updated_at,
       (SELECT count(*) FROM lead_list_companies c WHERE c.list_id = l.id) AS company_count
FROM lead_lists l
WHERE l.org_id = $1
ORDER BY l.created_at DESC`, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	lists := []SavedList{}
	for rows.Next() {
		var l SavedList
		if err := rows.Scan(&l.ID, &l.OrgID, &l.Name, &l.CreatedBy, &l.CreatedAt, &l.UpdatedAt, &l.CompanyCount); err != nil {
			return nil, err
		}
		lists = append(lists, l)
	}
	return lists, rows.Err()
}

func (r *PGRepository) GetList(ctx context.Context, orgID, listID string) (*SavedList, error) {
	var l SavedList
	err := r.pool.QueryRow(ctx, `
SELECT id, org_id, name, created_by, created_at, updated_at
FROM lead_lists WHERE org_id = $1 AND id = $2`, orgID, listID).
		Scan(&l.ID, &l.OrgID, &l.Name, &l.CreatedBy, &l.CreatedAt, &l.UpdatedAt)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil, ErrNotFound
		}
		return nil, err
	}

	rows, err := r.pool.Query(ctx, `
SELECT organisasjonsnummer, navn, organisasjonsform, naeringskode, naering_beskrivelse,
       kommunenummer, poststed, antall_ansatte, registreringsdato, hjemmeside, konkurs, under_avvikling
FROM lead_list_companies
WHERE list_id = $1 AND org_id = $2
ORDER BY navn ASC`, listID, orgID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	l.Companies = []brreg.Company{}
	for rows.Next() {
		var c brreg.Company
		if err := rows.Scan(
			&c.Organisasjonsnummer, &c.Navn, &c.Organisasjonsform, &c.Naeringskode, &c.NaeringBeskrivelse,
			&c.Kommunenummer, &c.Poststed, &c.AntallAnsatte, &c.Registreringsdato, &c.Hjemmeside,
			&c.Konkurs, &c.UnderAvvikling,
		); err != nil {
			return nil, err
		}
		l.Companies = append(l.Companies, c)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	l.CompanyCount = len(l.Companies)
	return &l, nil
}

func (r *PGRepository) DeleteList(ctx context.Context, orgID, listID string) error {
	tag, err := r.pool.Exec(ctx, `DELETE FROM lead_lists WHERE org_id = $1 AND id = $2`, orgID, listID)
	if err != nil {
		return err
	}
	if tag.RowsAffected() == 0 {
		return ErrNotFound
	}
	return nil
}
