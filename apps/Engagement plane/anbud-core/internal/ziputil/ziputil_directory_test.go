package ziputil

import (
	"archive/zip"
	"bytes"
	"encoding/binary"
	"errors"
	"io"
	"math/rand"
	"slices"
	"strings"
	"testing"
)

// The count of a directory and the budget of reading it are two defences, and each has to be shown to
// work with the other out of the way: a test that passes through either of them proves neither.

// The record has two counts, the entries on this disk and the entries in all, and an archive of one disk has
// them equal. A record whose two differ is believed on the total, which is what the library compares.
func TestTheCountOfARecordIsTheTotalAndNotTheCountOfThisDisk(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"})
	at := len(data) - eocdLen
	binary.LittleEndian.PutUint16(data[at+8:], 2)  // on this disk
	binary.LittleEndian.PutUint16(data[at+10:], 9) // in all
	if got, ok := EntryCount(data); !ok || got != 9 {
		t.Errorf("EntryCount = %d, %v, want the total of the record, 9", got, ok)
	}
}

// The headers are counted from where the record says the directory starts, whatever it says the directory
// weighs: a record that says the directory is far larger than the file is found by what it says of where it
// starts. (The opposite lie, a size that is too small, is the budget's to stop, and is tested below.)
func TestTheHeadersAreCountedWhateverTheRecordSaysOfTheSizeOfTheDirectory(t *testing.T) {
	t.Parallel()

	for _, size := range []uint32{0x7FFFFFFF, 0xFFFFFFF0} {
		data := manyEntries(t, 65539, 3)
		binary.LittleEndian.PutUint32(data[len(data)-eocdLen+12:], size)
		if got, ok := EntryCount(data); !ok || got != 65539 {
			t.Errorf("a size of %#x: EntryCount = %d, %v, want 65539", size, got, ok)
		}
	}
}

// Counting stops at the first header that is not one, and does not go on to read what follows as headers.
func TestTheHeadersAreCountedUpToTheFirstThatIsNotOne(t *testing.T) {
	t.Parallel()

	data := withCounts(build(t, file{name: "a", data: "1"}, file{name: "b", data: "2"}, file{name: "c", data: "3"}), 1)
	rec, ok := endRecord(data)
	if !ok {
		t.Fatal("no end record")
	}
	first := int(rec.offset)
	second := first + centralHeaderLen +
		int(binary.LittleEndian.Uint16(data[first+28:])) + int(binary.LittleEndian.Uint16(data[first+30:])) + int(binary.LittleEndian.Uint16(data[first+32:]))
	if got := binary.LittleEndian.Uint32(data[second:]); got != centralHeaderSignature {
		t.Fatalf("what the test takes for the second header starts with %#x", got)
	}
	copy(data[second:], "XXXX") // the lengths of the header stay as they were: a reader that does not look at the signature goes on
	if got, ok := EntryCount(data); !ok || got != 1 {
		t.Errorf("EntryCount = %d, %v, want 1: the headers are the ones up to the first that is damaged", got, ok)
	}
}

// A header is its fixed part, its name, its extra field and its comment, and the next begins after all of
// them: with a record that says one entry, three are found only by following the lengths.
func TestHeadersWithNamesExtraFieldsAndCommentsAreCountedByTheirLengths(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for _, h := range []zip.FileHeader{
		{Name: "a.txt", Extra: []byte{0xFE, 0xCA, 4, 0, 1, 2, 3, 4}, Comment: "kommentar"},
		{Name: "en-litt-lengre-fil.txt", Extra: []byte{0xFE, 0xCA, 10, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10}},
		{Name: "c.txt", Comment: "bare en kommentar"},
		{Name: "d.txt"},
	} {
		h.Method = zip.Store
		if _, err := zw.CreateHeader(&h); err != nil {
			t.Fatal(err)
		}
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	data := withCounts(buf.Bytes(), 1)
	if got, ok := EntryCount(data); !ok || got != 4 {
		t.Errorf("EntryCount = %d, %v, want 4: a header is not followed by the next when the name, the extra field or the comment is not skipped", got, ok)
	}
}

// A directory that says it is small, over a directory that is not, hides its entries from the count and
// from the check of the record. The library reads them all, and it reads them within the budget when the
// caller allows enough entries; what it read is counted when it has read it, and the archive is refused
// before any file of it is delivered.
func TestADirectoryThatUnderstatesItselfIsRefusedAfterItIsRead(t *testing.T) {
	t.Parallel()

	data := manyEntries(t, 65539, 3)
	binary.LittleEndian.PutUint32(data[len(data)-eocdLen+12:], 46) // the size of one header
	if got, ok := EntryCount(data); !ok || got > 3 {
		t.Fatalf("EntryCount = %d, %v: the test needs a record that hides the entries from the count", got, ok)
	}
	lim := limits()
	lim.MaxArchiveBytes = 32 << 20
	lim.MaxEntries = 4000
	lim.MaxTotalEntries = 1 << 20 // so that the count of the walk cannot be what refuses it
	if budget := directoryBytesBase + int64(lim.MaxEntries)*directoryBytesPerEntry; budget < 65539*(centralHeaderLen+8) {
		t.Fatalf("the budget of %d bytes does not hold the directory: the test would be won by the budget and not by the count", budget)
	}
	delivered := 0
	_, err := Walk(t.Context(), data, lim, func(Entry) error { delivered++; return nil })
	if !errors.Is(err, ErrTooManyEntries) || delivered != 0 {
		t.Errorf("err = %v after %d files, want ErrTooManyEntries before any file is delivered", err, delivered)
	}
}

// The budget of the directory is the room for the entries the caller allows, and the end of the file: an end
// record that is far from the end, behind a long comment, is read for a caller who allows a single entry.
func TestAnEndRecordBehindALongCommentIsReadForAVerySmallLimit(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	w, err := zw.Create("a.txt")
	if err != nil {
		t.Fatal(err)
	}
	_, _ = w.Write([]byte("x"))
	if err := zw.SetComment(strings.Repeat("k", 60000)); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	r, err := NewReader(buf.Bytes(), 1)
	if err != nil || len(r.File) != 1 {
		t.Fatalf("NewReader = %v: an end record 60 KB from the end is refused for the room it takes to find it", err)
	}
}

// The room for the directory is not a ceiling on the entries: a file much larger than it is read whole, when
// the directory is.
func TestAnEntryLargerThanTheBudgetOfTheDirectoryIsReadWhole(t *testing.T) {
	t.Parallel()

	big := make([]byte, 400<<10) // incompressible, so the archive holds all of it
	_, _ = rand.New(rand.NewSource(1)).Read(big)
	data := build(t, file{name: "stor.bin", data: string(big)})
	r, err := NewReader(data, 3)
	if err != nil {
		t.Fatal(err)
	}
	if budget := int64(directoryBytesBase + 3*directoryBytesPerEntry); int64(len(big)) <= budget {
		t.Fatalf("the file is %d bytes and the budget %d: the test needs a file that is over it", len(big), budget)
	}
	rc, err := r.File[0].Open()
	if err != nil {
		t.Fatal(err)
	}
	defer rc.Close()
	got, err := io.ReadAll(rc)
	if err != nil || !slices.Equal(got, big) {
		t.Errorf("read %d of %d bytes, %v: the entry is held to the budget of the directory", len(got), len(big), err)
	}
}

// With a ZIP64 locator in front of it, every one of the markers of the plain record sends the reader to a
// ZIP64 record, whose count has 64 bits: the count answers with more than any limit allows.
func TestEveryMarkerOfTheRecordWithALocatorIsAZip64RecordToTheCount(t *testing.T) {
	t.Parallel()

	data, _, _ := writeEntries(t, 70000) // ends with a ZIP64 record, its locator and the plain record
	at := len(data) - eocdLen
	if !hasZip64Locator(data, at) {
		t.Fatal("the test needs an archive with a locator")
	}
	for name, patch := range map[string]func([]byte){
		"a size of 0xFFFF": func(d []byte) {
			binary.LittleEndian.PutUint32(d[at+12:], 0xFFFF)
			binary.LittleEndian.PutUint32(d[at+16:], 0)
		},
		"a size of 0xFFFFFFFF": func(d []byte) {
			binary.LittleEndian.PutUint32(d[at+12:], 0xFFFFFFFF)
			binary.LittleEndian.PutUint32(d[at+16:], 0)
		},
		"an offset of 0xFFFFFFFF": func(d []byte) {
			binary.LittleEndian.PutUint32(d[at+12:], 46)
			binary.LittleEndian.PutUint32(d[at+16:], 0xFFFFFFFF)
		},
	} {
		d := slices.Clone(data)
		binary.LittleEndian.PutUint16(d[at+8:], 3)
		binary.LittleEndian.PutUint16(d[at+10:], 3)
		patch(d)
		if got, ok := EntryCount(d); !ok || got != zip64Count {
			t.Errorf("%s: EntryCount = %d, %v, want %d: a marker with a locator is a ZIP64 record", name, got, ok, zip64Count)
		}
	}
}

// Each marker alone is not enough: with no locator in front of the record, the library does not look for a
// ZIP64 record, and the count does not either.
func TestEveryMarkerOfTheRecordWithNoLocatorIsOnlyANumber(t *testing.T) {
	t.Parallel()

	data := build(t, file{name: "a.txt", data: "x"})
	at := len(data) - eocdLen
	if hasZip64Locator(data, at) {
		t.Fatal("the test needs an archive with no locator")
	}
	for name, patch := range map[string]func([]byte){
		"a size of 0xFFFF":        func(d []byte) { binary.LittleEndian.PutUint32(d[at+12:], 0xFFFF) },
		"a size of 0xFFFFFFFF":    func(d []byte) { binary.LittleEndian.PutUint32(d[at+12:], 0xFFFFFFFF) },
		"an offset of 0xFFFFFFFF": func(d []byte) { binary.LittleEndian.PutUint32(d[at+16:], 0xFFFFFFFF) },
	} {
		d := slices.Clone(data)
		patch(d)
		if got, _ := EntryCount(d); got == zip64Count {
			t.Errorf("%s: EntryCount = %d: a marker with no locator was taken for a ZIP64 record", name, got)
		}
	}
}

// A locator is the twenty bytes in front of the record, and a file may begin with it: the locator that is
// the first twenty bytes of the file is found, and one that would have to start before the file is not.
func TestALocatorAtTheVeryStartOfTheFileIsFound(t *testing.T) {
	t.Parallel()

	locator := make([]byte, zip64LocatorLen)
	binary.LittleEndian.PutUint32(locator, zip64LocatorSignature)
	rec := make([]byte, eocdLen)
	binary.LittleEndian.PutUint32(rec[0:], 0x06054b50)
	binary.LittleEndian.PutUint32(rec[12:], 0xFFFFFFFF)

	if got, ok := EntryCount(append(slices.Clone(locator), rec...)); !ok || got != zip64Count {
		t.Errorf("a locator that is the first twenty bytes: EntryCount = %d, %v, want %d", got, ok, zip64Count)
	}
	if got, _ := EntryCount(append(slices.Clone(locator[1:]), rec...)); got == zip64Count {
		t.Errorf("nineteen bytes in front of the record are not a locator, and EntryCount = %d", got)
	}
}

// countingReader hands out up to left bytes and counts what is taken from it.
type countingReader struct{ left, taken int64 }

func (c *countingReader) Read(p []byte) (int, error) {
	if c.left <= 0 {
		return 0, io.EOF
	}
	n := min(int64(len(p)), c.left)
	for i := range p[:n] {
		p[i] = 'a'
	}
	c.left -= n
	c.taken += n
	return int(n), nil
}

// A stream is read to one byte past the limit and no further: the byte that shows it is over, and not a
// stream that goes on.
func TestWhatIsTakenFromAStreamIsAtMostOneByteOverTheLimit(t *testing.T) {
	t.Parallel()

	w := &walker{lim: Limits{MaxEntryBytes: 100, MaxTotalBytes: 1 << 30}}
	src := &countingReader{left: 10_000}
	if _, err := w.readAll(src); !errors.Is(err, ErrBomb) {
		t.Fatalf("err = %v, want ErrBomb", err)
	}
	if src.taken != 101 {
		t.Errorf("took %d bytes of a stream of 10000 with a limit of 100, want 101", src.taken)
	}
}
