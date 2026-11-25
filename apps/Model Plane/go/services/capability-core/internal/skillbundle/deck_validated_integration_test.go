//go:build integration

package skillbundle

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// F-f09-08 against a real Postgres: the stamp narrows the list's deck_template flag, and the startup sweep stamps a
// clean bundle, leaves a scripted one alone and does nothing on a second run. Needs the DB harness (testcontainers).

// applyDeckMigrations adds what the deck stamp needs on top of setupBundleDB's schema: the retention state column
// (0037, which the list reads) and the stamp itself (0046).
func applyDeckMigrations(t *testing.T, pool *pgxpool.Pool) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	for _, migration := range []string{"0037_skill_bundle_retention.up.sql", "0046_skill_bundle_deck_validated.up.sql"} {
		sql, err := os.ReadFile(filepath.Join("..", "..", "migrations", migration))
		if err != nil {
			t.Fatalf("read migration %s: %v", migration, err)
		}
		if _, err := pool.Exec(ctx, string(sql)); err != nil {
			t.Fatalf("apply migration %s: %v", migration, err)
		}
	}
}

// deckFlag reports the deck_template flag the list gives the skill's bundle, failing when the skill is not listed.
func deckFlag(t *testing.T, store *Store, skillID string) bool {
	t.Helper()
	summaries, err := store.List(context.Background(), "org-a", nil)
	if err != nil {
		t.Fatalf("List: %v", err)
	}
	for _, summary := range summaries {
		if summary.SkillID == skillID {
			return summary.DeckTemplate
		}
	}
	t.Fatalf("skill %s is not listed", skillID)
	return false
}

func TestDeckTemplateIsListedOnlyOnceStamped(t *testing.T) {
	ctx := context.Background()
	pool := setupBundleDB(t)
	applyDeckMigrations(t, pool)
	store := NewStore(pool)
	clean := templateBundle(t, themeFixture(t))
	if _, err := store.Put(ctx, "org-a", "skill_clean", "user-admin", clean); err != nil {
		t.Fatalf("Put: %v", err)
	}
	if deckFlag(t, store, "skill_clean") {
		t.Fatal("an unstamped bundle with a theme is listed as a deck template")
	}
	if err := store.MarkDeckValidated(ctx, "org-a", "skill_clean", clean.ManifestSHA256); err != nil {
		t.Fatalf("MarkDeckValidated: %v", err)
	}
	if !deckFlag(t, store, "skill_clean") {
		t.Fatal("a stamped bundle with a theme is not listed as a deck template")
	}
}

func TestDeckSweepStampsACleanBundleLeavesAScriptedOneAndIsIdempotent(t *testing.T) {
	ctx := context.Background()
	pool := setupBundleDB(t)
	applyDeckMigrations(t, pool)
	store := NewStore(pool)
	clean := templateBundle(t, themeFixture(t))
	scripted := templateBundle(t, themeFixture(t), zipEntry{name: "templates/helper", body: []byte("#!/bin/sh\necho hi\n")})
	if _, err := store.Put(ctx, "org-a", "skill_clean", "user-admin", clean); err != nil {
		t.Fatalf("Put clean: %v", err)
	}
	if _, err := store.Put(ctx, "org-a", "skill_scripted", "user-admin", scripted); err != nil {
		t.Fatalf("Put scripted: %v", err)
	}
	checked, passed, failed, err := store.RevalidateDeckTemplates(ctx)
	if err != nil || checked != 2 || passed != 1 || failed != 1 {
		t.Fatalf("first sweep = (%d, %d, %d, %v), want (2, 1, 1, nil)", checked, passed, failed, err)
	}
	if !deckFlag(t, store, "skill_clean") {
		t.Fatal("the swept clean bundle is not listed as a deck template")
	}
	if deckFlag(t, store, "skill_scripted") {
		t.Fatal("a bundle with a program under templates/ is listed as a deck template")
	}
	checked, passed, failed, err = store.RevalidateDeckTemplates(ctx)
	if err != nil || checked != 1 || passed != 0 || failed != 1 {
		t.Fatalf("second sweep = (%d, %d, %d, %v), want only the failing bundle checked again", checked, passed, failed, err)
	}
}
