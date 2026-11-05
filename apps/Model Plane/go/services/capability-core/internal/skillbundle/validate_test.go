package skillbundle

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const validSkillMD = "---\nname: xlsx\ndescription: Build spreadsheets\n---\n\nBody.\n"

type zipEntry struct {
	name string
	body []byte
	mode fs.FileMode
}

func buildZip(t *testing.T, entries ...zipEntry) []byte {
	t.Helper()
	var buffer bytes.Buffer
	writer := zip.NewWriter(&buffer)
	for _, entry := range entries {
		mode := entry.mode
		if mode == 0 {
			mode = 0o644
		}
		header := &zip.FileHeader{Name: entry.name, Method: zip.Deflate}
		header.SetMode(mode)
		file, err := writer.CreateHeader(header)
		if err != nil {
			t.Fatalf("create %s: %v", entry.name, err)
		}
		if _, err := file.Write(entry.body); err != nil {
			t.Fatalf("write %s: %v", entry.name, err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatalf("close zip: %v", err)
	}
	return buffer.Bytes()
}

func refusalCode(t *testing.T, err error) string {
	t.Helper()
	var refusal *RefusalError
	if !errors.As(err, &refusal) {
		t.Fatalf("error = %v, want a RefusalError", err)
	}
	return refusal.Code
}

func TestParseAcceptsAValidBundleInPathOrder(t *testing.T) {
	body := buildZip(t,
		zipEntry{name: "scripts/run.py", body: []byte("print(1)\n"), mode: 0o755},
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "references/notes.md", body: []byte("notes")},
	)
	bundle, err := Parse(body)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	var paths []string
	for _, file := range bundle.Files {
		paths = append(paths, file.Path)
	}
	want := []string{"SKILL.md", "references/notes.md", "scripts/run.py"}
	if strings.Join(paths, ",") != strings.Join(want, ",") {
		t.Fatalf("paths = %v, want %v", paths, want)
	}
	if bundle.TotalBytes != int64(len(validSkillMD)+len("notes")+len("print(1)\n")) {
		t.Fatalf("total bytes = %d", bundle.TotalBytes)
	}
	if !bundle.Files[2].Executable || bundle.Files[0].Executable {
		t.Fatalf("executable bits = %v, %v; want only scripts/run.py executable", bundle.Files[0].Executable, bundle.Files[2].Executable)
	}
}

// The manifest digest is the same whatever order the zip stores its entries in, and changes
// with any path, digest, size or mode.
func TestManifestDigestIsDeterministicAndCoversEveryField(t *testing.T) {
	forward := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "scripts/a.sh", body: []byte("echo a"), mode: 0o755},
	)
	reversed := buildZip(t,
		zipEntry{name: "scripts/a.sh", body: []byte("echo a"), mode: 0o755},
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
	)
	a, err := Parse(forward)
	if err != nil {
		t.Fatalf("Parse forward: %v", err)
	}
	b, err := Parse(reversed)
	if err != nil {
		t.Fatalf("Parse reversed: %v", err)
	}
	if a.ManifestSHA256 != b.ManifestSHA256 {
		t.Fatalf("digest depends on zip entry order: %s != %s", a.ManifestSHA256, b.ManifestSHA256)
	}

	changedBody := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "scripts/a.sh", body: []byte("echo b"), mode: 0o755},
	)
	changedMode := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "scripts/a.sh", body: []byte("echo a"), mode: 0o644},
	)
	for name, body := range map[string][]byte{"body": changedBody, "mode": changedMode} {
		other, err := Parse(body)
		if err != nil {
			t.Fatalf("Parse %s variant: %v", name, err)
		}
		if other.ManifestSHA256 == a.ManifestSHA256 {
			t.Fatalf("changing the %s left the manifest digest unchanged", name)
		}
	}
}

func TestParseRefusesWhatIsNotAZip(t *testing.T) {
	if _, err := Parse([]byte("not a zip at all")); refusalCode(t, err) != "not_a_zip" {
		t.Fatalf("code = %v, want not_a_zip", err)
	}
}

func TestParseRefusesUnsafePaths(t *testing.T) {
	for _, name := range []string{"../evil.md", "/abs.md", "dir/../../x.md", "back\\slash.md", "C:drive.md", "dir//x.md", "./x.md", "ctl\x01.md", ""} {
		body := buildZip(t,
			zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
			zipEntry{name: name, body: []byte("x")},
		)
		if _, err := Parse(body); refusalCode(t, err) != "unsafe_path" {
			t.Errorf("path %q: code = %v, want unsafe_path", name, err)
		}
	}
}

func TestParseRefusesSymlinksAndNonRegularEntries(t *testing.T) {
	body := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "link", body: []byte("/etc/passwd"), mode: fs.ModeSymlink | 0o777},
	)
	if _, err := Parse(body); refusalCode(t, err) != "symlink_refused" {
		t.Fatalf("symlink: code = %v, want symlink_refused", err)
	}
	pipe := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "fifo", mode: fs.ModeNamedPipe | 0o644},
	)
	if _, err := Parse(pipe); refusalCode(t, err) != "not_a_regular_file" {
		t.Fatalf("pipe: code = %v, want not_a_regular_file", err)
	}
}

func TestParseRefusesDuplicatePaths(t *testing.T) {
	body := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
	)
	if _, err := Parse(body); refusalCode(t, err) != "duplicate_path" {
		t.Fatalf("code = %v, want duplicate_path", err)
	}
}

func TestParseCapsFileCountAndSizes(t *testing.T) {
	var many []zipEntry
	many = append(many, zipEntry{name: "SKILL.md", body: []byte(validSkillMD)})
	for i := 0; len(many) <= MaxFiles; i++ {
		many = append(many, zipEntry{name: fmt.Sprintf("references/f%03d.txt", i), body: []byte("x")})
	}
	if _, err := Parse(buildZip(t, many...)); refusalCode(t, err) != "too_many_files" {
		t.Fatalf("257 files: code = %v, want too_many_files", err)
	}

	exactFile := bytes.Repeat([]byte("a"), MaxFileBytes)
	if _, err := Parse(buildZip(t, zipEntry{name: "SKILL.md", body: []byte(validSkillMD)}, zipEntry{name: "big.txt", body: exactFile})); err != nil {
		t.Fatalf("a file of exactly 1 MiB must be accepted: %v", err)
	}
	overFile := bytes.Repeat([]byte("a"), MaxFileBytes+1)
	if _, err := Parse(buildZip(t, zipEntry{name: "SKILL.md", body: []byte(validSkillMD)}, zipEntry{name: "big.txt", body: overFile})); refusalCode(t, err) != "file_too_large" {
		t.Fatalf("1 MiB + 1: code = %v, want file_too_large", err)
	}

	var heavy []zipEntry
	heavy = append(heavy, zipEntry{name: "SKILL.md", body: []byte(validSkillMD)})
	for i := 0; i < 8; i++ {
		heavy = append(heavy, zipEntry{name: fmt.Sprintf("data/%d.bin", i), body: exactFile})
	}
	// SKILL.md plus eight 1 MiB files is 8 MiB + the front matter, one byte over the total cap.
	if _, err := Parse(buildZip(t, heavy...)); refusalCode(t, err) != "bundle_too_large" {
		t.Fatalf("over 8 MiB total: code = %v, want bundle_too_large", err)
	}
}

func TestParseRequiresSkillMarkdownWithFrontMatter(t *testing.T) {
	if _, err := Parse(buildZip(t, zipEntry{name: "README.md", body: []byte("hi")})); refusalCode(t, err) != "skill_md_missing" {
		t.Fatalf("no SKILL.md: code = %v, want skill_md_missing", err)
	}
	if _, err := Parse(buildZip(t, zipEntry{name: "references/SKILL.md", body: []byte(validSkillMD)})); refusalCode(t, err) != "skill_md_missing" {
		t.Fatalf("SKILL.md below the root: code = %v, want skill_md_missing", err)
	}
	for name, body := range map[string]string{
		"no front matter":   "# Title\nbody\n",
		"unclosed block":    "---\nname: a\ndescription: b\n",
		"missing name":      "---\ndescription: b\n---\n",
		"missing desc":      "---\nname: a\n---\n",
		"indented key only": "---\n  name: a\n  description: b\n---\n",
		"empty description": "---\nname: a\ndescription:\n---\n",
	} {
		if _, err := Parse(buildZip(t, zipEntry{name: "SKILL.md", body: []byte(body)})); refusalCode(t, err) != "skill_md_front_matter" {
			t.Errorf("%s: code = %v, want skill_md_front_matter", name, err)
		}
	}
}

func TestParseKeepsTheExecutableBitToScripts(t *testing.T) {
	body := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "tool.sh", body: []byte("echo"), mode: 0o755},
	)
	if _, err := Parse(body); refusalCode(t, err) != "exec_bit_outside_scripts" {
		t.Fatalf("code = %v, want exec_bit_outside_scripts", err)
	}
	nested := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "scripts/deep/run.sh", body: []byte("echo"), mode: 0o755},
	)
	if _, err := Parse(nested); err != nil {
		t.Fatalf("exec bit under scripts/: %v", err)
	}
}

func TestParseMarksNonTextFilesBinary(t *testing.T) {
	body := buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: "assets/logo.png", body: []byte{0x89, 'P', 'N', 'G', 0x00, 0xff}},
		zipEntry{name: "assets/notes.txt", body: []byte("plain text")},
	)
	bundle, err := Parse(body)
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	for _, file := range bundle.Files {
		switch file.Path {
		case "assets/logo.png":
			if !file.Binary {
				t.Errorf("%s not marked binary", file.Path)
			}
		case "assets/notes.txt", "SKILL.md":
			if file.Binary {
				t.Errorf("%s marked binary", file.Path)
			}
		}
	}
}

// EP-11 S8: an imported SKILL.md gives the skill its name, description and instructions.
func TestParseSkillFileSplitsFrontMatterFromBody(t *testing.T) {
	document, err := ParseSkillFile([]byte("---\r\nname: xlsx\r\ndescription: Build spreadsheets\r\n---\r\n\r\n# Steps\r\nUse the template.\r\n"))
	if err != nil {
		t.Fatalf("ParseSkillFile: %v", err)
	}
	if document.Name != "xlsx" || document.Description != "Build spreadsheets" {
		t.Fatalf("front matter = %q, %q", document.Name, document.Description)
	}
	if document.Body != "# Steps\r\nUse the template." {
		t.Fatalf("body = %q", document.Body)
	}
}

// A lone SKILL.md is held to the bundle's rules, with the bundle's codes.
func TestParseSkillFileRefusesWhatABundleWouldRefuse(t *testing.T) {
	for name, body := range map[string][]byte{
		"no front matter":   []byte("# Just a body\n"),
		"unclosed block":    []byte("---\nname: x\ndescription: y\n"),
		"missing describe":  []byte("---\nname: x\n---\nBody\n"),
		"indented name key": []byte("---\n name: x\ndescription: y\n---\nBody\n"),
		"not text":          append([]byte("---\nname: x\ndescription: y\n---\n"), 0x00, 0xff),
	} {
		if _, err := ParseSkillFile(body); refusalCode(t, err) != "skill_md_front_matter" {
			t.Errorf("%s: code = %q, want skill_md_front_matter", name, refusalCode(t, err))
		}
	}
	big := append([]byte(validSkillMD), bytes.Repeat([]byte("a"), MaxFileBytes)...)
	if _, err := ParseSkillFile(big); refusalCode(t, err) != "file_too_large" {
		t.Errorf("an over-cap SKILL.md: code = %q, want file_too_large", refusalCode(t, err))
	}
}

func TestBundleSkillDocumentReadsTheBundlesSkillFile(t *testing.T) {
	bundle, err := Parse(buildZip(t,
		zipEntry{name: "references/notes.md", body: []byte("---\nname: other\ndescription: no\n---\nNot this.\n")},
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	document := BundleSkillDocument(bundle)
	if document.Name != "xlsx" || document.Description != "Build spreadsheets" || document.Body != "Body." {
		t.Fatalf("document = %#v", document)
	}
}

// pngLogo and minimalMaster are the smallest bodies the deck template checks accept.
var pngLogo = []byte("\x89PNG\r\n\x1a\nrest-of-image")

func minimalMaster(t *testing.T) []byte {
	t.Helper()
	return buildZip(t,
		zipEntry{name: "[Content_Types].xml", body: []byte("<Types/>")},
		zipEntry{name: "ppt/presentation.xml", body: []byte("<p:presentation/>")},
	)
}

func themeFixture(t *testing.T) []byte {
	t.Helper()
	body, err := os.ReadFile(filepath.Join("testdata", "deck_theme_v1.json"))
	if err != nil {
		t.Fatalf("read theme fixture: %v", err)
	}
	return body
}

// themeWith is the fixture with one top-level key set (or removed when value is nil).
func themeWith(t *testing.T, key string, value any) []byte {
	t.Helper()
	var theme map[string]any
	if err := json.Unmarshal(themeFixture(t), &theme); err != nil {
		t.Fatalf("decode theme fixture: %v", err)
	}
	if value == nil {
		delete(theme, key)
	} else {
		theme[key] = value
	}
	body, err := json.Marshal(theme)
	if err != nil {
		t.Fatalf("encode theme: %v", err)
	}
	return body
}

func templateBundle(t *testing.T, theme []byte, extra ...zipEntry) Bundle {
	t.Helper()
	entries := append([]zipEntry{
		{name: "SKILL.md", body: []byte(validSkillMD)},
		{name: ThemePath, body: theme},
		{name: "templates/master.pptx", body: minimalMaster(t)},
		{name: "templates/logo.png", body: pngLogo},
	}, extra...)
	bundle, err := Parse(buildZip(t, entries...))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	return bundle
}

func TestDeckTemplatesSwitchIsTheLiteralTrueOnly(t *testing.T) {
	for _, raw := range []string{"", "1", "TRUE", " true", "yes", "on"} {
		if DeckTemplatesFromLookup(func(string) string { return raw }) {
			t.Errorf("%q enabled deck templates", raw)
		}
	}
	if !DeckTemplatesFromLookup(func(key string) string {
		if key == DeckTemplatesEnv {
			return "true"
		}
		return ""
	}) {
		t.Fatal(`"true" did not enable deck templates`)
	}
}

func TestDeckThemeFixtureMatchesTheRustContract(t *testing.T) {
	rust, err := os.ReadFile(filepath.Join("..", "..", "..", "..", "..", "rust", "crates", "mp-contracts", "tests", "fixtures", "deck_theme_v1.json"))
	if err != nil {
		t.Skipf("the Rust fixture is not in this checkout: %v", err)
	}
	if !bytes.Equal(bytes.ReplaceAll(rust, []byte("\r\n"), []byte("\n")), bytes.ReplaceAll(themeFixture(t), []byte("\r\n"), []byte("\n"))) {
		t.Fatal("testdata/deck_theme_v1.json differs from mp-contracts' fixture; copy it across")
	}
}

func TestABundleWithAValidThemeIsADeckTemplate(t *testing.T) {
	bundle := templateBundle(t, themeFixture(t))
	if err := ValidateDeckTemplate(bundle); err != nil {
		t.Fatalf("ValidateDeckTemplate: %v", err)
	}
	var paths []string
	for _, file := range bundle.Files {
		paths = append(paths, file.Path)
	}
	if !strings.Contains(strings.Join(paths, ","), ThemePath) {
		t.Fatalf("paths = %v, want the theme listed", paths)
	}
	plain, err := Parse(buildZip(t, zipEntry{name: "SKILL.md", body: []byte(validSkillMD)}))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if err := ValidateDeckTemplate(plain); err != nil {
		t.Fatalf("a bundle without a theme must pass unchanged: %v", err)
	}
}

func TestAThemeWithABadColourIsRefused(t *testing.T) {
	for _, colors := range []map[string]any{
		{"primary": "red", "secondary": "#000000", "text": "#000000", "background": "#ffffff"},
		{"primary": "#12345", "secondary": "#000000", "text": "#000000", "background": "#ffffff"},
		{"primary": "#000000", "secondary": "#000000", "text": "#000000", "background": "#ffffff",
			"accent": []string{"#000000", "#111111", "#222222", "#333333", "#444444"}},
		{"primary": "#000000", "secondary": "#000000", "text": "#000000", "background": "#ffffff", "accent": []string{"blue"}},
	} {
		err := ValidateDeckTemplate(templateBundle(t, themeWith(t, "colors", colors)))
		if code := refusalCode(t, err); code != "deck_theme_colour" {
			t.Errorf("colors %v: code = %q, want deck_theme_colour", colors, code)
		}
	}
}

func TestAThemeOutsideTheContractIsRefused(t *testing.T) {
	cases := map[string][]byte{
		"unknown key":      themeWith(t, "script", "run.sh"),
		"other version":    themeWith(t, "version", 2),
		"no name":          themeWith(t, "name", nil),
		"master elsewhere": themeWith(t, "master", "templates/other.pptx"),
		"logo outside":     themeWith(t, "logo", "assets/logo.png"),
		"long footer":      themeWith(t, "footer_text", strings.Repeat("x", maxThemeFooterChars+1)),
		"case-folded key":  bytes.Replace(themeFixture(t), []byte(`"name"`), []byte(`"Name"`), 1),
		"not json":         []byte("{"),
	}
	for name, theme := range cases {
		err := ValidateDeckTemplate(templateBundle(t, theme))
		if code := refusalCode(t, err); code != "deck_theme_invalid" {
			t.Errorf("%s: code = %q, want deck_theme_invalid", name, code)
		}
	}
}

func TestAMasterThatIsNotAPresentationIsRefused(t *testing.T) {
	notDeck := buildZip(t, zipEntry{name: "word/document.xml", body: []byte("<w:document/>")})
	for name, master := range map[string][]byte{"a docx": notDeck, "not a zip": []byte("plain text")} {
		bundle, err := Parse(buildZip(t,
			zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
			zipEntry{name: ThemePath, body: themeFixture(t)},
			zipEntry{name: "templates/master.pptx", body: master},
			zipEntry{name: "templates/logo.png", body: pngLogo},
		))
		if err != nil {
			t.Fatalf("Parse: %v", err)
		}
		if code := refusalCode(t, ValidateDeckTemplate(bundle)); code != "deck_master_not_presentation" {
			t.Errorf("%s: code = %q, want deck_master_not_presentation", name, code)
		}
	}
	missing, err := Parse(buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: ThemePath, body: themeFixture(t)},
		zipEntry{name: "templates/logo.png", body: pngLogo},
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if code := refusalCode(t, ValidateDeckTemplate(missing)); code != "deck_master_missing" {
		t.Errorf("code = %q, want deck_master_missing", code)
	}
}

func TestALogoThatIsNotAnImageIsRefused(t *testing.T) {
	bundle, err := Parse(buildZip(t,
		zipEntry{name: "SKILL.md", body: []byte(validSkillMD)},
		zipEntry{name: ThemePath, body: themeFixture(t)},
		zipEntry{name: "templates/master.pptx", body: minimalMaster(t)},
		zipEntry{name: "templates/logo.png", body: []byte("<svg onload=alert(1)/>")},
	))
	if err != nil {
		t.Fatalf("Parse: %v", err)
	}
	if code := refusalCode(t, ValidateDeckTemplate(bundle)); code != "deck_logo_not_image" {
		t.Errorf("code = %q, want deck_logo_not_image", code)
	}
	jpeg := templateBundle(t, themeWith(t, "logo", "templates/logo.jpg"),
		zipEntry{name: "templates/logo.jpg", body: []byte{0xff, 0xd8, 0xff, 0xe0, 0x00}})
	if err := ValidateDeckTemplate(jpeg); err != nil {
		t.Fatalf("a JPEG logo must pass: %v", err)
	}
	gone := templateBundle(t, themeWith(t, "logo", "templates/missing.png"))
	if code := refusalCode(t, ValidateDeckTemplate(gone)); code != "deck_logo_missing" {
		t.Errorf("code = %q, want deck_logo_missing", code)
	}
}

func TestAProgramUnderTemplatesIsRefused(t *testing.T) {
	for name, body := range map[string][]byte{
		"elf":    []byte("\x7fELF\x02\x01\x01"),
		"pe":     []byte("MZ\x90\x00"),
		"script": []byte("#!/bin/sh\necho hi\n"),
	} {
		bundle := templateBundle(t, themeFixture(t), zipEntry{name: "templates/helper", body: body})
		if code := refusalCode(t, ValidateDeckTemplate(bundle)); code != "template_executable" {
			t.Errorf("%s: code = %q, want template_executable", name, code)
		}
	}
	// The same program under scripts/ is a skill's business, not a template's.
	scripted := templateBundle(t, themeFixture(t), zipEntry{name: "scripts/helper.sh", body: []byte("#!/bin/sh\n"), mode: 0o755})
	if err := ValidateDeckTemplate(scripted); err != nil {
		t.Fatalf("scripts/ must stay allowed: %v", err)
	}
}
