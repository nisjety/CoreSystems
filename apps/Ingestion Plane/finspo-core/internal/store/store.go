package store

import "github.com/jackc/pgx/v5/pgxpool"

// Store bundles all repositories that share a single pgxpool.
type Store struct {
	pool *pgxpool.Pool
}

func New(pool *pgxpool.Pool) *Store {
	return &Store{pool: pool}
}

func (s *Store) Pool() *pgxpool.Pool { return s.pool }

func (s *Store) Sources() *Sources         { return &Sources{pool: s.pool} }
func (s *Store) Items() *Items             { return &Items{pool: s.pool} }
func (s *Store) Cursors() *Cursors         { return &Cursors{pool: s.pool} }
func (s *Store) Permissions() *Permissions { return &Permissions{pool: s.pool} }
func (s *Store) Analytics() *Analytics     { return &Analytics{pool: s.pool} }
func (s *Store) Proposals() *Proposals     { return &Proposals{pool: s.pool} }
func (s *Store) Audit() *Audit             { return &Audit{pool: s.pool} }
func (s *Store) Locks() *Locks             { return &Locks{pool: s.pool} }

func (s *Store) Recommendations() *Recommendations {
	return &Recommendations{analytics: s.Analytics()}
}
