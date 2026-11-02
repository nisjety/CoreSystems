package skillbundle

import (
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
	"testing"
)

// postgresReservedKeyWords are the key words that the PostgreSQL SQL Key Words appendix marks
// "reserved" or "reserved (can be function or type)". An unquoted one cannot name a column:
// CREATE TABLE fails with a syntax error at the name (SQLSTATE 42601). BINARY is one of them,
// which is why the skill bundle file flag is is_binary in migration 0021.
var postgresReservedKeyWords = wordSet(`
	ALL ANALYSE ANALYZE AND ANY ARRAY AS ASC ASYMMETRIC AUTHORIZATION BINARY BOTH CASE CAST
	CHECK COLLATE COLLATION COLUMN CONCURRENTLY CONSTRAINT CREATE CROSS CURRENT_CATALOG
	CURRENT_DATE CURRENT_ROLE CURRENT_SCHEMA CURRENT_TIME CURRENT_TIMESTAMP CURRENT_USER
	DEFAULT DEFERRABLE DESC DISTINCT DO ELSE END EXCEPT FALSE FETCH FOR FOREIGN FREEZE FROM
	FULL GRANT GROUP HAVING ILIKE IN INITIALLY INNER INTERSECT INTO IS ISNULL JOIN LATERAL
	LEADING LEFT LIKE LIMIT LOCALTIME LOCALTIMESTAMP NATURAL NOT NOTNULL NULL OFFSET ON ONLY
	OR ORDER OUTER OVERLAPS PLACING PRIMARY REFERENCES RETURNING RIGHT SELECT SESSION_USER
	SIMILAR SOME SYMMETRIC SYSTEM_USER TABLE TABLESAMPLE THEN TO TRAILING TRUE UNION UNIQUE
	USER USING VARIADIC VERBOSE WHEN WHERE WINDOW WITH
`)

// sqlNoise matches the parts of a migration that define no column: comments, string literals
// and $$-quoted bodies. The alternatives are tried left to right, so a "--" inside a
// string literal belongs to the literal.
var sqlNoise = regexp.MustCompile(`(?s)/\*.*?\*/|--[^\n]*|'(?:[^']|'')*'|\$\$.*?\$\$`)

// createTableHead matches a CREATE TABLE statement through the open parenthesis of its
// column list.
var createTableHead = regexp.MustCompile(`(?i)\bCREATE\s+(?:\w+\s+)?TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?[^\s(]+\s*\(`)

// alterTableHead matches the start of an ALTER TABLE statement through the table name.
var alterTableHead = regexp.MustCompile(`(?i)^\s*ALTER\s+TABLE\s+(?:IF\s+EXISTS\s+)?(?:ONLY\s+)?[^\s]+`)

// addClause matches one ADD clause of an ALTER TABLE and captures what it adds.
var addClause = regexp.MustCompile(`(?is)^\s*ADD\s+(?:COLUMN\s+)?(?:IF\s+NOT\s+EXISTS\s+)?(.*)`)

// tableConstraint matches a list item that opens a table constraint rather than a column.
var tableConstraint = regexp.MustCompile(`(?i)^\s*(?:CONSTRAINT\b|PRIMARY\s+KEY\b|FOREIGN\s+KEY\b|UNIQUE\s*\(|CHECK\s*\(|EXCLUDE\s+(?:USING\b|\())`)

// columnName matches the name that opens a column definition. A quoted name keeps its quotes.
var columnName = regexp.MustCompile(`^\s*("(?:[^"]|"")*"|[A-Za-z_][A-Za-z0-9_$]*)`)

// TestMigrationColumnsAreNotReservedWords reads every capability-core up migration. PostgreSQL
// rejects an unquoted reserved word as a column name, which fails the whole migration (0021
// failed this way on binary). The integration tests catch that only by applying the
// migrations, so this check runs without a database.
func TestMigrationColumnsAreNotReservedWords(t *testing.T) {
	paths, err := filepath.Glob(filepath.Join("..", "..", "migrations", "*.up.sql"))
	if err != nil {
		t.Fatalf("list migrations: %v", err)
	}
	if len(paths) == 0 {
		t.Fatal("no migrations under ../../migrations")
	}
	defined := 0
	for _, path := range paths {
		body, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		names := columnNames(string(body))
		defined += len(names)
		for _, name := range reservedNames(names) {
			t.Errorf("%s: column %s is an unquoted PostgreSQL reserved word", filepath.Base(path), name)
		}
	}
	if defined == 0 {
		t.Fatal("no column definitions found in any migration; the scanner is not reading the SQL")
	}
}

// TestMigrationColumnScanner pins the scanner on a migration shaped like the real ones. It
// must find every column, skip constraints, comments, literals and quoted names, and flag only
// the unquoted reserved words.
func TestMigrationColumnScanner(t *testing.T) {
	const sql = `-- binary and offset in a comment are not columns
CREATE TABLE IF NOT EXISTS widgets (
    id          TEXT PRIMARY KEY,
    binary      BOOLEAN NOT NULL DEFAULT FALSE,
    "order"     INTEGER NOT NULL DEFAULT 0,
    note        TEXT NOT NULL DEFAULT 'a, binary (quoted',
    CONSTRAINT widgets_note_check CHECK (note ~ '^[a-z]{1,8}$'),
    PRIMARY KEY (id)
);
ALTER TABLE widgets ADD COLUMN IF NOT EXISTS offset INTEGER NOT NULL DEFAULT 0, ADD COLUMN is_ok BOOLEAN;
ALTER TABLE widgets ADD CONSTRAINT widgets_offset_check CHECK (offset >= 0);
`
	columns := columnNames(sql)
	if want := []string{"id", "binary", `"order"`, "note", "offset", "is_ok"}; !reflect.DeepEqual(columns, want) {
		t.Errorf("columns = %q, want %q", columns, want)
	}
	if want := []string{"binary", "offset"}; !reflect.DeepEqual(reservedNames(columns), want) {
		t.Errorf("reserved = %q, want %q", reservedNames(columns), want)
	}
}

// columnNames returns the name of every column a migration defines: each column of a CREATE
// TABLE column list and each column an ALTER TABLE adds. Comments and string literals are
// skipped, and a quoted name is returned with its quotes.
func columnNames(sql string) []string {
	sql = sqlNoise.ReplaceAllString(sql, " ")
	var names []string
	for _, head := range createTableHead.FindAllStringIndex(sql, -1) {
		for _, item := range topLevelItems(parenthesized(sql, head[1]-1)) {
			if name := columnName.FindStringSubmatch(item); name != nil && !tableConstraint.MatchString(item) {
				names = append(names, name[1])
			}
		}
	}
	for _, statement := range strings.Split(sql, ";") {
		head := alterTableHead.FindStringIndex(statement)
		if head == nil {
			continue
		}
		for _, item := range topLevelItems(statement[head[1]:]) {
			add := addClause.FindStringSubmatch(item)
			if add == nil || tableConstraint.MatchString(add[1]) {
				continue
			}
			if name := columnName.FindStringSubmatch(add[1]); name != nil {
				names = append(names, name[1])
			}
		}
	}
	return names
}

// reservedNames returns the names that are unquoted PostgreSQL reserved key words.
func reservedNames(names []string) []string {
	var reserved []string
	for _, name := range names {
		if !strings.HasPrefix(name, `"`) && postgresReservedKeyWords[strings.ToUpper(name)] {
			reserved = append(reserved, name)
		}
	}
	return reserved
}

// topLevelItems splits a comma-separated list at the commas outside any parentheses.
func topLevelItems(list string) []string {
	var items []string
	depth, start := 0, 0
	for i := 0; i < len(list); i++ {
		switch list[i] {
		case '(':
			depth++
		case ')':
			depth--
		case ',':
			if depth == 0 {
				items = append(items, list[start:i])
				start = i + 1
			}
		}
	}
	return append(items, list[start:])
}

// parenthesized returns the text between the open parenthesis at index open and its match.
func parenthesized(text string, open int) string {
	depth := 0
	for i := open; i < len(text); i++ {
		switch text[i] {
		case '(':
			depth++
		case ')':
			depth--
			if depth == 0 {
				return text[open+1 : i]
			}
		}
	}
	return text[open+1:]
}

// wordSet returns the words of a whitespace-separated list as a set.
func wordSet(list string) map[string]bool {
	set := make(map[string]bool)
	for _, word := range strings.Fields(list) {
		set[word] = true
	}
	return set
}
