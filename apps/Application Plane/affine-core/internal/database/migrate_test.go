package database

import (
	"context"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

func TestMigrationFileNamesSortsUpMigrations(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	writeTestFile(t, dir, "002_second.up.sql", "SELECT 2;")
	writeTestFile(t, dir, "001_first.up.sql", "SELECT 1;")
	writeTestFile(t, dir, "003_ignore.down.sql", "SELECT 3;")

	files, err := migrationFileNames(dir)
	if err != nil {
		t.Fatalf("migrationFileNames() error = %v", err)
	}

	got := make([]string, 0, len(files))
	for _, file := range files {
		got = append(got, file.name)
	}

	want := []string{"001_first.up.sql", "002_second.up.sql"}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("migrationFileNames() = %v, want %v", got, want)
	}
}

func TestRunMigrationsAppliesOnlyMissingFiles(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	firstSQL := "SELECT 'first';"
	secondSQL := "SELECT 'second';"
	writeTestFile(t, dir, "002_second.up.sql", secondSQL)
	writeTestFile(t, dir, "001_first.up.sql", firstSQL)

	exec := &migrationExecutorMock{
		applied: map[string]bool{
			"001_first.up.sql": true,
		},
	}

	if err := runMigrations(context.Background(), exec, dir); err != nil {
		t.Fatalf("runMigrations() error = %v", err)
	}

	wantChecks := []string{"001_first.up.sql", "002_second.up.sql"}
	if !reflect.DeepEqual(exec.checkedNames, wantChecks) {
		t.Fatalf("checked migrations = %v, want %v", exec.checkedNames, wantChecks)
	}

	if containsSQL(exec.execSQL, firstSQL) {
		t.Fatalf("expected applied migration %q to be skipped, execs = %v", firstSQL, exec.execSQL)
	}
	if !containsSQL(exec.execSQL, secondSQL) {
		t.Fatalf("expected pending migration %q to run, execs = %v", secondSQL, exec.execSQL)
	}
	if !containsArg(exec.insertedNames, "002_second.up.sql") {
		t.Fatalf("expected pending migration to be recorded, inserted = %v", exec.insertedNames)
	}
	if containsArg(exec.insertedNames, "001_first.up.sql") {
		t.Fatalf("did not expect applied migration to be recorded again, inserted = %v", exec.insertedNames)
	}
	if exec.beginCount != 1 {
		t.Fatalf("expected one transaction for the pending migration, got %d", exec.beginCount)
	}
}

type migrationExecutorMock struct {
	applied      map[string]bool
	checkedNames []string
	execSQL      []string
	insertedNames []string
	beginCount   int
}

func (m *migrationExecutorMock) Exec(_ context.Context, sql string, _ ...any) (pgconn.CommandTag, error) {
	m.execSQL = append(m.execSQL, sql)
	return pgconn.CommandTag{}, nil
}

func (m *migrationExecutorMock) QueryRow(_ context.Context, _ string, args ...any) pgx.Row {
	name, _ := args[0].(string)
	m.checkedNames = append(m.checkedNames, name)
	return boolRow{value: m.applied[name]}
}

func (m *migrationExecutorMock) Begin(_ context.Context) (migrationTx, error) {
	m.beginCount++
	return &migrationTxMock{parent: m}, nil
}

type migrationTxMock struct {
	parent *migrationExecutorMock
}

func (m *migrationTxMock) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	m.parent.execSQL = append(m.parent.execSQL, sql)
	if len(args) > 0 {
		if name, ok := args[0].(string); ok {
			m.parent.insertedNames = append(m.parent.insertedNames, name)
		}
	}
	return pgconn.CommandTag{}, nil
}

func (m *migrationTxMock) Commit(context.Context) error {
	return nil
}

func (m *migrationTxMock) Rollback(context.Context) error {
	return nil
}

type boolRow struct {
	value bool
	err   error
}

func (r boolRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	if len(dest) != 1 {
		return nil
	}
	ptr, _ := dest[0].(*bool)
	if ptr != nil {
		*ptr = r.value
	}
	return nil
}

func writeTestFile(t *testing.T, dir, name, content string) {
	t.Helper()
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("write test file %s: %v", name, err)
	}
}

func containsSQL(values []string, target string) bool {
	for _, value := range values {
		if value == target {
			return true
		}
	}
	return false
}

func containsArg(values []string, target string) bool {
	for _, value := range values {
		if value == target {
			return true
		}
	}
	return false
}