package zdr

import (
	"crypto/sha256"
	"encoding/hex"
	"io/fs"
	"strings"
	"testing"
	"testing/fstest"
)

// wordingSnapshotPins are the approved snapshots of every acknowledgement text, by
// locale/key/variant. A changed sentence changes its hash here, so a wording edit cannot land
// without this list (and the version it sits in) being changed on purpose.
var wordingSnapshotPins = map[string]string{
	"en/chatgpt_subscription_not_supported/first_use":   "fd792d2fa2cbcd60db66fac37a804a58d0ad591b6361618f8d6fd6d98ef4d62f",
	"en/chatgpt_subscription_not_supported/org_zdr_off": "0164de083e9fbf0c7f89476c11ce33b8d1e0b7a07b611b6328c0107bb3038851",
	"en/chatgpt_subscription_not_supported/org_zdr_on":  "8441c0f7606ae46febd6ea0498f0934253ce3cac1d7ef6ba14336bc8678f52bc",
	"en/connection_cannot_verify/first_use":             "8d5ec3b92ada249cf87e6a8862400e79a8b14698b10795dc3420ba762c915367",
	"en/connection_cannot_verify/org_zdr_off":           "2cb0c772f5e2b32ee095b30ded173557e34449dafbbdca05c692e162aa42c4c2",
	"en/connection_cannot_verify/org_zdr_on":            "3a27e8ffc07398330619fe31cb238ce13623967de6008587ff96fbe6284eb106",
	"en/connection_supported/org_zdr_off":               "5fbfe0740f37ab9cc8bfc2859b019028b8fff937a2948e56e8d8e8d31442d809",
	"en/connection_supported/org_zdr_on":                "bd6efd9aa60b9f2976bebaba0b5d4c41f03baa1642be44ac2cd674b0d1e22374",
	"no/chatgpt_subscription_not_supported/first_use":   "a45d558038a27b3b42548ba7494f93e260bd8da97fa73e05eda58c038b5d757a",
	"no/chatgpt_subscription_not_supported/org_zdr_off": "94690fab5c7fc974fe7ee7720a776957ade272b211be0346c00951629e838d37",
	"no/chatgpt_subscription_not_supported/org_zdr_on":  "a5d374ee00058ea2188fe357425c3555c001cfbd5067b0294deede6ba425e72c",
	"no/connection_cannot_verify/first_use":             "575a4a25bf87a399c159c326c02c8fdcbc4928837d0d3a2d12b819f63e9abf4b",
	"no/connection_cannot_verify/org_zdr_off":           "c89acb290464f1d561b67365009dcd480d51db3c99cf4593b0038d24db149e87",
	"no/connection_cannot_verify/org_zdr_on":            "51a87c7306bcf73b96fe0e378462484a5349d906624debe23efb89caaac05f34",
	"no/connection_supported/org_zdr_off":               "c7444fc692771997ceafc1a429d438344a46f557ab51dba88364291d9f7557bd",
	"no/connection_supported/org_zdr_on":                "b860c0129420c2d98924f290e42697e107f235e0b2cc9e34e76fea33a7f38aa2",
}

func loadDefaultWordingForTest(t *testing.T) *Wording {
	t.Helper()
	wording, err := DefaultWording()
	if err != nil {
		t.Fatalf("DefaultWording: %v", err)
	}
	return wording
}

func contractKeysAndVariants(t *testing.T) (map[string][]string, []string) {
	t.Helper()
	assessor, err := Default()
	if err != nil {
		t.Fatalf("Default: %v", err)
	}
	variants := assessor.WordingVariants()
	if len(variants) == 0 {
		t.Fatal("the capability contract declares no wording keys")
	}
	return variants, []string{"no", "en"}
}

func TestEveryContractWordingKeyVariantAndLocaleHasACompleteText(t *testing.T) {
	wording := loadDefaultWordingForTest(t)
	variants, locales := contractKeysAndVariants(t)
	count := 0
	for key, list := range variants {
		for _, variant := range list {
			for _, locale := range locales {
				text, err := wording.Lookup(key, variant, locale)
				if err != nil {
					t.Fatalf("Lookup(%s, %s, %s): %v", key, variant, locale, err)
				}
				if strings.TrimSpace(text.Title) == "" || strings.TrimSpace(text.Body) == "" ||
					strings.TrimSpace(text.ContinueLabel) == "" || strings.TrimSpace(text.CancelLabel) == "" {
					t.Fatalf("%s/%s/%s has an empty title, body or button label", key, variant, locale)
				}
				if text.Version != wording.Version() || text.Locale != locale || text.Key != key || text.Variant != variant {
					t.Fatalf("%s/%s/%s carries the wrong identity: %+v", key, variant, locale, text)
				}
				if text.Snapshot != text.Title+"\n\n"+text.Body {
					t.Fatalf("%s/%s/%s: the snapshot is not title, blank line, body", key, variant, locale)
				}
				sum := sha256.Sum256([]byte(text.Snapshot))
				if text.SHA256 != hex.EncodeToString(sum[:]) {
					t.Fatalf("%s/%s/%s: sha256 is not the hash of the snapshot", key, variant, locale)
				}
				count++
			}
		}
	}
	// chatgpt_subscription_not_supported (3 variants) + connection_cannot_verify (3) +
	// connection_supported (2), in each of two locales.
	if count != 16 {
		t.Fatalf("expected 16 complete texts (8 per locale), found %d", count)
	}
}

func TestWordingRefusesAnUnknownKeyVariantOrLocale(t *testing.T) {
	wording := loadDefaultWordingForTest(t)
	for _, probe := range [][3]string{
		{"no_such_key", "org_zdr_on", "no"},
		{"connection_supported", "first_use", "no"},
		{"chatgpt_subscription_not_supported", "org_zdr_on", "sv"},
		{"chatgpt_subscription_not_supported", "", "en"},
	} {
		if _, err := wording.Lookup(probe[0], probe[1], probe[2]); err == nil {
			t.Fatalf("Lookup(%v) succeeded; it must refuse a text that does not exist", probe)
		}
	}
}

func TestAcknowledgementTextsStateOpenAIAndTheSkippedBackgroundJobsInBothLanguages(t *testing.T) {
	wording := loadDefaultWordingForTest(t)
	for _, key := range []string{"chatgpt_subscription_not_supported", "connection_cannot_verify"} {
		for _, variant := range []string{"org_zdr_on", "first_use"} {
			no, err := wording.Lookup(key, variant, "no")
			if err != nil {
				t.Fatal(err)
			}
			// The subscription's recipient is OpenAI by name; a key's recipient is its own provider.
			recipient := map[string]string{"chatgpt_subscription_not_supported": "OpenAI", "connection_cannot_verify": "leverandøren"}[key]
			if !strings.Contains(no.Snapshot, recipient) || !strings.Contains(no.Snapshot, "kunnskap") || !strings.Contains(no.Snapshot, "verktøy") {
				t.Fatalf("NO %s/%s does not say retrieved organisation knowledge and tool results go to %s", key, variant, recipient)
			}
			if !strings.Contains(no.Snapshot, "bakgrunnsjobber") || !strings.Contains(no.Snapshot, "kjøres ikke") {
				t.Fatalf("NO %s/%s does not say the background jobs are skipped", key, variant)
			}
			en, err := wording.Lookup(key, variant, "en")
			if err != nil {
				t.Fatal(err)
			}
			enRecipient := map[string]string{"chatgpt_subscription_not_supported": "OpenAI", "connection_cannot_verify": "the provider"}[key]
			if !strings.Contains(en.Snapshot, enRecipient) || !strings.Contains(en.Snapshot, "organisation knowledge") || !strings.Contains(en.Snapshot, "tool results") {
				t.Fatalf("EN %s/%s does not say retrieved organisation knowledge and tool results go to %s", key, variant, enRecipient)
			}
			if !strings.Contains(en.Snapshot, "background jobs") || !strings.Contains(en.Snapshot, "not run") {
				t.Fatalf("EN %s/%s does not say the background jobs are not run", key, variant)
			}
		}
	}
}

func TestOrgWarningWordingCarriesTheContradictionForBothLanguages(t *testing.T) {
	wording := loadDefaultWordingForTest(t)
	on, err := wording.Lookup("chatgpt_subscription_not_supported", "org_zdr_on", "no")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(on.Snapshot, "Organisasjonen din har Zero Data Retention (ZDR) på") {
		t.Fatalf("NO org_zdr_on must state that the organisation has ZDR on, got %q", on.Snapshot)
	}
	onEN, err := wording.Lookup("chatgpt_subscription_not_supported", "org_zdr_on", "en")
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(onEN.Snapshot, "Your organisation has Zero Data Retention (ZDR) turned on") {
		t.Fatalf("EN org_zdr_on must state that the organisation has ZDR on, got %q", onEN.Snapshot)
	}
}

func TestWordingSnapshotsAreExactlyTheApprovedOnes(t *testing.T) {
	wording := loadDefaultWordingForTest(t)
	variants, locales := contractKeysAndVariants(t)
	if len(wordingSnapshotPins) != 16 {
		t.Fatalf("the snapshot pins cover %d texts, want 16", len(wordingSnapshotPins))
	}
	for key, list := range variants {
		for _, variant := range list {
			for _, locale := range locales {
				id := locale + "/" + key + "/" + variant
				text, err := wording.Lookup(key, variant, locale)
				if err != nil {
					t.Fatal(err)
				}
				if want := wordingSnapshotPins[id]; text.SHA256 != want {
					t.Fatalf("%s: snapshot sha256 %s, approved %s; a wording change needs a new version and new pins", id, text.SHA256, want)
				}
			}
		}
	}
}

// copyEmbeddedWording returns the embedded texts as a writable in-memory file system, so a test
// can tamper with one file, add one or remove one and watch the loader refuse it.
func copyEmbeddedWording(t *testing.T) fstest.MapFS {
	t.Helper()
	out := fstest.MapFS{}
	err := fs.WalkDir(wordingFS, wordingRoot, func(path string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		data, readErr := fs.ReadFile(wordingFS, path)
		if readErr != nil {
			return readErr
		}
		out[path] = &fstest.MapFile{Data: data}
		return nil
	})
	if err != nil {
		t.Fatalf("copy embedded wording: %v", err)
	}
	return out
}

func TestTamperedWordingIsRefusedAtLoad(t *testing.T) {
	assessor, err := Default()
	if err != nil {
		t.Fatal(err)
	}
	if _, err := loadWordingFrom(copyEmbeddedWording(t), assessor); err != nil {
		t.Fatalf("the untampered embedded wording must load: %v", err)
	}

	edited := copyEmbeddedWording(t)
	edited["texts/v1/no/connection_supported.org_zdr_on.txt"].Data = append([]byte(nil), append(edited["texts/v1/no/connection_supported.org_zdr_on.txt"].Data, []byte("Ekstra.\n")...)...)
	if _, err := loadWordingFrom(edited, assessor); err == nil || !strings.Contains(err.Error(), "pinned") {
		t.Fatalf("an edited text must be refused against its pin, got %v", err)
	}

	extra := copyEmbeddedWording(t)
	extra["texts/v1/no/unlisted.org_zdr_on.txt"] = &fstest.MapFile{Data: []byte("title: x\ncontinue: x\ncancel: x\n\nx\n")}
	if _, err := loadWordingFrom(extra, assessor); err == nil {
		t.Fatal("a text that the index does not list must be refused")
	}

	missing := copyEmbeddedWording(t)
	delete(missing, "texts/v1/en/connection_supported.org_zdr_off.txt")
	if _, err := loadWordingFrom(missing, assessor); err == nil {
		t.Fatal("a text the index lists but the tree lacks must be refused")
	}
}

func TestTheIndexCannotNameAnUnknownVersionAsCurrent(t *testing.T) {
	assessor, err := Default()
	if err != nil {
		t.Fatal(err)
	}
	broken := copyEmbeddedWording(t)
	broken[wordingIndexPath].Data = []byte(strings.Replace(string(broken[wordingIndexPath].Data), `"current": "v1"`, `"current": "v9"`, 1))
	if _, err := loadWordingFrom(broken, assessor); err == nil {
		t.Fatal("an index whose current version does not exist must be refused")
	}
}
