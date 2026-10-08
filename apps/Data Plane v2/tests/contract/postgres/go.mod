module github.com/triodelab/dataplane/tests/contract/postgres

go 1.26.5

require (
	github.com/jackc/pgx/v5 v5.9.2
	github.com/triodelab/dataplane/shared/go v0.0.0-00010101000000-000000000000
)

require (
	github.com/jackc/pgpassfile v1.0.0 // indirect
	github.com/jackc/pgservicefile v0.0.0-20240606120523-5a60cdf6a761 // indirect
	github.com/jackc/puddle/v2 v2.2.2 // indirect
	golang.org/x/sync v0.17.0 // indirect
	golang.org/x/text v0.29.0 // indirect
)

// The real org-scope helper every Data Plane Go service uses. The contract tests
// call it (not a copy of what it does) so the role switch and the app.current_org
// setting under test are exactly the ones production issues.
replace github.com/triodelab/dataplane/shared/go => ../../../shared/go
