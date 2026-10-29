package ziputil

import (
	"archive/zip"
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"runtime"
	"slices"
	"strings"
	"testing"
)

// These tests pin the edges: the value a limit allows and the one it refuses, the characters a name may
// not keep, and the ways an end-of-directory record can lie. A limit that is off by one, or a check that
// is not made, passes every test of the ordinary case.

// withCounts returns the archive with the entry counts of its end record set to count.
func withCounts(data []byte, count uint16) []byte {
	out := slices.Clone(data)
	at := len(out) - eocdLen // no comment: the record is the last 22 bytes
	binary.LittleEndian.PutUint16(out[at+8:], count)
	binary.LittleEndian.PutUint16(out[at+10:], count)
	return out
}

// manyEntries is an archive of n empty files whose end record says it holds declared. The standard
// library writes more than 65535 entries with ZIP64 records, and this keeps the directory and replaces
// everything after it with a plain end record, so a reader that trusts the record sees a small number
// and a reader that counts sees n.
func manyEntries(t *testing.T, n int, declared uint16) []byte {
	t.Helper()
	data, size, offset := writeEntries(t, n)
	out := slices.Clone(data[:offset+size])
	rec := make([]byte, eocdLen)
	binary.LittleEndian.PutUint32(rec[0:], 0x06054b50)
	binary.LittleEndian.PutUint16(rec[8:], declared)
	binary.LittleEndian.PutUint16(rec[10:], declared)
	binary.LittleEndian.PutUint32(rec[12:], uint32(size))
	binary.LittleEndian.PutUint32(rec[16:], uint32(offset))
	return append(out, rec...)
}

// writeEntries writes n empty files, and returns the archive with the size and the offset of its directory.
func writeEntries(t *testing.T, n int) (data []byte, size, offset uint64) {
	t.Helper()
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for i := range n {
		if _, err := zw.CreateHeader(&zip.FileHeader{Name: fmt.Sprintf("f%d", i), Method: zip.Store}); err != nil {
			t.Fatal(err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	data = buf.Bytes()
	if n < 0xFFFF {
		last := data[len(data)-eocdLen:]
		return data, uint64(binary.LittleEndian.Uint32(last[12:])), uint64(binary.LittleEndian.Uint32(last[16:]))
	}
	// More than 65535 entries are written with a ZIP64 record before the plain one, which then holds the
	// maximum in every field: the real size and offset of the directory are in the ZIP64 record.
	const zip64EndLen = 56
	z64 := data[len(data)-eocdLen-zip64LocatorLen-zip64EndLen : len(data)-eocdLen-zip64LocatorLen]
	return data, binary.LittleEndian.Uint64(z64[40:]), binary.LittleEndian.Uint64(z64[48:])
}

func TestTheArchiveLimitIsExact(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a.txt", data: "innhold"})
	lim := limits()
	lim.MaxArchiveBytes = int64(len(data))
	if _, _, err := collect(t, data, lim); err != nil {
		t.Errorf("an archive of exactly the limit: %v", err)
	}
	lim.MaxArchiveBytes = int64(len(data)) - 1
	if _, _, err := collect(t, data, lim); !errors.Is(err, ErrTooLarge) {
		t.Errorf("an archive one byte over the limit: err = %v, want ErrTooLarge", err)
	}
}

func TestTheEntryLimitOfADirectoryIsExact(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"}, file{name: "c", data: "3"})
	lim := limits()
	lim.MaxEntries = 3
	if got, _, err := collect(t, data, lim); err != nil || len(got) != 3 {
		t.Errorf("three entries at a limit of three: %d, %v", len(got), err)
	}
	lim.MaxEntries = 2
	if _, _, err := collect(t, data, lim); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("three entries at a limit of two: err = %v, want ErrTooManyEntries", err)
	}
}

// 0xFFFF in the end record means the real count is in a ZIP64 record, and is more than any limit here
// allows: it is refused, whatever the limit is.
func TestAZip64DirectoryIsRefused(t *testing.T) {
	t.Parallel()

	data := withCounts(build(t, file{name: "a.txt", data: "x"}), 0xFFFF)
	lim := limits()
	lim.MaxEntries = 1 << 20
	if _, _, err := collect(t, data, lim); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("err = %v, want ErrTooManyEntries", err)
	}
}

// The count is checked before the directory is read: a record that claims sixty thousand entries over a
// directory of nothing is refused for its claim, and the library is not asked to allocate for it.
func TestADirectoryThatClaimsTooManyEntriesIsRefusedBeforeItIsRead(t *testing.T) {
	t.Parallel()

	rec := make([]byte, eocdLen)
	binary.LittleEndian.PutUint32(rec[0:], 0x06054b50)
	binary.LittleEndian.PutUint16(rec[8:], 60000)
	binary.LittleEndian.PutUint16(rec[10:], 60000)
	if _, _, err := collect(t, rec, limits()); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("err = %v, want ErrTooManyEntries, not a verdict on the directory", err)
	}
}

// The standard library compares the count it reads with the record's modulo 65536, so a directory of
// 65539 entries under a record that says 3 is accepted by it. The entries are counted after the
// directory is read, and the archive is refused.
func TestACountThatWrapsAroundIsRefused(t *testing.T) {
	t.Parallel()

	data := manyEntries(t, 65539, 3)
	if rec, ok := endRecord(data); !ok || rec.count != 3 {
		t.Fatalf("the record says %d, %v: the test needs it to say 3", rec.count, ok)
	}
	lim := limits()
	lim.MaxArchiveBytes = 32 << 20
	delivered := 0
	_, err := Walk(t.Context(), data, lim, func(Entry) error { delivered++; return nil })
	if !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("err = %v after %d files, want ErrTooManyEntries before any file is read", err, delivered)
	}
}

// A folder is an entry whose mode says so, whatever its name ends with.
func TestADirectoryByItsModeIsNotAFile(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "folder", mode: uint32(fs.ModeDir) | 0o755}, file{name: "a.txt", data: "x"})
	got, report, err := collect(t, data, limits())
	if err != nil || len(got) != 1 || len(report.Skipped) != 0 || report.Ignored != 0 {
		t.Errorf("delivered %v, skipped %+v, ignored %d, %v: a folder is nothing to report", keys(got), report.Skipped, report.Ignored, err)
	}
}

func TestTheEntryLimitOfAFileIsExact(t *testing.T) {
	t.Parallel()

	lim := limits()
	lim.MaxEntryBytes = 1000
	data := build(t, file{name: "exact.bin", data: strings.Repeat("0123456789", 100)}, file{name: "over.bin", data: strings.Repeat("0123456789", 100) + "x"})
	got, report, err := collect(t, data, lim)
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if _, ok := got["exact.bin"]; !ok {
		t.Errorf("a file of exactly the limit was not delivered: %v", keys(got))
	}
	if _, ok := got["over.bin"]; ok || len(report.Skipped) != 1 || report.Skipped[0].Path != "over.bin" || report.Skipped[0].Reason != SkipTooLarge {
		t.Errorf("a file one byte over: delivered %v, skipped %+v", keys(got), report.Skipped)
	}
}

func TestTheTotalBudgetIsExact(t *testing.T) {
	t.Parallel()

	files := []file{
		{name: "a.bin", data: strings.Repeat("a1", 50)}, {name: "b.bin", data: strings.Repeat("b2", 50)}, {name: "c.bin", data: strings.Repeat("c3", 50)},
	}
	lim := limits()
	lim.MaxTotalBytes = 300
	if got, _, err := collect(t, build(t, files...), lim); err != nil || len(got) != 3 {
		t.Errorf("300 bytes under a budget of 300: %d files, %v", len(got), err)
	}
	lim.MaxTotalBytes = 299
	if _, _, err := collect(t, build(t, files...), lim); !errors.Is(err, ErrTooLarge) {
		t.Errorf("300 bytes under a budget of 299: err = %v, want ErrTooLarge", err)
	}
}

// What is read from an entry is bounded by the limit whatever the directory declared. The library
// refuses a stream longer than its declared size, so this is tested on the reader itself.
func TestWhatIsReadOfAnEntryIsBounded(t *testing.T) {
	t.Parallel()

	lim := Limits{MaxEntryBytes: 100, MaxTotalBytes: 250}
	w := &walker{lim: lim}
	if body, err := w.readAll(strings.NewReader(strings.Repeat("a", 100))); err != nil || len(body) != 100 {
		t.Errorf("a stream of exactly the limit: %d bytes, %v", len(body), err)
	}
	w = &walker{lim: lim}
	if _, err := w.readAll(strings.NewReader(strings.Repeat("a", 101))); !errors.Is(err, ErrBomb) {
		t.Errorf("a stream one byte over: err = %v, want ErrBomb", err)
	}
	// 100 and 100 are 200 of 250; a third 100 is 300.
	w = &walker{lim: lim}
	for i := range 2 {
		if _, err := w.readAll(strings.NewReader(strings.Repeat("b", 100))); err != nil {
			t.Errorf("stream %d: %v", i+1, err)
		}
	}
	if _, err := w.readAll(strings.NewReader(strings.Repeat("c", 100))); !errors.Is(err, ErrTooLarge) {
		t.Errorf("the third stream, 300 in all: err = %v, want ErrTooLarge", err)
	}
}

// Only a file that really is a ZIP is opened: a file that is named like one and is not is a file.
func TestAFileNamedZipThatIsNotAZipIsAFile(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "vedlegg.zip", data: "this is not an archive"}, file{name: "ANNET.ZIP", data: "nor is this"})
	got, report, err := collect(t, data, limits())
	if err != nil || len(report.Skipped) != 0 {
		t.Fatalf("Walk: %v, skipped %+v", err, report.Skipped)
	}
	if got["vedlegg.zip"] != "this is not an archive" || got["ANNET.ZIP"] != "nor is this" {
		t.Errorf("delivered %v", got)
	}
}

func TestIsZipReadsTheHeader(t *testing.T) {
	t.Parallel()

	for header, want := range map[string]bool{
		"PK\x03\x04": true, "PK\x05\x06": true, "PK\x03\x04rest": true,
		"PK\x01\x02": false, "PX\x03\x04": false, "pk\x03\x04": false, "PK\x03": false, "PK": false, "": false,
	} {
		if got := isZip([]byte(header)); got != want {
			t.Errorf("isZip(%q) = %v, want %v", header, got, want)
		}
	}
}

// A hostile archive cannot make the report grow without limit.
func TestTheListOfSkippedFilesIsBounded(t *testing.T) {
	t.Parallel()

	var files []file
	for i := range 300 {
		files = append(files, file{name: fmt.Sprintf("../utenfor%d.txt", i), data: "x"})
	}
	lim := limits()
	lim.MaxEntries = 1000
	_, report, err := collect(t, build(t, files...), lim)
	if err != nil || len(report.Skipped) != maxSkipped {
		t.Errorf("skipped %d (%v), want the list held at %d", len(report.Skipped), err, maxSkipped)
	}
	// What does not fit in the list is counted, not lost.
	if report.SkippedMore != 100 {
		t.Errorf("SkippedMore = %d, want the 100 that did not fit", report.SkippedMore)
	}
}

func TestASkippedFileIsReportedWithItsReason(t *testing.T) {
	t.Parallel()

	_, report, err := collect(t, build(t, file{name: "../a.txt", data: "x"}), limits())
	if err != nil || len(report.Skipped) != 1 || report.Skipped[0] != (Skipped{Path: "../a.txt", Reason: SkipBadName}) {
		t.Errorf("skipped %+v, %v", report.Skipped, err)
	}
}

// A name keeps nothing that draws nothing or reorders what is around it, or it can pass for another.
func TestNamesLoseEveryBidirectionalAndInvisibleCharacter(t *testing.T) {
	t.Parallel()

	for _, r := range []rune{
		0x202a, 0x202b, 0x202c, 0x202d, 0x202e, // embeddings, pop and overrides
		0x2066, 0x2067, 0x2068, 0x2069, // isolates
		0x00ad, 0x200b, 0x200c, 0x200d, 0xfeff, 0x2060, // soft hyphen, zero-width characters, byte order mark, word joiner
	} {
		data := build(t, file{name: "kr" + string(r) + "av.txt", data: "x"})
		got, _, err := collect(t, data, limits())
		if _, ok := got["krav.txt"]; err != nil || !ok {
			t.Errorf("U+%04X: delivered %q, %v; want krav.txt", r, keys(got), err)
		}
	}
}

func TestANameThatIsNothingIsRefused(t *testing.T) {
	t.Parallel()

	for _, name := range []string{".", "./.", "/", "./"} {
		if name == "/" || name == "./" {
			continue // ends in a slash: a folder, which is nothing to report
		}
		got, report, err := collect(t, build(t, file{name: name, data: "x"}), limits())
		if err != nil || len(got) != 0 || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipBadName {
			t.Errorf("%q: delivered %v, skipped %+v, %v; want it refused as a bad name", name, keys(got), report.Skipped, err)
		}
	}
}

func TestTheNameLimitIsExact(t *testing.T) {
	t.Parallel()

	exact := strings.Repeat("å", maxPathRunes)
	got, report, err := collect(t, build(t, file{name: exact, data: "x"}, file{name: exact + "x", data: "y"}), limits())
	if err != nil {
		t.Fatal(err)
	}
	if _, ok := got[exact]; !ok {
		t.Errorf("a name of exactly %d characters was refused", maxPathRunes)
	}
	if len(got) != 1 || len(report.Skipped) != 1 || report.Skipped[0].Reason != SkipBadName {
		t.Errorf("a name one character over: delivered %d, skipped %+v", len(got), report.Skipped)
	}
}

// A run of bytes that are not text is one replacement character, not one for each byte.
func TestARunOfInvalidBytesInANameIsOneReplacement(t *testing.T) {
	t.Parallel()

	got, _, err := collect(t, build(t, file{name: "a\xff\xfe\xfdb.txt", data: "x"}), limits())
	if _, ok := got["a�b.txt"]; err != nil || !ok {
		t.Errorf("delivered %q, %v; want a\\ufffdb.txt", keys(got), err)
	}
}

func TestTheReportOfARefusedNameHasNoControlCharacters(t *testing.T) {
	t.Parallel()

	_, report, err := collect(t, build(t, file{name: "../a\x01\x1bb\n.txt", data: "x"}), limits())
	if err != nil || len(report.Skipped) != 1 {
		t.Fatalf("skipped %+v, %v", report.Skipped, err)
	}
	if got := report.Skipped[0].Path; got != "../ab.txt" {
		t.Errorf("reported as %q, want ../ab.txt", got)
	}
}

func TestTheReportOfARefusedNameIsClipped(t *testing.T) {
	t.Parallel()

	_, report, err := collect(t, build(t, file{name: "../" + strings.Repeat("x", 400), data: "x"}), limits())
	if err != nil || len(report.Skipped) != 1 || len([]rune(report.Skipped[0].Path)) != maxPathRunes {
		t.Errorf("skipped %+v, %v; want a path of %d characters", report.Skipped, err, maxPathRunes)
	}
}

func TestSystemFilesAreJunkWhereverTheyAre(t *testing.T) {
	t.Parallel()

	data := build(t,
		file{name: "__MACOSX/._a.txt", data: "x"}, file{name: "docs/__MACOSX/._b.txt", data: "x"}, file{name: "a/b/__macosx/c", data: "x"},
		file{name: "docs/.DS_Store", data: "x"}, file{name: "x/Thumbs.db", data: "x"}, file{name: "y/desktop.ini", data: "x"},
		file{name: "keep.txt", data: "x"},
	)
	got, report, err := collect(t, data, limits())
	if err != nil || len(got) != 1 || report.Ignored != 6 || len(report.Skipped) != 0 {
		t.Errorf("delivered %v, ignored %d, skipped %+v, %v; want only keep.txt, with the other six ignored", keys(got), report.Ignored, report.Skipped, err)
	}
}

// The end record is looked for in the last 65 KiB, because it may be followed by a comment.
func TestAnEndRecordFollowedByALongCommentIsFound(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	w, _ := zw.Create("a.txt")
	_, _ = w.Write([]byte("x"))
	if err := zw.SetComment(strings.Repeat("kommentar ", 400)); err != nil { // four kilobytes
		t.Fatal(err)
	}
	_ = zw.Close()
	if got, ok := entryCount(buf.Bytes()); !ok || got != 1 {
		t.Errorf("entryCount = %d, %v, want 1", got, ok)
	}
	if got, _, err := collect(t, buf.Bytes(), limits()); err != nil || len(got) != 1 {
		t.Errorf("Walk: %d, %v", len(got), err)
	}
}

// The record the library reads is the last one whose comment fits, and entryCount reads the same one.
// If it read an earlier one, a forged record in the comment could show it a small count while the
// library reads another.
func TestTheEndRecordReadIsTheLastOneThatFits(t *testing.T) {
	t.Parallel()

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
	fitting := withComment([]byte{'P', 'K', 5, 6, 0, 0, 0, 0, 7, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0})
	if got, ok := entryCount(fitting); !ok || got != 7 {
		t.Errorf("a forged record that fits: entryCount = %d, %v, want the forged 7, which is the one the library reads", got, ok)
	}
	truncated := withComment([]byte{'P', 'K', 5, 6, 0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xFF})
	if got, ok := entryCount(truncated); ok {
		t.Errorf("a forged record whose comment does not fit: entryCount = %d, true; the library gives up on it, and so must entryCount", got)
	}
}

// The directory is counted by its headers and not only by what the record says: 65539 entries under a
// record that says 3 are 65539.
func TestEntryCountCountsTheHeadersAndNotOnlyTheRecord(t *testing.T) {
	t.Parallel()

	if got, ok := EntryCount(manyEntries(t, 65539, 3)); !ok || got != 65539 {
		t.Errorf("EntryCount = %d, %v, want 65539", got, ok)
	}
	if got, ok := EntryCount(build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"})); !ok || got != 2 {
		t.Errorf("an ordinary archive: EntryCount = %d, %v, want 2", got, ok)
	}
}

// Self-extracting archives have something written before them, and the library then looks for the
// directory where it ends, not where the record says it starts. The count must find the same headers.
func TestEntryCountFindsTheDirectoryOfAnArchiveWithDataBeforeIt(t *testing.T) {
	t.Parallel()

	data := append([]byte(strings.Repeat("MZ self-extracting stub ", 5)), manyEntries(t, 65539, 3)...)
	if got, ok := EntryCount(data); !ok || got != 65539 {
		t.Errorf("EntryCount = %d, %v, want 65539", got, ok)
	}
	lim := limits()
	lim.MaxArchiveBytes = 32 << 20
	if _, err := Walk(t.Context(), data, lim, func(Entry) error { return nil }); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("Walk: err = %v, want ErrTooManyEntries", err)
	}
}

// The standard library allocates for every header it reads, and does so before any check made on the
// list it returns. A directory of 65539 entries is refused before it is read at all.
func TestAWrappedCountIsRefusedWithoutAllocatingForTheEntries(t *testing.T) {
	data := manyEntries(t, 65539, 3)
	lim := limits()
	lim.MaxArchiveBytes = 32 << 20

	// Not parallel: the figure is the process's, and a test running beside this one would count in it.
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	_, err := Walk(t.Context(), data, lim, func(Entry) error { return nil })
	runtime.ReadMemStats(&after)

	if !errors.Is(err, ErrTooManyEntries) {
		t.Fatalf("err = %v, want ErrTooManyEntries", err)
	}
	if spent := after.TotalAlloc - before.TotalAlloc; spent > 4<<20 {
		t.Errorf("refusing the directory allocated %d bytes: the library read its entries first (it needs about 13 MB for them)", spent)
	}
}

// An entry that fails its checksum is inflated whole before it is found out, and what was read is spent.
func TestWhatWasReadOfAnEntryThatFailsIsStillSpent(t *testing.T) {
	t.Parallel()

	failing := func(n int) io.Reader {
		return io.MultiReader(strings.NewReader(strings.Repeat("a", n)), errReader{})
	}
	w := &walker{lim: Limits{MaxEntryBytes: 100, MaxTotalBytes: 150}}
	if _, err := w.readAll(failing(80)); err == nil || errors.Is(err, ErrTooLarge) {
		t.Errorf("the first stream: err = %v, want its own failure", err)
	}
	if w.total != 80 {
		t.Errorf("spent %d, want the 80 that were read", w.total)
	}
	if _, err := w.readAll(failing(80)); !errors.Is(err, ErrTooLarge) {
		t.Errorf("the second stream, 160 in all: err = %v, want ErrTooLarge: failed reads count too", err)
	}
}

type errReader struct{}

func (errReader) Read([]byte) (int, error) { return 0, errors.New("zip: checksum error") }

// A nested file that starts like an archive and is not one is a damaged file. The files around it are
// still read, and it is listed as unreadable.
func TestADamagedNestedArchiveDoesNotRefuseTheRest(t *testing.T) {
	t.Parallel()

	data := build(t,
		file{name: "Vedlegg.zip", data: "PK\x03\x04 and then junk that is no directory"},
		file{name: "krav.txt", data: "innhold"},
	)
	got, report, err := collect(t, data, limits())
	if err != nil {
		t.Fatalf("Walk: %v", err)
	}
	if got["krav.txt"] != "innhold" || len(got) != 1 {
		t.Errorf("delivered %v, want krav.txt", keys(got))
	}
	if len(report.Skipped) != 1 || report.Skipped[0] != (Skipped{Path: "Vedlegg.zip", Reason: SkipUnreadable}) {
		t.Errorf("skipped %+v, want Vedlegg.zip as unreadable", report.Skipped)
	}
}

// A nested archive that is hostile, though, refuses the whole: what it holds counts against the same
// limits, and an archive that is over them says nothing good of the file it came in.
func TestAHostileNestedArchiveStillRefusesTheWhole(t *testing.T) {
	t.Parallel()

	inner := build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"}, file{name: "c", data: "3"})
	data := build(t, file{name: "inne.zip", data: string(inner)}, file{name: "krav.txt", data: "x"})
	lim := limits()
	lim.MaxEntries = 2 // the outer holds two entries, the inner three
	if _, _, err := collect(t, data, lim); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("err = %v, want ErrTooManyEntries from the nested archive", err)
	}
}

func TestTheReportOfARefusedNameIsInNormalFormAndHasNoBidirectionalControls(t *testing.T) {
	t.Parallel()

	_, report, err := collect(t, build(t, file{name: "../a\u202eb\u200bc.txt", data: "x"}, file{name: "../e\u0301.txt", data: "x"}), limits())
	if err != nil || len(report.Skipped) != 2 {
		t.Fatalf("skipped %+v, %v", report.Skipped, err)
	}
	if got := report.Skipped[0].Path; got != "../abc.txt" {
		t.Errorf("reported as %q, want ../abc.txt", got)
	}
	if got := report.Skipped[1].Path; got != "../\u00e9.txt" {
		t.Errorf("reported as %q, want the normalised ../\u00e9.txt", got)
	}
}

// The end record says how big the directory is, and the library does not go by it: it reads headers
// until one is bad. A record that says 46 bytes over twenty thousand headers is read as far as a directory
// of the allowed number of entries can be, and refused, with next to nothing allocated for the entries.
func TestADirectoryThatLiesAboutItsSizeIsRefusedWithoutBeingReadWhole(t *testing.T) {
	data := manyEntries(t, 20000, 3)
	binary.LittleEndian.PutUint32(data[len(data)-eocdLen+12:], 46) // the size of one header
	if got, ok := EntryCount(data); !ok || got > 3 {
		t.Fatalf("EntryCount = %d, %v: the test needs a record that hides the flood from the count", got, ok)
	}

	lim := limits()
	lim.MaxArchiveBytes = 16 << 20
	var before, after runtime.MemStats
	runtime.GC()
	runtime.ReadMemStats(&before)
	_, err := Walk(t.Context(), data, lim, func(Entry) error { return nil })
	runtime.ReadMemStats(&after)

	if !errors.Is(err, ErrTooManyEntries) {
		t.Fatalf("err = %v, want ErrTooManyEntries", err)
	}
	if spent := after.TotalAlloc - before.TotalAlloc; spent > 3<<20 {
		t.Errorf("refusing the directory allocated %d bytes: the library read the flood (20000 entries are about 5 MB)", spent)
	}
}

// A record that tells the library to look for a ZIP64 record, and a ZIP64 record that is there, can hold
// a count of 64 bits however small the count of the plain record is. The marker is a size of 0xFFFF (16
// bits of a 32-bit field, as the library compares it) or an offset of 0xFFFFFFFF, with the locator.
func TestAZip64RecordThatIsThereIsRefusedWhateverTheCountSays(t *testing.T) {
	t.Parallel()

	data, _, _ := writeEntries(t, 70000)
	at := len(data) - eocdLen
	for name, patch := range map[string]func([]byte){
		"a size of 0xFFFF": func(d []byte) {
			binary.LittleEndian.PutUint16(d[at+8:], 3)
			binary.LittleEndian.PutUint16(d[at+10:], 3)
			binary.LittleEndian.PutUint32(d[at+12:], 0xFFFF)
			binary.LittleEndian.PutUint32(d[at+16:], 0)
		},
		"an offset of 0xFFFFFFFF": func(d []byte) {
			binary.LittleEndian.PutUint16(d[at+8:], 3)
			binary.LittleEndian.PutUint16(d[at+10:], 3)
			binary.LittleEndian.PutUint32(d[at+12:], 46)
		},
	} {
		d := slices.Clone(data)
		patch(d)
		lim := limits()
		lim.MaxArchiveBytes = 64 << 20
		if _, err := Walk(t.Context(), d, lim, func(Entry) error { return nil }); !errors.Is(err, ErrTooManyEntries) {
			t.Errorf("%s: err = %v, want ErrTooManyEntries", name, err)
		}
	}
}

// A marker with no locator is only a number: the library does not look for a ZIP64 record either.
func TestAZip64MarkerWithNoLocatorIsNotAZip64Record(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a.txt", data: "x"})
	rec, ok := endRecord(data)
	if !ok || hasZip64Locator(data, rec.at) {
		t.Fatalf("the test needs an archive with no locator")
	}
	binary.LittleEndian.PutUint32(data[len(data)-eocdLen+12:], 0xFFFF)
	if got, _ := EntryCount(data); got == zip64Count {
		t.Errorf("EntryCount = %d: a marker with no locator was taken for a ZIP64 record", got)
	}
}

// The most the caller allows is allowed, with names of the longest length in the widest characters.
func TestAnArchiveWithAsManyEntriesAsAllowedAndTheLongestNamesIsRead(t *testing.T) {
	t.Parallel()

	var files []file
	for i := range 50 {
		files = append(files, file{name: strings.Repeat("\U0001F642", maxPathRunes-3) + fmt.Sprintf("%03d", i), data: "x"})
	}
	lim := limits()
	lim.MaxEntries = 50
	if got, _, err := collect(t, build(t, files...), lim); err != nil || len(got) != 50 {
		t.Errorf("50 entries at a limit of 50: %d, %v", len(got), err)
	}
}

func TestNewReaderSaysWhyItRefuses(t *testing.T) {
	t.Parallel()

	if _, err := NewReader([]byte("not an archive at all, but long enough to hold an end record in it...."), 10); !errors.Is(err, ErrNotArchive) {
		t.Errorf("text: err = %v, want ErrNotArchive", err)
	}
	four := build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"}, file{name: "c", data: "3"}, file{name: "d", data: "4"})
	if _, err := NewReader(four, 3); !errors.Is(err, ErrTooManyEntries) {
		t.Errorf("four entries at a limit of three: err = %v, want ErrTooManyEntries", err)
	}
	r, err := NewReader(four, 4)
	if err != nil || len(r.File) != 4 {
		t.Fatalf("four entries at a limit of four: %v", err)
	}
	// What is read after the directory is the entries, and is not held to the budget of the directory.
	body, err := r.File[0].Open()
	if err != nil {
		t.Fatal(err)
	}
	if got, _ := io.ReadAll(body); string(got) != "1" {
		t.Errorf("the first entry reads %q", got)
	}
}
