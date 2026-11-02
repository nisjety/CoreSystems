package zdr

import (
	"bytes"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"path"
	"strings"
	"sync"
)

// The acknowledgement wording (Z-13). Every text the user is shown before they acknowledge a
// connection that cannot give ZDR lives in a versioned, immutable file under texts/. The index
// pins each file to its sha256, and the loader refuses anything else: an edited file, a file the
// index does not list, a listed file that is missing, or a text the capability contract needs
// that does not exist in every locale. A wording change is therefore a new version directory and
// a new index, never an edit in place.
//
// Each file holds, in this order: a title line, a continue line, a cancel line, a blank line, then
// the body (paragraphs separated by blank lines). What the user reads, and what an acknowledgement
// stores, is the snapshot: the title, a blank line and the body. Its sha256 is Text.SHA256.

//go:embed texts
var wordingFS embed.FS

const (
	wordingRoot      = "texts"
	wordingIndexPath = "texts/index.json"
	wordingFormat    = "zdr-acknowledgement-wording/1"
)

// wordingLocales are the locales the product ships acknowledgement text in.
var wordingLocales = []string{"no", "en"}

// ErrNoWording is returned for a key, variant or locale that has no text.
var ErrNoWording = errors.New("no acknowledgement text for this key, variant and locale")

type wordingIndex struct {
	Format   string                    `json:"format"`
	Current  string                    `json:"current"`
	Versions map[string]wordingVersion `json:"versions"`
}

type wordingVersion struct {
	Status string            `json:"status"`
	Files  map[string]string `json:"files"`
}

// Text is one acknowledgement text as a user is shown it.
type Text struct {
	Version       string
	Key           string
	Variant       string
	Locale        string
	Title         string
	Body          string
	ContinueLabel string
	CancelLabel   string
	// Snapshot is exactly what the user reads: the title, a blank line, then the body.
	Snapshot string
	// SHA256 is the lowercase hex sha256 of Snapshot. An acknowledgement stores it with the text.
	SHA256 string
}

// Wording is the loaded, verified set of texts of one version. It is immutable after load.
type Wording struct {
	version string
	status  string
	texts   map[string]Text
}

// Version is the version the texts belong to, the value an acknowledgement records as its text version.
func (w *Wording) Version() string { return w.version }

// Status is the review state of the version, for example draft_pending_od_legal until counsel signs off.
func (w *Wording) Status() string { return w.status }

// Lookup returns the text for one key, variant and locale. It never falls back to another locale
// or variant: a missing text is an error, so nobody is shown the wrong wording.
func (w *Wording) Lookup(key, variant, locale string) (Text, error) {
	text, ok := w.texts[wordingID(locale, key, variant)]
	if !ok {
		return Text{}, fmt.Errorf("%w: %s/%s in %q", ErrNoWording, key, variant, locale)
	}
	return text, nil
}

func wordingID(locale, key, variant string) string {
	return locale + "/" + key + "/" + variant
}

var (
	wordingOnce    sync.Once
	wordingDefault *Wording
	wordingErr     error
)

// DefaultWording is the embedded wording verified against the embedded capability contract. A
// service calls it at boot when acknowledgements are switched on and refuses to start on error.
func DefaultWording() (*Wording, error) {
	wordingOnce.Do(func() {
		assessor, err := Default()
		if err != nil {
			wordingErr = err
			return
		}
		wordingDefault, wordingErr = loadWordingFrom(wordingFS, assessor)
	})
	return wordingDefault, wordingErr
}

// loadWordingFrom verifies and parses the wording under texts/ in fsys against assessor's contract.
func loadWordingFrom(fsys fs.FS, assessor *Assessor) (*Wording, error) {
	raw, err := fs.ReadFile(fsys, wordingIndexPath)
	if err != nil {
		return nil, fmt.Errorf("read acknowledgement wording index: %w", err)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var index wordingIndex
	if err := decoder.Decode(&index); err != nil {
		return nil, fmt.Errorf("parse acknowledgement wording index: %w", err)
	}
	if index.Format != wordingFormat {
		return nil, fmt.Errorf("acknowledgement wording index has format %q, want %q", index.Format, wordingFormat)
	}
	version, ok := index.Versions[index.Current]
	if !ok {
		return nil, fmt.Errorf("acknowledgement wording: current version %q is not in the index", index.Current)
	}
	if strings.TrimSpace(version.Status) == "" {
		return nil, fmt.Errorf("acknowledgement wording %s has no review status", index.Current)
	}

	// Every listed file exists and matches its pinned digest. Line endings are normalised to LF
	// before hashing, as the capability contract is, so a checkout's line endings cannot break a pin.
	for name, pin := range version.Files {
		if !strings.HasPrefix(name, index.Current+"/") || strings.Contains(name, "..") {
			return nil, fmt.Errorf("acknowledgement wording lists %q outside version %s", name, index.Current)
		}
		if !sha256Pattern.MatchString(pin) {
			return nil, fmt.Errorf("acknowledgement wording pin for %s is not a sha256 digest", name)
		}
		data, err := fs.ReadFile(fsys, path.Join(wordingRoot, name))
		if err != nil {
			return nil, fmt.Errorf("acknowledgement wording %s is listed but missing: %w", name, err)
		}
		if actual := sha256Hex(strings.ReplaceAll(string(data), "\r\n", "\n")); actual != pin {
			return nil, fmt.Errorf("acknowledgement wording %s does not match its pinned digest (pinned %s, actual %s)", name, pin, actual)
		}
	}

	// And nothing else sits in the version directory: a text the index does not pin is refused.
	err = fs.WalkDir(fsys, path.Join(wordingRoot, index.Current), func(p string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.IsDir() {
			return nil
		}
		name := strings.TrimPrefix(p, wordingRoot+"/")
		if _, listed := version.Files[name]; !listed {
			return fmt.Errorf("acknowledgement wording file %s is not pinned in the index", name)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	variants := assessor.WordingVariants()
	texts := make(map[string]Text, len(version.Files))
	for name := range version.Files {
		text, err := parseWordingEntry(fsys, index.Current, name, variants)
		if err != nil {
			return nil, err
		}
		texts[wordingID(text.Locale, text.Key, text.Variant)] = text
	}

	// Completeness: the contract needs a text for every key, variant and locale, and nothing more.
	for key, list := range variants {
		for _, variant := range list {
			for _, locale := range wordingLocales {
				if _, ok := texts[wordingID(locale, key, variant)]; !ok {
					return nil, fmt.Errorf("acknowledgement wording %s has no text for %s/%s in %s", index.Current, key, variant, locale)
				}
			}
		}
	}
	return &Wording{version: index.Current, status: version.Status, texts: texts}, nil
}

// parseWordingEntry reads one text file, named <version>/<locale>/<key>.<variant>.txt, and checks
// its locale, key and variant against the contract.
func parseWordingEntry(fsys fs.FS, version, name string, variants map[string][]string) (Text, error) {
	locale := path.Base(path.Dir(name))
	base := strings.TrimSuffix(path.Base(name), ".txt")
	parts := strings.Split(base, ".")
	if len(parts) != 2 || path.Dir(path.Dir(name)) != version || !contains(wordingLocales, locale) {
		return Text{}, fmt.Errorf("acknowledgement wording file %s must be named <version>/<locale>/<key>.<variant>.txt", name)
	}
	key, variant := parts[0], parts[1]
	list, known := variants[key]
	if !known || !contains(list, variant) {
		return Text{}, fmt.Errorf("acknowledgement wording file %s has no key and variant in the capability contract", name)
	}
	raw, err := fs.ReadFile(fsys, path.Join(wordingRoot, name))
	if err != nil {
		return Text{}, fmt.Errorf("read acknowledgement wording %s: %w", name, err)
	}
	title, continueLabel, cancelLabel, body, err := parseWordingFile(string(raw))
	if err != nil {
		return Text{}, fmt.Errorf("acknowledgement wording %s: %w", name, err)
	}
	snapshot := title + "\n\n" + body
	return Text{
		Version:       version,
		Key:           key,
		Variant:       variant,
		Locale:        locale,
		Title:         title,
		Body:          body,
		ContinueLabel: continueLabel,
		CancelLabel:   cancelLabel,
		Snapshot:      snapshot,
		SHA256:        sha256Hex(snapshot),
	}, nil
}

// parseWordingFile parses the fixed layout: title, continue, cancel, a blank line, the body.
func parseWordingFile(content string) (title, continueLabel, cancelLabel, body string, err error) {
	content = strings.ReplaceAll(content, "\r\n", "\n")
	if !strings.HasSuffix(content, "\n") {
		return "", "", "", "", errors.New("must end with a newline")
	}
	lines := strings.SplitN(strings.TrimSuffix(content, "\n"), "\n", 5)
	if len(lines) != 5 || lines[3] != "" {
		return "", "", "", "", errors.New("must be a title, continue and cancel line, a blank line, then the body")
	}
	title, err = wordingField(lines[0], "title: ")
	if err != nil {
		return "", "", "", "", err
	}
	continueLabel, err = wordingField(lines[1], "continue: ")
	if err != nil {
		return "", "", "", "", err
	}
	cancelLabel, err = wordingField(lines[2], "cancel: ")
	if err != nil {
		return "", "", "", "", err
	}
	body = strings.TrimSpace(lines[4])
	if body == "" {
		return "", "", "", "", errors.New("the body is empty")
	}
	return title, continueLabel, cancelLabel, body, nil
}

func wordingField(line, prefix string) (string, error) {
	if !strings.HasPrefix(line, prefix) {
		return "", fmt.Errorf("expected a line starting %q", prefix)
	}
	value := strings.TrimSpace(strings.TrimPrefix(line, prefix))
	if value == "" {
		return "", fmt.Errorf("the %s value is empty", strings.TrimSuffix(prefix, ": "))
	}
	return value, nil
}
