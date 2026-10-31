package pg

import (
	"fmt"
	"os"
	"os/exec"
	"sort"
	"strings"
	"testing"
	"time"
)

// This test creates and removes only its own uniquely named disposable DB.
// It uses the Postgres owner's local socket, never a provider/Integration DB.
func TestMigrationsWithoutRuntimeOwnedTables(t *testing.T) {
	container := os.Getenv("QUARRY_TEST_POSTGRES_CONTAINER")
	if container == "" {
		t.Skip("QUARRY_TEST_POSTGRES_CONTAINER unset")
	}
	user := os.Getenv("QUARRY_TEST_POSTGRES_USER")
	if user == "" {
		user = "ingestion_user"
	}
	database := fmt.Sprintf("quarry_control_migration_test_%d", time.Now().UnixNano())
	run := func(input string, args ...string) ([]byte, error) {
		command := exec.Command("docker", append([]string{"exec", "-i", container}, args...)...)
		command.Stdin = strings.NewReader(input)
		return command.CombinedOutput()
	}
	if output, err := run("", "createdb", "-U", user, database); err != nil {
		t.Fatalf("create isolated test database: %v: %s", err, output)
	}
	t.Cleanup(func() {
		if output, err := run("", "dropdb", "-U", user, database); err != nil {
			t.Errorf("remove isolated test database: %v: %s", err, output)
		}
	})
	entries, err := migrationFS.ReadDir("migrations")
	if err != nil {
		t.Fatal(err)
	}
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".sql") {
			names = append(names, entry.Name())
		}
	}
	sort.Strings(names)
	for _, name := range names {
		migration, err := migrationFS.ReadFile("migrations/" + name)
		if err != nil {
			t.Fatal(err)
		}
		if output, err := run(string(migration), "psql", "-X", "-v", "ON_ERROR_STOP=1", "-U", user, "-d", database); err != nil {
			t.Fatalf("fresh control-only database migration %s: %v: %s", name, err, output)
		}
	}
	check := `SELECT to_regclass('public.snapshots_v2') IS NOT NULL AND to_regclass('public.quarry_request_queues') IS NULL;`
	output, err := run(check, "psql", "-X", "-At", "-v", "ON_ERROR_STOP=1", "-U", user, "-d", database)
	if err != nil || strings.TrimSpace(string(output)) != "t" {
		t.Fatalf("control snapshots must exist without creating runtime queues: %v: %s", err, output)
	}
}

func TestSnapshotMigrationDoesNotAlterRuntimeOwnedQueues(t *testing.T) {
	migration, err := migrationFS.ReadFile("migrations/012_snapshots_v2.sql")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(strings.ToLower(string(migration)), "alter table quarry_request_queues") {
		t.Fatal("control migrations must not alter runtime-owned queue tables")
	}
}
