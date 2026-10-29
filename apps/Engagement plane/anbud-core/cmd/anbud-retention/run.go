package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/retention"
)

func openPool(ctx context.Context, dsn string) (*pgxpool.Pool, error) { return db.Open(ctx, dsn) }

// checkTLS is the rule every client of the database follows (REQUIREMENTS.md section 9.5), named by
// this command's variable.
func checkTLS(dsn string) error { return db.CheckTLSFor(envDSN, dsn) }

// The roles of the role model belong to processes, not to people: a tombstone that names one of them
// says nothing about who erased. A superuser bypasses the guards the procedure relies on.
var systemRoles = map[string]bool{"anbud_app": true, "anbud_migrator": true, "anbud_dba": true}

const selectLogin = `
SELECT session_user::text,
       r.rolsuper,
       pg_catalog.pg_has_role(session_user, 'anbud_dba', 'MEMBER'),
       pg_catalog.has_function_privilege('public.anbud_redact_raw_document(bigint, text, text)', 'EXECUTE')
  FROM pg_catalog.pg_roles r
 WHERE r.rolname = session_user`

// checkLogin refuses any login but a person's own role in anbud_dba, the rule of the manual
// procedure (docs/PERSONAL-DATA.md section 3). It runs in a dry run too, so that a dry run shows
// what the same login would erase.
func checkLogin(ctx context.Context, pool *pgxpool.Pool) error {
	var (
		name                     string
		super, member, mayRedact bool
	)
	if err := pool.QueryRow(ctx, selectLogin).Scan(&name, &super, &member, &mayRedact); err != nil {
		return fmt.Errorf("could not check the login (SQLSTATE %s); is the role model in place (scripts/roles.sql)?", sqlState(err))
	}
	switch {
	case super:
		return errors.New("refused: the login is a superuser; run it as your own login in anbud_dba")
	case systemRoles[name]:
		return errors.New("refused: the login is a role of the role model, not a person's; run it as your own login in anbud_dba")
	case !member || !mayRedact:
		return errors.New("refused: the login is not a member of anbud_dba, or cannot run anbud_redact_raw_document()")
	}
	return nil
}

// The readers read raw_documents_live: a body that is already erased is not a body to decide on.
const selectLive = `
SELECT id, doffin_id, body, fetched_at
  FROM public.raw_documents_live
 WHERE source = 'doffin'
 ORDER BY id`

const redact = `SELECT public.anbud_redact_raw_document($1, 'controller_decision', $2)`

// sqlStateNotInPrerequisiteState is what the function answers for a row that is already redacted.
const sqlStateNotInPrerequisiteState = "55000"

type expired struct {
	ID      int64  `json:"id"`
	Notice  string `json:"notice"`
	Basis   string `json:"basis"`
	From    string `json:"from"`
	Expires string `json:"expires"`
	Action  string `json:"action"`
}

type summary struct {
	Mode           string         `json:"mode"`
	AsOf           string         `json:"as_of"`
	Months         int            `json:"months"`
	RequestRef     string         `json:"request_ref"`
	Held           int            `json:"held"`
	KeptOpen       int            `json:"kept_open"`
	Expired        int            `json:"expired"`
	ExpiredByBasis map[string]int `json:"expired_by_basis"`
	Erased         int            `json:"erased"`
	AlreadyErased  int            `json:"already_erased"`
	Remaining      int            `json:"remaining"`
}

// requestRef is the request reference of the tombstones: the period and the day, never a person.
func requestRef(r request) string {
	return fmt.Sprintf("retention:%dm:%s", r.months, r.asOf.Format(dateLayout))
}

func run(ctx context.Context, pool *pgxpool.Pool, r request, stdout, stderr io.Writer) int {
	s := summary{Mode: "dry_run", AsOf: r.asOf.Format(dateLayout), Months: r.months, RequestRef: requestRef(r), ExpiredByBasis: map[string]int{}}
	if r.apply {
		s.Mode = "apply"
	}
	var due []expired
	rows, err := pool.Query(ctx, selectLive)
	if err != nil {
		fatalf(stderr, fmt.Sprintf("reading the stored bodies failed (SQLSTATE %s)", sqlState(err)))
		return exitFailed
	}
	for rows.Next() {
		var (
			id      int64
			notice  string
			body    []byte
			fetched time.Time
		)
		if err := rows.Scan(&id, &notice, &body, &fetched); err != nil {
			rows.Close()
			fatalf(stderr, fmt.Sprintf("reading the stored bodies failed (SQLSTATE %s)", sqlState(err)))
			return exitFailed
		}
		s.Held++
		d, err := retention.Decide(body, fetched, r.months, r.asOf)
		if err != nil {
			rows.Close()
			fatalf(stderr, err.Error())
			return exitUsage
		}
		switch {
		case d.Basis == retention.BasisOpen:
			s.KeptOpen++
		case d.Expired:
			s.Expired++
			s.ExpiredByBasis[string(d.Basis)]++
			due = append(due, expired{ID: id, Notice: notice, Basis: string(d.Basis), From: d.From.Format(dateLayout), Expires: d.ExpiresAt.Format(dateLayout), Action: "listed"})
		}
	}
	if err := rows.Err(); err != nil {
		fatalf(stderr, fmt.Sprintf("reading the stored bodies failed (SQLSTATE %s)", sqlState(err)))
		return exitFailed
	}

	enc := json.NewEncoder(stdout)
	code := exitDone
	for i := range due {
		e := &due[i]
		if r.apply {
			if s.Erased+s.AlreadyErased >= r.limit {
				break
			}
			var tombstone int64
			err := pool.QueryRow(ctx, redact, e.ID, s.RequestRef).Scan(&tombstone)
			switch {
			case err == nil:
				e.Action = "erased"
				s.Erased++
			case sqlState(err) == sqlStateNotInPrerequisiteState:
				// Erased by someone else since it was read: nothing to do, and no second tombstone.
				e.Action = "already_erased"
				s.AlreadyErased++
			default:
				_ = enc.Encode(s)
				fatalf(stderr, fmt.Sprintf("erasing row %d failed (SQLSTATE %s); %d erased before it stay erased", e.ID, sqlState(err), s.Erased))
				return exitFailed
			}
		}
		_ = enc.Encode(e)
	}
	if r.apply {
		s.Remaining = s.Expired - s.Erased - s.AlreadyErased
		if s.Remaining > 0 {
			code = exitPartial
		}
	}
	_ = enc.Encode(s)
	return code
}

// sqlState is the SQLSTATE of a database error, or "none". pgx's error text can quote a statement or
// a DSN, so only the code is ever printed.
func sqlState(err error) string {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code
	}
	return "none"
}
