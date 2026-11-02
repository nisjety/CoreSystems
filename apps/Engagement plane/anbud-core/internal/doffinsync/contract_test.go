package doffinsync

import (
	"reflect"
	"strings"
	"testing"
	"time"
)

// T40
func TestDayOutcomeHasNoBodyOrContactField(t *testing.T) {
	t.Parallel()

	forbidden := []string{"body", "raw", "name", "email", "phone", "contact", "address"}
	for _, typ := range []reflect.Type{
		reflect.TypeOf(DayOutcome{}),
		reflect.TypeOf(Summary{}),
		reflect.TypeOf(RunOutcome{}),
		reflect.TypeOf(Attempt{}),
	} {
		checkReportFields(t, typ, forbidden)
	}
}

// checkReportFields walks typ and its composite fields. It fails on a byte
// slice or on a field whose name suggests a body or a contact detail.
func checkReportFields(t *testing.T, typ reflect.Type, forbidden []string) {
	t.Helper()
	for i := 0; i < typ.NumField(); i++ {
		f := typ.Field(i)
		lower := strings.ToLower(f.Name)
		for _, bad := range forbidden {
			if strings.Contains(lower, bad) {
				t.Errorf("%s.%s looks like a body or contact field", typ.Name(), f.Name)
			}
		}
		ft := f.Type
		if ft.Kind() == reflect.Slice && ft.Elem().Kind() == reflect.Uint8 {
			t.Errorf("%s.%s is a byte slice", typ.Name(), f.Name)
		}
		for ft.Kind() == reflect.Slice || ft.Kind() == reflect.Pointer {
			ft = ft.Elem()
		}
		if ft.Kind() == reflect.Struct && ft != reflect.TypeOf(time.Time{}) {
			checkReportFields(t, ft, forbidden)
		}
	}
}

// T42
func TestNewRejectsNilDependencies(t *testing.T) {
	t.Parallel()

	full := newHarness().deps()
	nilOne := map[string]func(*Deps){
		"Search":   func(d *Deps) { d.Search = nil },
		"Download": func(d *Deps) { d.Download = nil },
		"Pages":    func(d *Deps) { d.Pages = nil },
		"Notices":  func(d *Deps) { d.Notices = nil },
		"Runs":     func(d *Deps) { d.Runs = nil },
		"Days":     func(d *Deps) { d.Days = nil },
	}
	for name, mutate := range nilOne {
		d := full
		mutate(&d)
		if _, err := New(d, Options{Mode: ModeSync}); err == nil {
			t.Errorf("New accepted a nil %s dependency", name)
		}
	}
	for _, mode := range []Mode{"", "bogus"} {
		if _, err := New(full, Options{Mode: mode}); err == nil {
			t.Errorf("New accepted mode %q", mode)
		}
	}
	if _, err := New(full, Options{Mode: ModeCensus}); err != nil {
		t.Errorf("New rejected a complete census configuration: %v", err)
	}
}
