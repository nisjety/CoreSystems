package store

import (
	"errors"

	"github.com/jackc/pgx/v5"
)

// ErrNotFound is returned by Get-style methods when the requested row is absent.
var ErrNotFound = errors.New("store: not found")

// pgxNoRows is exported here to avoid sprinkling the pgx import across files.
var pgxNoRows = pgx.ErrNoRows
