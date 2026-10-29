package profilesuggest

import (
	"reflect"
	"testing"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/incumbency"
)

const org = "923456783"

func awardTo(cpv []string, nuts []string, winners ...string) incumbency.Award {
	a := incumbency.Award{CPV: cpv, NUTS: nuts}
	for _, w := range winners {
		a.Winners = append(a.Winners, eforms.Winner{Name: "Winner", OrgNumber: w})
	}
	return a
}

func TestOnlyAwardsNamingTheOrganisationCount(t *testing.T) {
	t.Parallel()

	awards := []incumbency.Award{
		awardTo([]string{"72212000-7"}, []string{"NO081"}, "923 456 783"),
		awardTo([]string{"45000000"}, []string{"NO02"}, "999999999"),
		awardTo([]string{"72212000"}, []string{"NO081"}, "923456783", "888888888"),
	}
	s := FromWins(awards, org)
	if s.Wins != 2 {
		t.Fatalf("wins = %d, want the two awards that name the organisation", s.Wins)
	}
	if !reflect.DeepEqual(s.CPV, []string{"72212000"}) {
		t.Errorf("cpv = %v: the suffix is dropped and the other buyer's award is not counted", s.CPV)
	}
	if !reflect.DeepEqual(s.NUTS, []string{"NO081"}) {
		t.Errorf("nuts = %v", s.NUTS)
	}
}

func TestCodesAreCountedOncePerAwardAndRankedByCount(t *testing.T) {
	t.Parallel()

	awards := []incumbency.Award{
		awardTo([]string{"72212000", "72212000-1"}, []string{"NO081", "NO081"}, org),
		awardTo([]string{"45000000"}, []string{"NO081"}, org),
		awardTo([]string{"45000000"}, []string{"NO02"}, org),
	}
	s := FromWins(awards, org)
	if !reflect.DeepEqual(s.CPV, []string{"45000000", "72212000"}) {
		t.Errorf("cpv = %v, want the most frequent first, a repeat within one award counted once", s.CPV)
	}
	if !reflect.DeepEqual(s.NUTS, []string{"NO081", "NO02"}) {
		t.Errorf("nuts = %v", s.NUTS)
	}
}

func TestEachListIsCapped(t *testing.T) {
	t.Parallel()

	var cpv []string
	for i := 0; i < MaxCodes+5; i++ {
		cpv = append(cpv, "7000000"+string(rune('0'+i)))
	}
	s := FromWins([]incumbency.Award{awardTo(cpv, nil, org)}, org)
	if len(s.CPV) != MaxCodes {
		t.Errorf("cpv has %d codes, want at most %d", len(s.CPV), MaxCodes)
	}
}

func TestNoWinsSuggestsNothingAndAnEmptyListIsNotNil(t *testing.T) {
	t.Parallel()

	s := FromWins(nil, org)
	if s.Wins != 0 || s.CPV == nil || s.NUTS == nil {
		t.Errorf("suggestion = %+v, want no wins and empty (not nil) lists for the JSON", s)
	}
}

func TestProfileRecordsWhereEachListCameFrom(t *testing.T) {
	t.Parallel()

	p := FromWins([]incumbency.Award{awardTo([]string{"72212000"}, []string{"NO081"}, org)}, org).Profile("balanced")
	sources := p["sources"].(map[string]string)
	if sources["cpv"] != SourceWins || sources["nuts"] != SourceWins || sources["orgNumber"] != SourceOrganisation {
		t.Errorf("sources = %v", sources)
	}
	if p["orgNumber"] != org || p["strictness"] != "balanced" {
		t.Errorf("profile = %v", p)
	}
	if p["confirmed"] != false {
		t.Errorf("a suggestion must not be confirmed: %v", p["confirmed"])
	}
	if p["suggestedFrom"] == nil {
		t.Errorf("the suggestion does not say how many wins it came from")
	}
}
