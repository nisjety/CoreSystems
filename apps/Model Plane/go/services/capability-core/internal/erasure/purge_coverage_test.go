package erasure

import (
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"
)

// The migrations are the source of truth for which tables hold an organization's rows. This scan does not need a
// database, so it runs in the default test lane and fails when a migration adds an org-scoped table, or a child of one,
// that the purge does not delete.
var (
	createTableStart = regexp.MustCompile(`(?m)^CREATE TABLE IF NOT EXISTS (\w+) \(`)
	createTableEnd   = "\n);"
	orgColumnLine    = regexp.MustCompile(`(?m)^\s*org_id\s`)
	addOrgColumn     = regexp.MustCompile(`ALTER TABLE (\w+)\s+ADD COLUMN IF NOT EXISTS org_id\b`)
	referencesTable  = regexp.MustCompile(`REFERENCES (\w+)`)
)

// sessionCoreOwned names the tables the purge deletes that a session-core migration creates on this same database. They
// are absent from capability-core's migrations, so the scan cannot see them.
var sessionCoreOwned = map[string]bool{"space_watches": true, "space_watch_events": true}

type migratedTable struct {
	hasOrgID   bool
	references map[string]bool
}

// readMigratedTables scans every capability-core *.up.sql in name order, the order docker-entrypoint.sh applies them.
func readMigratedTables(t *testing.T) map[string]*migratedTable {
	t.Helper()
	files, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil {
		t.Fatalf("list migrations: %v", err)
	}
	if len(files) == 0 {
		t.Fatal("no capability-core migrations found: the scan would prove nothing")
	}
	sort.Strings(files)

	tables := map[string]*migratedTable{}
	ensure := func(name string) *migratedTable {
		if tables[name] == nil {
			tables[name] = &migratedTable{references: map[string]bool{}}
		}
		return tables[name]
	}
	for _, file := range files {
		data, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("read %s: %v", file, err)
		}
		text := string(data)
		for _, loc := range createTableStart.FindAllStringSubmatchIndex(text, -1) {
			name := text[loc[2]:loc[3]]
			body := text[loc[1]:]
			end := strings.Index(body, createTableEnd)
			if end < 0 {
				t.Fatalf("%s: CREATE TABLE %s has no closing line", filepath.Base(file), name)
			}
			block := body[:end]
			table := ensure(name)
			if orgColumnLine.MatchString(block) {
				table.hasOrgID = true
			}
			for _, ref := range referencesTable.FindAllStringSubmatch(block, -1) {
				table.references[ref[1]] = true
			}
		}
		for _, match := range addOrgColumn.FindAllStringSubmatch(text, -1) {
			ensure(match[1]).hasOrgID = true
		}
	}
	return tables
}

func purgeIndex() map[string]int {
	index := map[string]int{}
	for i, step := range orgPurgeSteps {
		index[step.table] = i
	}
	return index
}

// TestEveryOrgScopedCapabilityCoreTableIsPurged fails when a migration adds an org_id table that the purge leaves out.
func TestEveryOrgScopedCapabilityCoreTableIsPurged(t *testing.T) {
	tables := readMigratedTables(t)
	index := purgeIndex()

	var orgTables []string
	for name, table := range tables {
		if table.hasOrgID {
			orgTables = append(orgTables, name)
		}
	}
	sort.Strings(orgTables)
	// Twenty-three tables carry an org_id today (0001 to 0045). A scan that finds far fewer has lost a pattern.
	if len(orgTables) < 20 {
		t.Fatalf("the scan found %d org-scoped tables (%v); expected at least 20", len(orgTables), orgTables)
	}
	for _, name := range orgTables {
		if _, ok := index[name]; !ok {
			t.Errorf("table %s carries org_id in a capability-core migration, but no purge step deletes it", name)
		}
	}
}

// TestPurgeDeletesChildrenBeforeTheirParents fails when a child table that a migration references is not purged, or is
// purged after the parent it references, which would fail the delete on a foreign key.
func TestPurgeDeletesChildrenBeforeTheirParents(t *testing.T) {
	tables := readMigratedTables(t)
	index := purgeIndex()

	for child, table := range tables {
		for parent := range table.references {
			childAt, childPurged := index[child]
			parentAt, parentPurged := index[parent]
			if !parentPurged {
				continue
			}
			if !childPurged {
				t.Errorf("table %s references purged table %s, but no purge step deletes it", child, parent)
				continue
			}
			if childAt >= parentAt {
				t.Errorf("purge deletes %s (step %d) after %s (step %d), but %s references %s",
					child, childAt, parent, parentAt, child, parent)
			}
		}
	}
}

// TestEveryPurgeStepNamesARealTable fails when a step names a table that no migration creates and that is not one of the
// session-core tables named on purpose. It also fails on a duplicated step, which would count the same rows twice.
func TestEveryPurgeStepNamesARealTable(t *testing.T) {
	tables := readMigratedTables(t)
	seen := map[string]bool{}
	for _, step := range orgPurgeSteps {
		if seen[step.table] {
			t.Errorf("table %s has more than one purge step", step.table)
		}
		seen[step.table] = true
		if _, ok := tables[step.table]; !ok && !sessionCoreOwned[step.table] {
			t.Errorf("purge step for %s names a table no capability-core migration creates", step.table)
		}
		if !strings.Contains(step.sql, "DELETE FROM "+step.table+" ") {
			t.Errorf("purge step for %s does not delete from that table: %s", step.table, step.sql)
		}
		if !strings.Contains(step.sql, "$1") {
			t.Errorf("purge step for %s does not scope its delete by the organization id", step.table)
		}
	}
}
