package postgrescontract

import (
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
	"testing"
)

// priorVersion is the newest migration that was applied before CR-T6; cprVersion
// is this task's migration. The migrator applies by version order, so cprVersion
// must stay above priorVersion and unique.
const (
	priorVersion = "20260907120000"
	cprVersion   = "20261003130000"
	cprUp        = "20261003130000_context_provenance.sql"
	cprDown      = "20261003130000_context_provenance.down.sql"
)

// statusVocabulary is the closed set of context_status values. NULL is separate:
// it means "unknown", not a status. Downstream tasks (CR-T7/T8/T9a/T11) write and
// read exactly these strings; changing the list is a cross-task decision.
var statusVocabulary = []string{
	"pending", "ready", "legacy",
	"skipped_zdr", "skipped_policy", "skipped_budget",
	"failed_transient", "failed_permanent",
}

// errorClassVocabulary is the closed, content-free set of context_error_class
// values: the design's eleven, plus zdr_org / zdr_document (CR-T7: a restricted
// document and a ZDR org must be distinguishable) and posture_unavailable (CR-T1
// 503 retention_posture_unavailable, which CR-T7 maps to a deferral).
var errorClassVocabulary = []string{
	"rate_limited", "provider_unavailable", "timeout", "auth",
	"zdr_unavailable", "zdr_org", "zdr_document",
	"residency_unavailable", "posture_unavailable",
	"content_filter", "invalid_output", "doc_missing", "too_long", "internal",
}

// dataPlaneRoot is apps/Data Plane v2, derived from this file's location so the
// tests do not depend on the working directory.
func dataPlaneRoot(t testing.TB) string {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("cannot locate the test source file")
	}
	// <root>/tests/contract/postgres/sqlfiles_test.go
	return filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", ".."))
}

func migrationsDir(t testing.TB) string {
	t.Helper()
	return filepath.Join(dataPlaneRoot(t), "infra", "postgres", "migrations")
}

// readSQL reads a migration with line endings normalised: working trees on
// Windows check these files out with CRLF, CI checks them out with LF, and a
// contract must hold on both.
func readSQL(t testing.TB, name string) string {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(migrationsDir(t), name))
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	return strings.ReplaceAll(string(raw), "\r\n", "\n")
}

var lineComment = regexp.MustCompile(`(?m)--[^\n]*$`)

// code drops `--` line comments so the lint looks at statements, not prose.
// None of the migration's string literals contains `--`.
func code(sql string) string {
	return lineComment.ReplaceAllString(sql, "")
}

// matches returns capture group 1 of every match of re in s.
func matches(re *regexp.Regexp, s string) []string {
	var out []string
	for _, m := range re.FindAllStringSubmatch(s, -1) {
		out = append(out, m[1])
	}
	return out
}

func quotedTokens(s string) []string {
	return matches(regexp.MustCompile(`'([a-z_]+)'`), s)
}

func sortedUnique(in []string) []string {
	seen := map[string]bool{}
	var out []string
	for _, s := range in {
		if !seen[s] {
			seen[s] = true
			out = append(out, s)
		}
	}
	sort.Strings(out)
	return out
}

func sameSet(a, b []string) bool {
	a, b = sortedUnique(a), sortedUnique(b)
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func diffSets(want, got []string) (missing, extra []string) {
	in := func(set []string, s string) bool {
		for _, x := range set {
			if x == s {
				return true
			}
		}
		return false
	}
	for _, w := range want {
		if !in(got, w) {
			missing = append(missing, w)
		}
	}
	for _, g := range got {
		if !in(want, g) {
			extra = append(extra, g)
		}
	}
	return
}

// constraintCheckBody returns the expression inside `ADD CONSTRAINT <name> CHECK ( ... );`.
func constraintCheckBody(t testing.TB, sql, name string) string {
	t.Helper()
	re := regexp.MustCompile(`(?s)ADD CONSTRAINT ` + regexp.QuoteMeta(name) + `\s+CHECK\s*\((.*?)\);`)
	m := re.FindStringSubmatch(sql)
	if m == nil {
		t.Fatalf("constraint %s not found in the migration", name)
	}
	return m[1]
}

// statementBody returns the text from the first occurrence of startRe to endRe.
func statementBody(t testing.TB, sql, startPattern, endPattern string) string {
	t.Helper()
	re := regexp.MustCompile(`(?s)` + startPattern + `(.*?)` + endPattern)
	m := re.FindStringSubmatch(sql)
	if m == nil {
		t.Fatalf("statement %q ... %q not found in the migration", startPattern, endPattern)
	}
	return m[0]
}
