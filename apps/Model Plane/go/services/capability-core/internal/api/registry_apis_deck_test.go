package api

import (
	"net/http"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/services/capability-core/internal/skillbundle"
)

// F-f09-08: an upload that passes the deck template check while SKILL_DECK_TEMPLATES_ENABLED is on is stamped after the
// write, in a transaction of its own; with the switch off nothing is stamped. A bundle that fails the check is refused
// before anything is written.

// deckTestTheme is a deck theme the contract accepts: no master and no logo, so the bundle needs only the theme file.
const deckTestTheme = `{"version": 1, "name": "Brand", "colors": {"primary": "#112233", "secondary": "#445566", "text": "#000000", "background": "#FFFFFF"}, "fonts": {"heading": "Arial", "body": "Arial"}}`

// deckTestStamps returns the statements of a recorded transaction that stamp a deck template.
func deckTestStamps(tx *bundleTestRecorder) []bundleTestExec {
	var stamps []bundleTestExec
	for _, exec := range tx.execs {
		if strings.Contains(exec.sql, "SET deck_validated_at") {
			stamps = append(stamps, exec)
		}
	}
	return stamps
}

// deckTestIndex is the position of the first statement that contains text, or -1.
func deckTestIndex(tx *bundleTestRecorder, text string) int {
	for i, exec := range tx.execs {
		if strings.Contains(exec.sql, text) {
			return i
		}
	}
	return -1
}

func TestSkillBundleDeckUploadIsStampedOnlyWhileTheSwitchIsOn(t *testing.T) {
	admin := bundleTestAdmin()
	upload := bundleTestZip(t, map[string]string{"SKILL.md": bundleTestSkillMD, "templates/deck-theme.json": deckTestTheme})
	bundle, err := skillbundle.Parse(upload)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}

	t.Setenv(skillbundle.DeckTemplatesEnv, "true")
	on := &bundleTestRecorder{t: t}
	response := serveBundle(t, true, &bundleTestDB{t: t, tx: on}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusCreated {
		t.Fatalf("upload with the switch on = %d (%s), want 201", response.Code, response.Body.String())
	}
	stamps := deckTestStamps(on)
	if len(stamps) != 1 || !slicesContainsString(stamps[0].args, "skill_1") || !slicesContainsString(stamps[0].args, bundle.ManifestSHA256) {
		t.Fatalf("stamps with the switch on = %+v, want one stamp for skill_1 at the bundle's digest", stamps)
	}
	if !on.committed {
		t.Fatal("the stamp did not commit")
	}
	if audit, stamp := deckTestIndex(on, "INSERT INTO registry_audit_log"), deckTestIndex(on, "SET deck_validated_at"); audit < 0 || stamp <= audit {
		t.Fatalf("the stamp is at %d, not after the write's audit row at %d", stamp, audit)
	}

	t.Setenv(skillbundle.DeckTemplatesEnv, "")
	off := &bundleTestRecorder{t: t}
	response = serveBundle(t, true, &bundleTestDB{t: t, tx: off}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", upload)
	if response.Code != http.StatusCreated {
		t.Fatalf("upload with the switch off = %d (%s), want 201", response.Code, response.Body.String())
	}
	if stamps := deckTestStamps(off); len(stamps) != 0 {
		t.Fatalf("stamps with the switch off = %+v, want none", stamps)
	}
}

func TestSkillBundleDeckUploadThatFailsTheCheckWritesNothing(t *testing.T) {
	admin := bundleTestAdmin()
	scripted := bundleTestZip(t, map[string]string{
		"SKILL.md":                  bundleTestSkillMD,
		"templates/deck-theme.json": deckTestTheme,
		"templates/helper.sh":       "#!/bin/sh\necho hi\n",
	})
	t.Setenv(skillbundle.DeckTemplatesEnv, "true")
	tx := &bundleTestRecorder{t: t}
	response := serveBundle(t, true, &bundleTestDB{t: t, tx: tx}, &admin, http.MethodPut, "/api/v1/skills/skill_1/bundle", scripted)
	if response.Code != http.StatusUnprocessableEntity || !strings.Contains(response.Body.String(), "template_executable") {
		t.Fatalf("a program under templates/ = %d (%s), want 422 template_executable", response.Code, response.Body.String())
	}
	if tx.committed || len(tx.execs) != 0 {
		t.Fatalf("a refused bundle wrote %d statements (committed=%v)", len(tx.execs), tx.committed)
	}
}
