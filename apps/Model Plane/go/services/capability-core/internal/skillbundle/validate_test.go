package skillbundle

import (
	"archive/zip"
	"bytes"
	"errors"
	"fmt"
	"io/fs"
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
