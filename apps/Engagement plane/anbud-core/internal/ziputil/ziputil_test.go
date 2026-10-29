package ziputil

import (
	"archive/zip"
	"bytes"
	"compress/flate"
	"context"
	"errors"
	"fmt"
	"hash/crc32"
	"io"
	"io/fs"
	"slices"
	"strings"
	"testing"
)

func fsMode(m uint32) fs.FileMode { return fs.FileMode(m) }

const modeSymlink = uint32(fs.ModeSymlink) | 0o777

// file is one entry of a test archive.
type file struct {
	name string
	data string
	mode uint32 // unix mode bits, 0 for a plain file
}

// build writes an archive. Entries are deflated unless the data is empty.
func build(t *testing.T, files ...file) []byte {
	t.Helper()
	var buf bytes.Buffer
	w := zip.NewWriter(&buf)
	for _, f := range files {
		h := &zip.FileHeader{Name: f.name, Method: zip.Deflate}
		if f.mode != 0 {
			h.SetMode(fsMode(f.mode))
		}
		fw, err := w.CreateHeader(h)
		if err != nil {
			t.Fatalf("create %q: %v", f.name, err)
		}
		if _, err := io.WriteString(fw, f.data); err != nil {
			t.Fatalf("write %q: %v", f.name, err)
		}
	}
	if err := w.Close(); err != nil {
		t.Fatalf("close: %v", err)
	}
	return buf.Bytes()
}

// collect walks an archive and returns what fn was given, by path.
func collect(t *testing.T, data []byte, lim Limits) (map[string]string, Report, error) {
	t.Helper()
	got := map[string]string{}
	report, err := Walk(context.Background(), data, lim, func(e Entry) error {
		got[e.Path] = string(e.Data)
		return nil
	})
	return got, report, err
}

func limits() Limits {
	return Limits{MaxArchiveBytes: 1 << 20, MaxEntries: 50, MaxEntryBytes: 1 << 16, MaxTotalBytes: 1 << 18, MaxRatio: 100, MaxDepth: 3}
}

func TestWalkReadsTheFilesOfAnArchive(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "konkurransegrunnlag.txt", data: "Kapittel 1"}, file{name: "vedlegg/prisskjema.txt", data: "Pris"})
	got, report, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	want := map[string]string{"konkurransegrunnlag.txt": "Kapittel 1", "vedlegg/prisskjema.txt": "Pris"}
	if len(got) != 2 || got["konkurransegrunnlag.txt"] != want["konkurransegrunnlag.txt"] || got["vedlegg/prisskjema.txt"] != want["vedlegg/prisskjema.txt"] {
		t.Errorf("entries = %v, want %v", got, want)
	}
	if report.Files != 2 || report.Bytes != int64(len("Kapittel 1")+len("Pris")) || len(report.Skipped) != 0 || report.Ignored != 0 {
		t.Errorf("report = %+v", report)
	}
}

func TestWalkIgnoresJunkAndDirectoriesSilently(t *testing.T) {
	t.Parallel()

	data := build(t,
		file{name: "dokumenter/"},
		file{name: "__MACOSX/._konkurransegrunnlag.pdf", data: "junk"},
		file{name: ".DS_Store", data: "junk"},
		file{name: "sub/Thumbs.db", data: "junk"},
		file{name: "desktop.ini", data: "junk"},
		file{name: "dokumenter/ekte.txt", data: "innhold"},
	)
	got, report, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 || got["dokumenter/ekte.txt"] != "innhold" {
		t.Errorf("entries = %v, want only the real file", got)
	}
	if report.Ignored != 4 || len(report.Skipped) != 0 {
		t.Errorf("ignored %d skipped %v, want 4 ignored (the directory is not counted) and none skipped", report.Ignored, report.Skipped)
	}
}

// A path is data from the archive's author. Nothing is ever written to disk here, but the name is
// stored and shown, so it is made safe first: relative, slash-separated, no dot segments, no
// control characters, in one Unicode form.
func TestWalkSanitisesNames(t *testing.T) {
	t.Parallel()

	data := build(t,
		file{name: `vedlegg\kontrakt.txt`, data: "a"},          // a Windows separator
		file{name: "/absolutt/sti.txt", data: "b"},             // leading slash
		file{name: "./relativ/./fil.txt", data: "c"},           // dot segments
		file{name: "med\x00nul\x07.txt", data: "d"},            // control characters
		file{name: "a\xcc\x8ab.txt", data: "e"},                // NFD: a + combining ring above
		file{name: "ok" + strings.Repeat("x", 300), data: "f"}, // far too long
	)
	got, _, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	for path := range got {
		if strings.ContainsAny(path, "\\\x00\x07") || strings.HasPrefix(path, "/") || strings.Contains(path, "./") || len([]rune(path)) > maxPathRunes {
			t.Errorf("path %q is not safe", path)
		}
	}
	for _, want := range []string{"vedlegg/kontrakt.txt", "absolutt/sti.txt", "relativ/fil.txt", "mednul.txt", "åb.txt"} {
		if _, ok := got[want]; !ok {
			t.Errorf("no entry %q in %v", want, keys(got))
		}
	}
}

// A name is stored and shown, so what cannot be seen is not kept in it: not only the few characters a list names but
// the marks that say which way a line reads, the invisible operators, the tag characters (which can hold a message
// no one sees), the variation selectors and the fillers. What is read in another script is kept.
func TestWalkDropsWhatCannotBeSeenFromANameWhicheverCharacterItIs(t *testing.T) {
	t.Parallel()

	hidden := []string{
		"\u200e", "\u200f", "\u061c", "\u2061", "\u2062", "\u2063", "\u2064", "\u180e", "\U000E0041", "\U000E007F",
		"\ufe0f", "\ufe00", "\U000E0100", "\u034f", "\u3164", "\uffa0", "\u115f", "\u1160", "\u17b4", "\u17b5",
		// what the list of a few did name
		"\u00ad", "\u200b", "\u200c", "\u200d", "\ufeff", "\u2060", "\u202a", "\u202e", "\u2066", "\u2069",
	}
	var files []file
	for i, r := range hidden {
		files = append(files, file{name: fmt.Sprintf("d%02d_a%sb.txt", i, r), data: "x"})
	}
	files = append(files, file{name: "한글-مرحبا-漢字.txt", data: "y"})
	got, _, err := collect(t, build(t, files...), limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	for i := range hidden {
		want := fmt.Sprintf("d%02d_ab.txt", i)
		if _, ok := got[want]; !ok {
			t.Errorf("U+%04X was kept in a name: no entry %q in %v", []rune(hidden[i])[0], want, keys(got))
		}
	}
	if _, ok := got["한글-مرحبا-漢字.txt"]; !ok {
		t.Errorf("a name in other scripts was changed: %v", keys(got))
	}
}

// The same of the name of a file that was refused, which is reported.
func TestTheReportOfARefusedNameHasNothingInItThatCannotBeSeen(t *testing.T) {
	t.Parallel()

	_, report, err := collect(t, build(t, file{name: "../a\u200eb\u2061c\U000E0041d\ufe0fe.txt", data: "x"}), limits())
	if err != nil || len(report.Skipped) != 1 {
		t.Fatalf("skipped %+v, %v", report.Skipped, err)
	}
	if got := report.Skipped[0].Path; got != "../abcde.txt" {
		t.Errorf("reported as %q, want ../abcde.txt", got)
	}
}

func keys(m map[string]string) []string {
	var out []string
	for k := range m {
		out = append(out, k)
	}
	slices.Sort(out)
	return out
}

func TestWalkSkipsAnEntryThatTriesToLeaveItsFolder(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "../../etc/passwd", data: "x"}, file{name: "ok/../../utenfor.txt", data: "y"}, file{name: "fin.txt", data: "z"})
	got, report, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 || got["fin.txt"] != "z" {
		t.Errorf("entries = %v, want only fin.txt", keys(got))
	}
	if len(report.Skipped) != 2 || report.Skipped[0].Reason != SkipBadName || report.Skipped[1].Reason != SkipBadName {
		t.Errorf("skipped = %+v, want two bad names", report.Skipped)
	}
}

func TestWalkSkipsSpecialFilesAndKeepsTheRest(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "lenke", data: "/etc/passwd", mode: modeSymlink}, file{name: "ekte.txt", data: "ok"})
	got, report, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipSpecial {
		t.Errorf("entries %v skipped %+v, want the symlink skipped as special", keys(got), report.Skipped)
	}
}

func TestWalkSkipsADuplicateName(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "Krav.txt", data: "første"}, file{name: "krav.TXT", data: "andre"}, file{name: "krav.txt", data: "tredje"})
	got, report, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 || got["Krav.txt"] != "første" {
		t.Errorf("entries = %v, want the first of the three", got)
	}
	if len(report.Skipped) != 2 || report.Skipped[0].Reason != SkipDuplicateName {
		t.Errorf("skipped = %+v", report.Skipped)
	}
}

func TestWalkSkipsAnEntryOverTheEntryLimitWithoutReadingIt(t *testing.T) {
	t.Parallel()

	lim := limits()
	lim.MaxEntryBytes = 100
	data := build(t, file{name: "stor.txt", data: strings.Repeat("a", 500)}, file{name: "liten.txt", data: "ok"})
	got, report, err := collect(t, data, lim)
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 || got["liten.txt"] != "ok" || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipTooLarge {
		t.Errorf("entries %v skipped %+v", keys(got), report.Skipped)
	}
}

func TestWalkRefusesAnArchiveThatCouldBeABomb(t *testing.T) {
	t.Parallel()

	// 8 MiB of zeros deflates to a few kilobytes: a ratio of well over a thousand.
	lim := limits()
	lim.MaxEntryBytes = 16 << 20
	lim.MaxTotalBytes = 64 << 20
	lim.MaxArchiveBytes = 1 << 20
	data := build(t, file{name: "nuller.bin", data: strings.Repeat("\x00", 8<<20)})
	if len(data) > 1<<20 {
		t.Fatalf("the fixture is %d bytes: it should be small, that is the point", len(data))
	}
	_, _, err := collect(t, data, lim)
	if !errors.Is(err, ErrBomb) {
		t.Errorf("err = %v, want ErrBomb", err)
	}
}

func TestWalkAllowsAHighRatioOnATinyFile(t *testing.T) {
	t.Parallel()

	// A few hundred bytes of one letter deflate by a large factor, and mean no harm: the ratio
	// is judged only on files big enough for it to matter.
	data := build(t, file{name: "a.txt", data: strings.Repeat("a", 5000)})
	if _, _, err := collect(t, data, limits()); err != nil {
		t.Errorf("err = %v, want none", err)
	}
}

// The size in an entry's header is the author's word, and the standard library holds the entry to
// it: bytes past the declared size are an error, not more data. An entry that lies is skipped as
// unreadable, and what it claimed to hold is never delivered.
func TestWalkSkipsAnEntryThatLiesAboutItsSize(t *testing.T) {
	t.Parallel()

	data := craft(t, "lugner.txt", strings.Repeat("x", 50_000), 10) // the header says 10 bytes
	var delivered int
	report, err := Walk(context.Background(), data, limits(), func(e Entry) error { delivered += len(e.Data); return nil })
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if delivered != 0 || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipUnreadable {
		t.Errorf("delivered %d bytes, skipped %+v: want nothing delivered and the entry skipped as unreadable", delivered, report.Skipped)
	}
}

// craft writes one deflated entry whose header declares declaredSize bytes, whatever it holds.
func craft(t *testing.T, name, content string, declaredSize uint64) []byte {
	t.Helper()
	var comp bytes.Buffer
	fw, err := flate.NewWriter(&comp, flate.BestCompression)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := io.WriteString(fw, content); err != nil {
		t.Fatal(err)
	}
	if err := fw.Close(); err != nil {
		t.Fatal(err)
	}
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	h := &zip.FileHeader{
		Name: name, Method: zip.Deflate, CRC32: crc32.ChecksumIEEE([]byte(content)),
		CompressedSize64: uint64(comp.Len()), UncompressedSize64: declaredSize,
	}
	w, err := zw.CreateRaw(h)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := w.Write(comp.Bytes()); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

func TestWalkRefusesWhenTheWholeArchiveExpandsTooFar(t *testing.T) {
	t.Parallel()

	lim := limits()
	lim.MaxEntryBytes = 1 << 16
	lim.MaxTotalBytes = 1 << 12 // 4 KiB in all
	lim.MaxRatio = 1_000_000
	var files []file
	for i := range 10 {
		files = append(files, file{name: "fil" + string(rune('a'+i)) + ".txt", data: strings.Repeat(string(rune('a'+i)), 1000)})
	}
	_, _, err := collect(t, build(t, files...), lim)
	if !errors.Is(err, ErrTooLarge) {
		t.Errorf("err = %v, want ErrTooLarge", err)
	}
}

func TestWalkRefusesWhatIsNotAnArchiveOrIsTooBig(t *testing.T) {
	t.Parallel()

	for name, data := range map[string][]byte{
		"empty":     {},
		"text":      []byte("dette er ikke et arkiv"),
		"truncated": build(t, file{name: "a.txt", data: strings.Repeat("a", 2000)})[:40],
	} {
		if _, _, err := collect(t, data, limits()); !errors.Is(err, ErrNotArchive) {
			t.Errorf("%s: err = %v, want ErrNotArchive", name, err)
		}
	}

	lim := limits()
	lim.MaxArchiveBytes = 100
	if _, _, err := collect(t, build(t, file{name: "a.txt", data: strings.Repeat("a", 300)}), lim); !errors.Is(err, ErrTooLarge) {
		t.Errorf("an archive over the size limit: err = %v, want ErrTooLarge", err)
	}
}

func TestWalkRefusesTooManyEntriesBeforeItReadsTheDirectory(t *testing.T) {
	t.Parallel()

	lim := limits()
	lim.MaxEntries = 5
	var files []file
	for i := range 6 {
		files = append(files, file{name: "f" + string(rune('a'+i)), data: "x"})
	}
	if _, _, err := collect(t, build(t, files...), lim); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("err = %v, want ErrTooManyEntries", err)
	}
	// Exactly the limit is fine.
	if _, _, err := collect(t, build(t, files[:5]...), lim); err != nil {
		t.Errorf("five entries: err = %v", err)
	}
}

func TestWalkSkipsAnEncryptedEntry(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	w, err := zw.CreateRaw(&zip.FileHeader{Name: "hemmelig.pdf", Method: zip.Store, Flags: 0x1, CompressedSize64: 4, UncompressedSize64: 4, CRC32: 1})
	if err != nil {
		t.Fatal(err)
	}
	_, _ = w.Write([]byte("abcd"))
	w, _ = zw.Create("vanlig.txt")
	_, _ = io.WriteString(w, "ok")
	_ = zw.Close()

	got, report, err := collect(t, buf.Bytes(), limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 || got["vanlig.txt"] != "ok" || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipEncrypted {
		t.Errorf("entries %v skipped %+v", keys(got), report.Skipped)
	}
}

func TestWalkOpensANestedArchiveUpToTheDepthLimit(t *testing.T) {
	t.Parallel()

	inner := build(t, file{name: "krav.txt", data: "innvendig"})
	middle := build(t, file{name: "inner.zip", data: string(inner)}, file{name: "midt.txt", data: "midt"})
	outer := build(t, file{name: "vedlegg.zip", data: string(middle)}, file{name: "topp.txt", data: "topp"})

	lim := limits()
	lim.MaxDepth = 3
	got, report, err := collect(t, outer, lim)
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	for _, want := range []string{"topp.txt", "vedlegg.zip/midt.txt", "vedlegg.zip/inner.zip/krav.txt"} {
		if _, ok := got[want]; !ok {
			t.Errorf("no entry %q in %v", want, keys(got))
		}
	}
	if len(report.Skipped) != 0 {
		t.Errorf("skipped = %+v", report.Skipped)
	}

	lim.MaxDepth = 2
	got, report, err = collect(t, outer, lim)
	if err != nil {
		t.Fatalf("Walk at depth 2: %v", err)
	}
	if _, ok := got["vedlegg.zip/inner.zip/krav.txt"]; ok {
		t.Errorf("the third level was opened at depth 2")
	}
	if len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipTooDeep {
		t.Errorf("skipped = %+v, want the inner archive skipped as too deep", report.Skipped)
	}
}

// Each name is bounded on its own, and the path of a file in a nested archive is the names of all its
// archives and of the file: a path no consumer can store is skipped, not delivered.
func TestWalkSkipsAFileWhoseNestedPathIsTooLong(t *testing.T) {
	t.Parallel()

	inner := build(t, file{name: strings.Repeat("b", 100) + ".txt", data: "x"}, file{name: "kort.txt", data: "y"})
	outer := build(t, file{name: strings.Repeat("a", 200) + ".zip", data: string(inner)})

	got, report, err := collect(t, outer, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if len(got) != 1 {
		t.Errorf("delivered %v, want only the file with the short path", keys(got))
	}
	if len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipBadName {
		t.Fatalf("skipped = %+v, want the long path skipped as a bad name", report.Skipped)
	}
	if n := len([]rune(report.Skipped[0].Path)); n > maxPathRunes {
		t.Errorf("the skipped path is reported with %d characters, more than a path may have (%d)", n, maxPathRunes)
	}
}

// What a nested archive expands to counts against the same budget, so wrapping a bomb in an
// archive does not hide it.
func TestWalkCountsANestedArchiveAgainstTheSameBudget(t *testing.T) {
	t.Parallel()

	inner := build(t, file{name: "stor.bin", data: strings.Repeat("\x00", 4<<20)})
	outer := build(t, file{name: "inne.zip", data: string(inner)})
	lim := limits()
	lim.MaxEntryBytes = 16 << 20
	lim.MaxTotalBytes = 64 << 20
	lim.MaxArchiveBytes = 8 << 20
	if _, _, err := collect(t, outer, lim); !errors.Is(err, ErrBomb) {
		t.Errorf("err = %v, want ErrBomb from the inner archive", err)
	}
}

func TestWalkStopsWhenTheCallbackFails(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a.txt", data: "a"}, file{name: "b.txt", data: "b"}, file{name: "c.txt", data: "c"})
	stop := errors.New("stop")
	calls := 0
	_, err := Walk(context.Background(), data, limits(), func(Entry) error {
		calls++
		if calls == 2 {
			return stop
		}
		return nil
	})
	if !errors.Is(err, stop) || calls != 2 {
		t.Errorf("err = %v after %d calls, want the callback's error after 2", err, calls)
	}
}

func TestWalkStopsOnACancelledContext(t *testing.T) {
	t.Parallel()

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, err := Walk(ctx, build(t, file{name: "a.txt", data: "a"}), limits(), func(Entry) error { return nil })
	if !errors.Is(err, context.Canceled) {
		t.Errorf("err = %v, want context.Canceled", err)
	}
}

func TestWalkGivesEachEntryItsOwnBytes(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a.txt", data: "aaa"}, file{name: "b.txt", data: "bbb"})
	var held [][]byte
	_, err := Walk(context.Background(), data, limits(), func(e Entry) error {
		held = append(held, e.Data)
		return nil
	})
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if string(held[0]) != "aaa" || string(held[1]) != "bbb" {
		t.Errorf("a callback that keeps its bytes sees %q and %q: they were reused", held[0], held[1])
	}
}

func TestEntryReportsItsHash(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a.txt", data: "abc"})
	var sum string
	if _, err := Walk(context.Background(), data, limits(), func(e Entry) error { sum = e.SHA256; return nil }); err != nil {
		t.Fatalf("Walk: %v", err)
	}
	// sha256("abc")
	if sum != "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" {
		t.Errorf("SHA256 = %q", sum)
	}
}

// The entry count that decides whether the directory is read at all must be the one the standard
// library reads, or a crafted archive could show this code a small number and the reader a large
// one. Where the library refuses an archive, entryCount must find no directory either.
func TestEntryCountAgreesWithTheStandardLibrary(t *testing.T) {
	t.Parallel()

	plain := build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"}, file{name: "c", data: "3"})
	empty := build(t)

	// A record in a file's data, before the real one: the search starts at the end.
	inData := build(t, file{name: "a", data: "PK\x05\x06" + strings.Repeat("\x00", 18)}, file{name: "b", data: "2"})

	// A comment that holds another end-of-directory record. With a comment length that does not
	// fit, the library gives up on the archive (it does not look further back), and so must
	// entryCount. With one that fits, the library reads the decoy as the record, and so must entryCount.
	withComment := func(comment []byte) []byte {
		var buf bytes.Buffer
		zw := zip.NewWriter(&buf)
		for _, name := range []string{"a", "b", "c", "d"} {
			w, _ := zw.Create(name)
			_, _ = w.Write([]byte(name))
		}
		if err := zw.SetComment(string(comment)); err != nil {
			t.Fatal(err)
		}
		_ = zw.Close()
		return buf.Bytes()
	}
	truncatedDecoy := withComment([]byte{'P', 'K', 5, 6, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF})
	fittingDecoy := withComment([]byte{'P', 'K', 5, 6, 0, 0, 0, 0, 7, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0})

	for name, data := range map[string][]byte{
		"plain": plain, "no entries": empty, "record inside a file": inData,
		"decoy with a truncated comment": truncatedDecoy, "decoy that fits": fittingDecoy,
		"text": []byte("not an archive at all, and long enough to hold a record at its end ........"), "empty": {},
	} {
		r, err := zip.NewReader(bytes.NewReader(data), int64(len(data)))
		got, ok := entryCount(data)
		switch {
		case err == nil && (!ok || got != len(r.File)):
			t.Errorf("%s: entryCount = %d, %v; the standard library reads %d entries", name, got, ok, len(r.File))
		case err != nil && ok && !errors.Is(err, zip.ErrInsecurePath):
			// The library refuses the archive, and entryCount may find a record the library then
			// cannot use (a decoy that fits points at nothing). Walk refuses it either way, after
			// the library does: what must never happen is a count smaller than the library's.
			t.Logf("%s: the library refuses (%v); entryCount found %d", name, err, got)
		}
	}
	if _, _, err := collect(t, truncatedDecoy, limits()); !errors.Is(err, ErrNotArchive) {
		t.Errorf("the archive with a truncated decoy: err = %v, want ErrNotArchive", err)
	}
	if _, _, err := collect(t, fittingDecoy, limits()); !errors.Is(err, ErrNotArchive) {
		t.Errorf("the archive with a decoy that fits: err = %v, want ErrNotArchive", err)
	}
}
