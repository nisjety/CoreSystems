package demo

import (
	"encoding/json"
	"slices"
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/match"
)

func decode(t *testing.T, body string) (Request, []FieldError) {
	t.Helper()
	return DecodeMatchRequest([]byte(body))
}

func mustDecode(t *testing.T, body string) Request {
	t.Helper()
	r, errs := decode(t, body)
	if len(errs) != 0 {
		t.Fatalf("DecodeMatchRequest(%s): %+v", body, errs)
	}
	return r
}

func TestDecodeAppliesTheDefaults(t *testing.T) {
	t.Parallel()

	r := mustDecode(t, `{"profile":{"cpv":["72000000"]}}`)
	if r.Strictness != match.Balanced || r.StrictnessName != "balanced" || r.ShowAll || r.Limit != 50 || r.Scan != 300 {
		t.Errorf("request = %+v, want balanced, relevant only, limit 50, scan 300", r)
	}
	if !slices.Equal(r.Profile.CPV, []string{"72000000"}) {
		t.Errorf("cpv = %v", r.Profile.CPV)
	}
}

func TestDecodeNormalisesTheProfile(t *testing.T) {
	t.Parallel()

	r := mustDecode(t, `{
	  "profile": {
	    "org_number": "NO 974 760 673 MVA",
	    "cpv": ["45000000-7", "72212000", "45000000"],
	    "excluded_cpv": ["03000000"],
	    "nuts": ["no081", "NO0A2"],
	    "value_range": {"currency": "nok", "min": 200000, "max": 5000000},
	    "keywords": ["  Fagsystem ", "fagsystem", "drift"],
	    "held": ["startbank", "iso9001"],
	    "lead_times": {"startbank": {"days": 2}, "iso9001": {"days": 90, "equivalent_accepted": true}}
	  },
	  "strictness": "wide", "show": "all", "limit": 10, "scan": 40
	}`)
	p := r.Profile
	if p.OrgNumber != "974760673" {
		t.Errorf("org number = %q", p.OrgNumber)
	}
	if !slices.Equal(p.CPV, []string{"45000000", "72212000"}) || !slices.Equal(p.ExcludedCPV, []string{"03000000"}) {
		t.Errorf("cpv %v excluded %v: codes lose their check digit and appear once", p.CPV, p.ExcludedCPV)
	}
	if !slices.Equal(p.NUTS, []string{"NO081", "NO0A2"}) || !slices.Equal(p.Keywords, []string{"Fagsystem", "drift"}) {
		t.Errorf("nuts %v keywords %v", p.NUTS, p.Keywords)
	}
	if p.MinValue == nil || *p.MinValue != (eforms.Amount{Minor: 20_000_000, Currency: "NOK"}) ||
		p.MaxValue == nil || *p.MaxValue != (eforms.Amount{Minor: 500_000_000, Currency: "NOK"}) {
		t.Errorf("value range = %+v %+v, want 200 000 to 5 000 000 NOK in øre", p.MinValue, p.MaxValue)
	}
	if !p.Held[match.CapStartBank] || !p.Held[match.CapISO9001] || len(p.Held) != 2 {
		t.Errorf("held = %v", p.Held)
	}
	if p.LeadTimes[match.CapStartBank] != (match.LeadTime{Days: 2}) || p.LeadTimes[match.CapISO9001] != (match.LeadTime{Days: 90, EquivalentAccepted: true}) {
		t.Errorf("lead times = %v", p.LeadTimes)
	}
	if r.Strictness != match.Wide || !r.ShowAll || r.Limit != 10 || r.Scan != 40 {
		t.Errorf("request = %+v", r)
	}
}

func TestDecodeAcceptsAValueRangeWithOneBound(t *testing.T) {
	t.Parallel()

	r := mustDecode(t, `{"profile":{"keywords":["x1"],"value_range":{"currency":"NOK","max":1000000}}}`)
	if r.Profile.MinValue != nil || r.Profile.MaxValue == nil || r.Profile.MaxValue.Minor != 100_000_000 {
		t.Errorf("range = %+v %+v", r.Profile.MinValue, r.Profile.MaxValue)
	}
}

func TestDecodeRefusesWhatIsNotValid(t *testing.T) {
	t.Parallel()

	long := strings.Repeat("a", 81)
	cases := map[string]struct {
		body          string
		field, reason string
	}{
		"not json":          {`{`, "", "the body is not valid JSON"},
		"a json array":      {`[1]`, "", "the body must be a JSON object"},
		"two values":        {`{"profile":{"cpv":["72000000"]}} {}`, "", "the body has more than one JSON value"},
		"unknown field":     {`{"profile":{"cpv":["72000000"]},"surprise":1}`, "", "the body has a field this endpoint does not know"},
		"wrong type":        {`{"profile":{"cpv":"72000000"}}`, "profile.cpv", "has the wrong type"},
		"no profile":        {`{}`, "profile", "is required"},
		"nothing to match":  {`{"profile":{}}`, "profile", "needs at least one CPV code or one keyword to match on"},
		"bad cpv":           {`{"profile":{"cpv":["72000000","7200"]}}`, "profile.cpv[1]", "is not a CPV code"},
		"bad excluded cpv":  {`{"profile":{"cpv":["72000000"],"excluded_cpv":["x"]}}`, "profile.excluded_cpv[0]", "is not a CPV code"},
		"bad nuts":          {`{"profile":{"cpv":["72000000"],"nuts":["N0081"]}}`, "profile.nuts[0]", "is not a NUTS code"},
		"extra-regio":       {`{"profile":{"cpv":["72000000"],"nuts":["NOZZZ"]}}`, "profile.nuts[0]", "is extra-regio and names no place"},
		"bad org":           {`{"profile":{"cpv":["72000000"],"org_number":"12345"}}`, "profile.org_number", "is not a Norwegian organisation number"},
		"org check digit":   {`{"profile":{"cpv":["72000000"],"org_number":"974760674"}}`, "profile.org_number", "has a wrong check digit"},
		"empty keyword":     {`{"profile":{"keywords":["a1","  "]}}`, "profile.keywords[1]", "is empty"},
		"long keyword":      {`{"profile":{"keywords":["` + long + `"]}}`, "profile.keywords[0]", "is longer than 80 characters"},
		"unknown held":      {`{"profile":{"cpv":["72000000"],"held":["iso9001","magic"]}}`, "profile.held[1]", "is not a known capability"},
		"unknown lead time": {`{"profile":{"cpv":["72000000"],"lead_times":{"magic":{"days":1}}}}`, "profile.lead_times", "has a key that is not a known capability"},
		"no days":           {`{"profile":{"cpv":["72000000"],"lead_times":{"startbank":{}}}}`, "profile.lead_times.startbank.days", "is required"},
		"days out of range": {`{"profile":{"cpv":["72000000"],"lead_times":{"startbank":{"days":-1}}}}`, "profile.lead_times.startbank.days", "must be between 0 and 3650"},
		"no bounds":         {`{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK"}}}`, "profile.value_range", "needs a min or a max"},
		"bad currency":      {`{"profile":{"cpv":["72000000"],"value_range":{"currency":"KRONER","min":1}}}`, "profile.value_range.currency", "is not a 3-letter currency code"},
		"negative min":      {`{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","min":-1}}}`, "profile.value_range.min", "must be between 0 and 1000000000000"},
		"huge max":          {`{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","max":1000000000001}}}`, "profile.value_range.max", "must be between 0 and 1000000000000"},
		"min over max":      {`{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","min":5,"max":4}}}`, "profile.value_range", "has a min above its max"},
		"bad strictness":    {`{"profile":{"cpv":["72000000"]},"strictness":"loose"}`, "strictness", "must be one of strict, balanced, wide"},
		"bad show":          {`{"profile":{"cpv":["72000000"]},"show":"some"}`, "show", "must be one of relevant, all"},
		"limit zero":        {`{"profile":{"cpv":["72000000"]},"limit":0}`, "limit", "must be between 1 and 200"},
		"limit high":        {`{"profile":{"cpv":["72000000"]},"limit":201}`, "limit", "must be between 1 and 200"},
		"scan zero":         {`{"profile":{"cpv":["72000000"]},"scan":0}`, "scan", "must be between 1 and 1000"},
		"scan high":         {`{"profile":{"cpv":["72000000"]},"scan":1001}`, "scan", "must be between 1 and 1000"},
	}
	for name, tc := range cases {
		_, errs := decode(t, tc.body)
		if !slices.Contains(errs, FieldError{Field: tc.field, Reason: tc.reason}) {
			t.Errorf("%s: errors = %+v, want {%q %q}", name, errs, tc.field, tc.reason)
		}
	}
}

func TestDecodeRefusesTooManyEntries(t *testing.T) {
	t.Parallel()

	codes := make([]string, 51)
	for i := range codes {
		codes[i] = `"72000000"`
	}
	_, errs := decode(t, `{"profile":{"cpv":[`+strings.Join(codes, ",")+`]}}`)
	if !slices.Contains(errs, FieldError{Field: "profile.cpv", Reason: "has more than 50 entries"}) {
		t.Errorf("errors = %+v", errs)
	}
}

func TestDecodeReportsEveryProblemAtOnceButNoMoreThanTwenty(t *testing.T) {
	t.Parallel()

	_, errs := decode(t, `{"profile":{"cpv":["x"],"nuts":["y"],"org_number":"z"},"strictness":"q","limit":0}`)
	if len(errs) != 5 {
		t.Errorf("errors = %+v, want 5", errs)
	}

	bad := make([]string, 50)
	for i := range bad {
		bad[i] = `"x"`
	}
	list := strings.Join(bad, ",")
	_, errs = decode(t, `{"profile":{"cpv":[`+list+`],"nuts":[`+list+`]}}`)
	if len(errs) != maxFieldErrors {
		t.Errorf("errors = %d, want the first %d", len(errs), maxFieldErrors)
	}
}

// A rejected value is never echoed: a key pasted into the wrong field would be printed back by the
// error that refuses it, and into the log of whoever relays the response.
func TestDecodeNeverEchoesARejectedValue(t *testing.T) {
	t.Parallel()

	const marker = "sk-live-0123456789abcdef"
	for name, body := range map[string]string{
		"cpv":       `{"profile":{"cpv":["` + marker + `"]}}`,
		"nuts":      `{"profile":{"cpv":["72000000"],"nuts":["` + marker + `"]}}`,
		"org":       `{"profile":{"cpv":["72000000"],"org_number":"` + marker + `"}}`,
		"held":      `{"profile":{"cpv":["72000000"],"held":["` + marker + `"]}}`,
		"lead time": `{"profile":{"cpv":["72000000"],"lead_times":{"` + marker + `":{"days":1}}}}`,
		"strict":    `{"profile":{"cpv":["72000000"]},"strictness":"` + marker + `"}`,
		"currency":  `{"profile":{"cpv":["72000000"],"value_range":{"currency":"` + marker + `","min":1}}}`,
		"unknown":   `{"profile":{"cpv":["72000000"]},"` + marker + `":1}`,
		"type":      `{"profile":{"cpv":"` + marker + `"}}`,
		"keyword":   `{"profile":{"keywords":["` + strings.Repeat(marker, 8) + `"]}}`,
	} {
		_, errs := decode(t, body)
		if len(errs) == 0 {
			t.Errorf("%s: accepted", name)
			continue
		}
		out, _ := json.Marshal(errs)
		if strings.Contains(string(out), marker) {
			t.Errorf("%s: the errors echo the value: %s", name, out)
		}
	}
}

// The last value each limit accepts. A boundary that is off by one is a refusal of something valid
// or an acceptance of something that is not.
func TestDecodeAcceptsTheBoundaries(t *testing.T) {
	t.Parallel()

	cpvs := strings.Repeat(`"72000000",`, 49) + `"72000000"`
	kws := make([]string, 50)
	for i := range kws {
		kws[i] = `"k` + strings.Repeat("x", i) + `"`
	}
	for name, body := range map[string]string{
		"limit 1":            `{"profile":{"cpv":["72000000"]},"limit":1}`,
		"limit 200":          `{"profile":{"cpv":["72000000"]},"limit":200}`,
		"scan 1":             `{"profile":{"cpv":["72000000"]},"scan":1}`,
		"scan 1000":          `{"profile":{"cpv":["72000000"]},"scan":1000}`,
		"50 codes":           `{"profile":{"cpv":[` + cpvs + `]}}`,
		"50 keywords":        `{"profile":{"keywords":[` + strings.Join(kws, ",") + `]}}`,
		"keyword of 80":      `{"profile":{"keywords":["` + strings.Repeat("æ", 80) + `"]}}`,
		"lead time 0 days":   `{"profile":{"cpv":["72000000"],"lead_times":{"startbank":{"days":0}}}}`,
		"lead time 3650":     `{"profile":{"cpv":["72000000"],"lead_times":{"startbank":{"days":3650}}}}`,
		"min equals max":     `{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","min":5,"max":5}}}`,
		"zero min":           `{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","min":0}}}`,
		"the largest amount": `{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","max":1000000000000}}}`,
	} {
		if _, errs := decode(t, body); len(errs) != 0 {
			t.Errorf("%s: refused: %+v", name, errs)
		}
	}
	r := mustDecode(t, `{"profile":{"cpv":["72000000"],"value_range":{"currency":"NOK","max":1000000000000}}}`)
	if r.Profile.MaxValue.Minor != 100_000_000_000_000 {
		t.Errorf("the largest amount in øre = %d", r.Profile.MaxValue.Minor)
	}
}

func TestDecodeRefusesAnEmptyBody(t *testing.T) {
	t.Parallel()

	for _, body := range []string{"", "   ", "null"} {
		if _, errs := decode(t, body); len(errs) == 0 {
			t.Errorf("body %q accepted", body)
		}
	}
}

func TestDecodeCompanyRequestNormalisesTheNumber(t *testing.T) {
	t.Parallel()

	number, errs := DecodeCompanyRequest([]byte(`{"org_number":" NO 974 760 673 MVA "}`))
	if len(errs) != 0 || number != "974760673" {
		t.Errorf("number %q errors %+v", number, errs)
	}
}

func TestDecodeCompanyRequestRefusesWhatIsNotValid(t *testing.T) {
	t.Parallel()

	for name, tc := range map[string]struct {
		body          string
		field, reason string
	}{
		"missing":        {`{}`, "org_number", "is required"},
		"empty":          {`{"org_number":"  "}`, "org_number", "is required"},
		"null":           {`{"org_number":null}`, "org_number", "is required"},
		"check digit":    {`{"org_number":"974760674"}`, "org_number", "has a wrong check digit"},
		"not a number":   {`{"org_number":"abc"}`, "org_number", "is not a Norwegian organisation number"},
		"wrong type":     {`{"org_number":974760673}`, "org_number", "has the wrong type"},
		"unknown field":  {`{"org_number":"974760673","x":1}`, "", "the body has a field this endpoint does not know"},
		"not json":       {`{`, "", "the body is not valid JSON"},
		"empty body":     {``, "", "the body is not valid JSON"},
		"two values":     {`{"org_number":"974760673"} {}`, "", "the body has more than one JSON value"},
		"an array":       {`["974760673"]`, "", "the body must be a JSON object"},
		"a profile body": {`{"profile":{"cpv":["72000000"]}}`, "", "the body has a field this endpoint does not know"},
	} {
		number, errs := DecodeCompanyRequest([]byte(tc.body))
		if number != "" || !slices.Contains(errs, FieldError{Field: tc.field, Reason: tc.reason}) {
			t.Errorf("%s: number %q errors %+v, want {%q %q}", name, number, errs, tc.field, tc.reason)
		}
	}
}

func TestDecodeCompanyRequestNeverEchoesTheValue(t *testing.T) {
	t.Parallel()

	const marker = "sk-live-0123456789abcdef"
	for _, body := range []string{`{"org_number":"` + marker + `"}`, `{"` + marker + `":1}`} {
		_, errs := DecodeCompanyRequest([]byte(body))
		out, _ := json.Marshal(errs)
		if len(errs) == 0 || strings.Contains(string(out), marker) {
			t.Errorf("body %s: errors %s", body, out)
		}
	}
}
