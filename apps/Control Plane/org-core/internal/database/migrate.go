package database

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func RunMigrations(ctx context.Context, db *DB, migrationsDir string) error {
	entries, err := os.ReadDir(migrationsDir)
	if err != nil {
		return fmt.Errorf("read migrations dir: %w", err)
	}

	var files []string
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		name := e.Name()
		if strings.HasSuffix(name, ".up.sql") {
			files = append(files, filepath.Join(migrationsDir, name))
		}
	}
	sort.Strings(files)

	for _, file := range files {
		sqlBytes, err := os.ReadFile(file)
		if err != nil {
			return fmt.Errorf("read migration %s: %w", file, err)
		}

		// Execute each DDL statement individually so that schema changes from
		// one statement are visible to subsequent statements in the same file.
		stmts := splitStatements(string(sqlBytes))
		for _, stmt := range stmts {
			if stmt == "" {
				continue
			}
			if _, err := db.Pool.Exec(ctx, stmt); err != nil {
				return fmt.Errorf("execute migration %s: %w", file, err)
			}
		}
	}

	return nil
}

// splitStatements splits a SQL script on semicolons while correctly handling
// single-quoted strings, dollar-quoted strings ($$ ... $$ or $tag$ ... $tag$),
// line comments (--) and block comments (/* */).
func splitStatements(sql string) []string {
	var stmts []string
	var buf strings.Builder
	i := 0
	runes := []rune(sql)
	n := len(runes)

	peek := func(offset int) rune {
		if i+offset < n {
			return runes[i+offset]
		}
		return 0
	}

	for i < n {
		ch := runes[i]

		// Line comment: skip until newline
		if ch == '-' && peek(1) == '-' {
			for i < n && runes[i] != '\n' {
				buf.WriteRune(runes[i])
				i++
			}
			continue
		}

		// Block comment: skip /* ... */
		if ch == '/' && peek(1) == '*' {
			buf.WriteRune(ch)
			buf.WriteRune(runes[i+1])
			i += 2
			for i < n {
				if runes[i] == '*' && i+1 < n && runes[i+1] == '/' {
					buf.WriteRune(runes[i])
					buf.WriteRune(runes[i+1])
					i += 2
					break
				}
				buf.WriteRune(runes[i])
				i++
			}
			continue
		}

		// Single-quoted string: copy until closing quote (handle '' escapes)
		if ch == '\'' {
			buf.WriteRune(ch)
			i++
			for i < n {
				c := runes[i]
				buf.WriteRune(c)
				i++
				if c == '\'' {
					if i < n && runes[i] == '\'' {
						// Escaped quote: ''
						buf.WriteRune(runes[i])
						i++
					} else {
						break
					}
				}
			}
			continue
		}

		// Dollar-quoted string: $tag$ ... $tag$ (tag may be empty → $$)
		if ch == '$' {
			// Collect the dollar-quote tag
			j := i + 1
			for j < n && runes[j] != '$' && runes[j] != '\n' {
				j++
			}
			if j < n && runes[j] == '$' {
				tag := string(runes[i : j+1]) // e.g. "$$" or "$body$"
				// Write the opening tag
				for k := i; k <= j; k++ {
					buf.WriteRune(runes[k])
				}
				i = j + 1
				// Scan for the closing tag
				for i < n {
					if runes[i] == '$' {
						end := i + len([]rune(tag))
						if end <= n && string(runes[i:end]) == tag {
							for k := i; k < end; k++ {
								buf.WriteRune(runes[k])
							}
							i = end
							break
						}
					}
					buf.WriteRune(runes[i])
					i++
				}
				continue
			}
		}

		// Semicolon terminates a statement
		if ch == ';' {
			stmt := strings.TrimSpace(buf.String())
			if stmt != "" {
				stmts = append(stmts, stmt)
			}
			buf.Reset()
			i++
			continue
		}

		buf.WriteRune(ch)
		i++
	}

	// Trailing statement without semicolon
	if stmt := strings.TrimSpace(buf.String()); stmt != "" {
		stmts = append(stmts, stmt)
	}

	return stmts
}

