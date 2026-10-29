package main

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
	"github.com/triodelab/anbud/internal/eforms/eformstest"
	"github.com/triodelab/anbud/internal/rawstore"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

// Needs a throwaway server on ANBUD_TEST_DSN: run these with make test-db. The notices are stored
// through anbud_app and the migrations go through anbud_migrator, as in production, and the command
// runs as a login made for the test and granted into anbud_dba, as a DBA's own login would be.

const (
	oldNotice   = "2020-100001" // its deadline was 2020-01-15: long expired
	freshNotice = "2026-100002" // its deadline is 2026-09-01: kept for twelve months after
	asOf        = "2026-10-28"
)

type seeded struct {
	rs  dbtest.RoleSet
	app *pgxpool.Pool
	dba string // the DSN of a DBA's own login
}

func seed(t *testing.T) seeded {
	t.Helper()
	ctx := context.Background()
	rs := dbtest.RoleDatabase(t)
	if err := db.ApplyMigrations(ctx, dbtest.Pool(t, rs.MigratorDSN)); err != nil {
		t.Fatalf("migrate: %s", dbtest.Describe(err))
	}
	app := dbtest.Pool(t, rs.AppDSN)
	store := rawstore.New(app)
	day := time.Date(2026, 10, 28, 0, 0, 0, 0, time.UTC)
	run, err := store.BeginRun(ctx, doffinsync.RunStart{Mode: doffinsync.ModeSync, From: day, To: day})
	if err != nil {
		t.Fatalf("begin run: %s", dbtest.Describe(err))
	}
	for id, deadline := range map[string]time.Time{
		oldNotice:   time.Date(2020, 1, 15, 12, 0, 0, 0, time.UTC),
		freshNotice: time.Date(2026, 9, 1, 12, 0, 0, 0, time.UTC),
	} {
		body := eformstest.ContractNotice(eformstest.Notice{ID: uuidFor(id), Deadline: deadline})
		sum := sha256.Sum256(body)
		n := doffin.Notice{ID: id, ContentType: "application/xml", Body: body, SHA256: hex.EncodeToString(sum[:]), FetchedAt: deadline.AddDate(0, -1, 0)}
		if _, err := store.StoreNotice(ctx, run, n); err != nil {
			t.Fatalf("store %s: %s", id, dbtest.Describe(err))
		}
	}
	return seeded{rs: rs, app: app, dba: dbtest.CreateLoginRole(t, rs, "anbud_test_retention_dba", "", dbtest.RoleDBA)}
}

func uuidFor(id string) string {
	return "00000000-0000-4000-8000-0000" + strings.ReplaceAll(id, "-", "")[:8]
}

func runCLI(t *testing.T, dsn string, args ...string) (int, string, string) {
	t.Helper()
	var out, errOut bytes.Buffer
	code := cli(args, func(k string) string {
		if k == envDSN {
			return dsn
		}
		return ""
	}, &out, &errOut)
	return code, out.String(), errOut.String()
}

func lastSummary(t *testing.T, out string) summary {
	t.Helper()
	lines := strings.Split(strings.TrimSpace(out), "\n")
	var s summary
	if err := json.Unmarshal([]byte(lines[len(lines)-1]), &s); err != nil {
		t.Fatalf("the last line is not a summary: %v", err)
	}
	return s
}

// redacted lists the notice ids whose bodies are erased, with each tombstone's reason, request
// reference and login.
func redacted(t *testing.T, pool *pgxpool.Pool) []string {
	t.Helper()
	rows, err := pool.Query(context.Background(), `
SELECT d.doffin_id || ' ' || t.reason || ' ' || t.request_ref || ' ' || t.redacted_by
  FROM raw_documents d JOIN raw_redactions t ON t.raw_document_id = d.id
 WHERE d.redacted_at IS NOT NULL AND octet_length(d.body) = 0
 ORDER BY d.doffin_id`)
	if err != nil {
		t.Fatalf("read redactions: %s", dbtest.Describe(err))
	}
	defer rows.Close()
	var got []string
	for rows.Next() {
		var s string
		if err := rows.Scan(&s); err != nil {
			t.Fatal(err)
		}
		got = append(got, s)
	}
	return got
}

func assertNoDSNParts(t *testing.T, output string, dsns ...string) {
	t.Helper()
	for _, dsn := range dsns {
		for _, part := range dbtest.DSNParts(dsn) {
			if strings.Contains(output, part) {
				t.Errorf("the output carries a part of a DSN (%d bytes of it)", len(part))
			}
		}
	}
}

// A dry run lists what has expired and erases nothing. -apply erases exactly that, through the
// audited function, with a tombstone that names the period, the day and the DBA's own login. A run
// after it finds nothing more, because an erased body is no longer a live one.
func TestRetentionErasesOnlyWhatHasExpiredAndOnlyWithApply(t *testing.T) {
	s := seed(t)

	code, out, errOut := runCLI(t, s.dba, "-months", "12", "-as-of", asOf)
	if code != exitDone {
		t.Fatalf("dry run: exit %d, stderr %q", code, errOut)
	}
	if sum := lastSummary(t, out); sum.Mode != "dry_run" || sum.Held != 2 || sum.Expired != 1 || sum.Erased != 0 || sum.ExpiredByBasis["deadline"] != 1 {
		t.Errorf("dry run summary %+v, want 2 held, 1 expired on its deadline, none erased", sum)
	}
	if !strings.Contains(out, `"notice":"`+oldNotice+`"`) || strings.Contains(out, freshNotice) {
		t.Errorf("dry run lists %q; want the old notice only", out)
	}
	if got := redacted(t, s.app); len(got) != 0 {
		t.Fatalf("a dry run erased %v", got)
	}

	code, out, errOut = runCLI(t, s.dba, "-months", "12", "-as-of", asOf, "-apply")
	if code != exitDone {
		t.Fatalf("apply: exit %d, stderr %q", code, errOut)
	}
	if sum := lastSummary(t, out); sum.Mode != "apply" || sum.Erased != 1 || sum.Remaining != 0 {
		t.Errorf("apply summary %+v, want 1 erased and none remaining", sum)
	}
	want := oldNotice + " controller_decision retention:12m:" + asOf + " anbud_test_retention_dba"
	if got := redacted(t, s.app); len(got) != 1 || got[0] != want {
		t.Errorf("redacted %v, want [%s]", got, want)
	}
	assertNoDSNParts(t, out+errOut, s.dba, s.rs.AppDSN)

	code, out, _ = runCLI(t, s.dba, "-months", "12", "-as-of", asOf, "-apply")
	if sum := lastSummary(t, out); code != exitDone || sum.Held != 1 || sum.Expired != 0 || sum.Erased != 0 {
		t.Errorf("second apply: exit %d, %+v; want the fresh notice held and nothing to erase", code, sum)
	}
}

// -limit stops an apply early and says so with exit 2; the next run goes on.
func TestALimitedApplyExitsPartialAndTheNextRunGoesOn(t *testing.T) {
	s := seed(t)
	code, out, _ := runCLI(t, s.dba, "-months", "1", "-as-of", asOf, "-apply", "-limit", "1")
	if sum := lastSummary(t, out); code != exitPartial || sum.Expired != 2 || sum.Erased != 1 || sum.Remaining != 1 {
		t.Fatalf("first apply: exit %d, %+v; want exit 2 with one erased and one remaining", code, sum)
	}
	code, out, _ = runCLI(t, s.dba, "-months", "1", "-as-of", asOf, "-apply", "-limit", "1")
	if sum := lastSummary(t, out); code != exitDone || sum.Erased != 1 || sum.Remaining != 0 {
		t.Errorf("second apply: exit %d, %+v; want the last one erased", code, sum)
	}
	if got := redacted(t, s.app); len(got) != 2 {
		t.Errorf("redacted %v, want both", got)
	}
}

// Only a person's own login in anbud_dba may run it: the application, the owner, a superuser and a
// login outside the group are refused before anything is read, and nothing is erased. The refusal is
// the command's own check; the function's refusal of the same roles is tested in internal/db.
func TestRetentionRefusesEveryLoginButADBAsOwn(t *testing.T) {
	s := seed(t)
	outsider := dbtest.CreateLoginRole(t, s.rs, "anbud_test_retention_outsider", "")
	for _, tc := range []struct {
		name, dsn, want string
	}{
		{"the application role", s.rs.AppDSN, "role of the role model"},
		{"the owner role", s.rs.MigratorDSN, "role of the role model"},
		{"the test server's own role", s.rs.AdminDSN, "superuser"},
		{"a login outside anbud_dba", outsider, "not a member of anbud_dba"},
	} {
		code, out, errOut := runCLI(t, tc.dsn, "-months", "1", "-as-of", asOf, "-apply")
		if code != exitUsage || !strings.Contains(errOut, tc.want) || out != "" {
			t.Errorf("%s: exit %d, stderr %q, stdout %q; want exit 1 saying %q and no output", tc.name, code, errOut, out, tc.want)
		}
		assertNoDSNParts(t, out+errOut, tc.dsn)
	}
	if got := redacted(t, s.app); len(got) != 0 {
		t.Errorf("a refused login erased %v", got)
	}
}
