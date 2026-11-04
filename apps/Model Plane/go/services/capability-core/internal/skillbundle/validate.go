package skillbundle

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"strings"
	"unicode"
	"unicode/utf8"
)

// RefusalError names why an upload was refused. Code is a closed, content-free word that
// the HTTP layer returns as the error; it never carries a path or a file body.
type RefusalError struct {
	Code string
}

func (e *RefusalError) Error() string {
	return "skill bundle refused: " + e.Code
}

func refuse(code string) error {
	return &RefusalError{Code: code}
}

// Parse validates an uploaded zip and returns the bundle it holds. Every check runs before
// the bundle is stored: the zip must parse, every entry must be a plain file at a safe
// relative path, the caps must hold, the executable bit may only sit under scripts/, and
// SKILL.md must be at the root with its name and description in front matter.
//
// Symlinks, devices and other non-regular entries are refused, not skipped, so a bundle can
// never point a reader at a file outside itself. Directory entries carry no content and are
// ignored.
func Parse(body []byte) (Bundle, error) {
	reader, err := zip.NewReader(bytes.NewReader(body), int64(len(body)))
	if err != nil {
		return Bundle{}, refuse("not_a_zip")
	}
	if len(reader.File) > MaxFiles {
		return Bundle{}, refuse("too_many_files")
	}
	var (
		files   []File
		total   int64
		seen    = make(map[string]bool, len(reader.File))
		skillAt = -1
	)
	for _, entry := range reader.File {
		mode := entry.Mode()
		if mode.IsDir() {
			continue
		}
		if mode&fs.ModeSymlink != 0 {
			return Bundle{}, refuse("symlink_refused")
		}
		if !mode.IsRegular() {
			return Bundle{}, refuse("not_a_regular_file")
		}
		if !validPath(entry.Name) {
			return Bundle{}, refuse("unsafe_path")
		}
		if seen[entry.Name] {
			return Bundle{}, refuse("duplicate_path")
		}
		seen[entry.Name] = true
		if entry.UncompressedSize64 > MaxFileBytes {
			return Bundle{}, refuse("file_too_large")
		}
		if int64(entry.UncompressedSize64) > MaxTotalBytes-total {
			return Bundle{}, refuse("bundle_too_large")
		}
		content, err := readEntry(entry)
		if err != nil {
			return Bundle{}, err
		}
		total += int64(len(content))
		if total > MaxTotalBytes {
			return Bundle{}, refuse("bundle_too_large")
		}
		executable := mode.Perm()&0o111 != 0
		if executable && !strings.HasPrefix(entry.Name, scriptsDir) {
			return Bundle{}, refuse("exec_bit_outside_scripts")
		}
		file := File{
			Path:       entry.Name,
			SHA256:     bodyDigest(content),
			Size:       int64(len(content)),
			Executable: executable,
			Binary:     isBinary(content),
			Body:       content,
		}
		files = append(files, file)
		if entry.Name == SkillFileName {
			skillAt = len(files) - 1
		}
	}
	if skillAt < 0 {
		return Bundle{}, refuse("skill_md_missing")
	}
	if skillFile := files[skillAt]; skillFile.Binary || !validFrontMatter(skillFile.Body) {
		return Bundle{}, refuse("skill_md_front_matter")
	}
	sortFiles(files)
	return Bundle{Files: files, TotalBytes: total, ManifestSHA256: manifestDigest(files)}, nil
}

// readEntry reads one file body, never more than MaxFileBytes. Reading to EOF also makes the
// zip reader check the entry's CRC, so a corrupt entry is refused rather than stored.
func readEntry(entry *zip.File) ([]byte, error) {
	rc, err := entry.Open()
	if err != nil {
		return nil, refuse("not_a_zip")
	}
	defer rc.Close()
	content, err := io.ReadAll(io.LimitReader(rc, MaxFileBytes+1))
	if err != nil {
		return nil, refuse("not_a_zip")
	}
	if len(content) > MaxFileBytes {
		return nil, refuse("file_too_large")
	}
	return content, nil
}

// validPath accepts only a clean, relative, forward-slash path. A path with a drive colon, a
// backslash, a leading slash, an empty, "." or ".." segment, or a control character is refused.
func validPath(name string) bool {
	if name == "" || len(name) > 512 || strings.HasPrefix(name, "/") || strings.ContainsAny(name, "\\:") {
		return false
	}
	for _, r := range name {
		if r < 0x20 || r == 0x7f {
			return false
		}
	}
	for _, segment := range strings.Split(name, "/") {
		if segment == "" || segment == "." || segment == ".." {
			return false
		}
	}
	return true
}

// validFrontMatter requires a YAML-style front matter block at the top of SKILL.md with a
// non-empty name and description. Keys are read at column zero only, so an indented line in
// a body cannot satisfy the check.
func validFrontMatter(body []byte) bool {
	fields, closed := frontMatter(body)
	return closed && fields["name"] != "" && fields["description"] != ""
}

// frontMatter reads the column-zero keys of the front matter block at the top of SKILL.md
// and whether the block is closed.
func frontMatter(body []byte) (map[string]string, bool) {
	lines := strings.Split(string(body), "\n")
	if len(lines) == 0 || strings.TrimRight(lines[0], "\r") != "---" {
		return nil, false
	}
	fields := make(map[string]string, 2)
	for _, raw := range lines[1:] {
		line := strings.TrimRight(raw, "\r")
		if line == "---" {
			return fields, true
		}
		key, value, found := strings.Cut(line, ":")
		if !found || key == "" || key != strings.TrimSpace(key) {
			continue
		}
		fields[key] = strings.TrimSpace(value)
	}
	return fields, false
}

// FrontMatterName is the name in the bundle's SKILL.md front matter, read by the same rules
// Parse checked it with. The skill lint compares it with the name of the skill the bundle is
// uploaded to (EP-11 S4).
func FrontMatterName(bundle Bundle) string {
	for _, file := range bundle.Files {
		if file.Path == SkillFileName {
			fields, _ := frontMatter(file.Body)
			return fields["name"]
		}
	}
	return ""
}

// DeckTemplatesEnv is the switch for deck brand templates (EP-5 S6). Only the literal "true"
// turns it on; off, an upload is checked exactly as before and no listing reports a template.
const DeckTemplatesEnv = "SKILL_DECK_TEMPLATES_ENABLED"

// DeckTemplatesFromLookup reads SKILL_DECK_TEMPLATES_ENABLED strictly.
func DeckTemplatesFromLookup(lookup func(string) string) bool {
	return lookup(DeckTemplatesEnv) == "true"
}

// The deck theme contract. It mirrors mp_contracts::deck_theme in the Rust workspace; both read
// the same fixture (testdata/deck_theme_v1.json here), so a rule changed on one side fails the
// other's test.
const (
	// ThemePath is the file that makes a bundle a deck template.
	ThemePath    = "templates/deck-theme.json"
	masterPath   = "templates/master.pptx"
	templatesDir = "templates/"

	themeVersion          = 1
	maxThemeBytes         = 32 << 10
	maxThemeNameChars     = 80
	maxThemeDescChars     = 200
	maxThemeAccents       = 4
	maxThemeFontChars     = 64
	maxThemeFooterChars   = 80
	maxThemeLogoPathBytes = 255
)

// deckTheme is a decoded theme file. Pointers mark the optional keys.
type deckTheme struct {
	Version     int
	Name        string
	Description *string
	Colors      struct {
		Primary, Secondary, Text, Background string
		Accent                               []string
	}
	Fonts struct {
		Heading, Body string
	}
	Logo       *string
	Master     *string
	FooterText *string
}

// ValidateDeckTemplate checks the templates/ half of a parsed bundle while deck templates are on.
// Nothing under templates/ may be a program: templates need no scripts, so one there is a
// supply-chain risk, not a feature. A master must be a presentation. When the theme is present it
// must hold the contract, and the master and logo it names must be in the bundle, the logo a PNG
// or JPEG by its signature.
func ValidateDeckTemplate(bundle Bundle) error {
	files := make(map[string]File, len(bundle.Files))
	for _, file := range bundle.Files {
		files[file.Path] = file
		if strings.HasPrefix(file.Path, templatesDir) && (file.Executable || looksExecutable(file.Body)) {
			return refuse("template_executable")
		}
	}
	if master, ok := files[masterPath]; ok && !isPresentation(master.Body) {
		return refuse("deck_master_not_presentation")
	}
	themeFile, ok := files[ThemePath]
	if !ok {
		return nil
	}
	if themeFile.Binary || len(themeFile.Body) == 0 || len(themeFile.Body) > maxThemeBytes {
		return refuse("deck_theme_invalid")
	}
	theme, ok := decodeDeckTheme(themeFile.Body)
	if !ok {
		return refuse("deck_theme_invalid")
	}
	if code := theme.problem(); code != "" {
		return refuse(code)
	}
	if theme.Master != nil {
		if _, present := files[masterPath]; !present {
			return refuse("deck_master_missing")
		}
	}
	if theme.Logo != nil {
		logo, present := files[*theme.Logo]
		if !present {
			return refuse("deck_logo_missing")
		}
		if !isPNGOrJPEG(logo.Body) {
			return refuse("deck_logo_not_image")
		}
	}
	return nil
}

// decodeDeckTheme reads the theme with exact, case-sensitive keys at every level. Go's decoder
// matches keys case-insensitively and skips unknown ones; the contract does neither.
func decodeDeckTheme(body []byte) (deckTheme, bool) {
	var theme deckTheme
	top, ok := exactKeys(body, []string{"version", "name", "colors", "fonts"},
		[]string{"description", "logo", "master", "footer_text"})
	if !ok {
		return theme, false
	}
	colors, ok := exactKeys(top["colors"], []string{"primary", "secondary", "text", "background"}, []string{"accent"})
	if !ok {
		return theme, false
	}
	fonts, ok := exactKeys(top["fonts"], []string{"heading", "body"}, nil)
	if !ok {
		return theme, false
	}
	required := []struct {
		raw    json.RawMessage
		target any
	}{
		{top["version"], &theme.Version},
		{top["name"], &theme.Name},
		{colors["primary"], &theme.Colors.Primary},
		{colors["secondary"], &theme.Colors.Secondary},
		{colors["text"], &theme.Colors.Text},
		{colors["background"], &theme.Colors.Background},
		{fonts["heading"], &theme.Fonts.Heading},
		{fonts["body"], &theme.Fonts.Body},
	}
	for _, field := range required {
		if !decodeStrict(field.raw, field.target) {
			return theme, false
		}
	}
	optional := []struct {
		raw    json.RawMessage
		target **string
	}{
		{top["description"], &theme.Description},
		{top["logo"], &theme.Logo},
		{top["master"], &theme.Master},
		{top["footer_text"], &theme.FooterText},
	}
	for _, field := range optional {
		if isAbsent(field.raw) {
			continue
		}
		var value string
		if !decodeStrict(field.raw, &value) {
			return theme, false
		}
		*field.target = &value
	}
	if raw := colors["accent"]; !isAbsent(raw) && !decodeStrict(raw, &theme.Colors.Accent) {
		return theme, false
	}
	return theme, true
}

// isAbsent treats a missing optional key and an explicit null alike, as the Rust contract does.
func isAbsent(raw json.RawMessage) bool {
	return raw == nil || string(raw) == "null"
}

// exactKeys decodes one JSON object and refuses it unless every required key is present and no
// key outside required and optional is.
func exactKeys(raw json.RawMessage, required, optional []string) (map[string]json.RawMessage, bool) {
	var object map[string]json.RawMessage
	if raw == nil || !decodeStrict(raw, &object) || object == nil {
		return nil, false
	}
	allowed := make(map[string]bool, len(required)+len(optional))
	for _, key := range required {
		if _, ok := object[key]; !ok {
			return nil, false
		}
		allowed[key] = true
	}
	for _, key := range optional {
		allowed[key] = true
	}
	for key := range object {
		if !allowed[key] {
			return nil, false
		}
	}
	return object, true
}

// decodeStrict decodes exactly one JSON value into target. A null for a required key leaves the
// target empty, which the rules then refuse.
func decodeStrict(raw json.RawMessage, target any) bool {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if err := decoder.Decode(target); err != nil {
		return false
	}
	_, err := decoder.Token()
	return errors.Is(err, io.EOF)
}

// problem returns the refusal code of the first rule the theme breaks, or "" when it holds them all.
func (t deckTheme) problem() string {
	if t.Version != themeVersion || !themeText(t.Name, maxThemeNameChars) {
		return "deck_theme_invalid"
	}
	if t.Description != nil && utf8.RuneCountInString(*t.Description) > maxThemeDescChars {
		return "deck_theme_invalid"
	}
	for _, colour := range []string{t.Colors.Primary, t.Colors.Secondary, t.Colors.Text, t.Colors.Background} {
		if !isHexColour(colour) {
			return "deck_theme_colour"
		}
	}
	if len(t.Colors.Accent) > maxThemeAccents {
		return "deck_theme_colour"
	}
	for _, colour := range t.Colors.Accent {
		if !isHexColour(colour) {
			return "deck_theme_colour"
		}
	}
	if !themeText(t.Fonts.Heading, maxThemeFontChars) || !themeText(t.Fonts.Body, maxThemeFontChars) {
		return "deck_theme_invalid"
	}
	if t.Logo != nil && !validLogoPath(*t.Logo) {
		return "deck_theme_invalid"
	}
	if t.Master != nil && *t.Master != masterPath {
		return "deck_theme_invalid"
	}
	if t.FooterText != nil && !themeText(*t.FooterText, maxThemeFooterChars) {
		return "deck_theme_invalid"
	}
	return ""
}

// themeText is one line of display text: not blank, at most limit characters, no control character.
func themeText(value string, limit int) bool {
	if strings.TrimSpace(value) == "" || utf8.RuneCountInString(value) > limit {
		return false
	}
	for _, r := range value {
		if unicode.IsControl(r) {
			return false
		}
	}
	return true
}

// isHexColour accepts "#" and six hex digits, either case.
func isHexColour(value string) bool {
	if len(value) != 7 || value[0] != '#' {
		return false
	}
	for _, r := range value[1:] {
		if !strings.ContainsRune("0123456789abcdefABCDEF", r) {
			return false
		}
	}
	return true
}

// validLogoPath is a clean bundle path under templates/ naming a .png, .jpg or .jpeg file.
func validLogoPath(path string) bool {
	lower := strings.ToLower(path)
	return len(path) <= maxThemeLogoPathBytes && strings.HasPrefix(path, templatesDir) && validPath(path) &&
		(strings.HasSuffix(lower, ".png") || strings.HasSuffix(lower, ".jpg") || strings.HasSuffix(lower, ".jpeg"))
}

// isPresentation reports whether body is a zip that holds ppt/presentation.xml, the part every
// PresentationML package has. It reads the central directory only, never a part's body.
func isPresentation(body []byte) bool {
	reader, err := zip.NewReader(bytes.NewReader(body), int64(len(body)))
	if err != nil {
		return false
	}
	for _, entry := range reader.File {
		if entry.Name == "ppt/presentation.xml" {
			return true
		}
	}
	return false
}

// isPNGOrJPEG checks the file signature, not the name.
func isPNGOrJPEG(body []byte) bool {
	return bytes.HasPrefix(body, []byte("\x89PNG\r\n\x1a\n")) || bytes.HasPrefix(body, []byte{0xff, 0xd8, 0xff})
}

// looksExecutable reports a native program or an interpreter script by its first bytes: ELF,
// PE ("MZ"), Mach-O and "#!".
func looksExecutable(body []byte) bool {
	for _, magic := range [][]byte{
		[]byte("\x7fELF"), []byte("MZ"), []byte("#!"),
		{0xfe, 0xed, 0xfa, 0xce}, {0xfe, 0xed, 0xfa, 0xcf}, {0xce, 0xfa, 0xed, 0xfe}, {0xcf, 0xfa, 0xed, 0xfe},
	} {
		if bytes.HasPrefix(body, magic) {
			return true
		}
	}
	return false
}
