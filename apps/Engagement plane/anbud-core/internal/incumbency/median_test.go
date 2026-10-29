package incumbency

import "testing"

// The median of the tender counts is the middle one, and for an even number of counts the mean of
// the two in the middle: a count of two and a count of four give three, not four. Taking the upper of
// the two would overstate how hard a buyer's competitions are.
func TestMedianOfTenderCounts(t *testing.T) {
	t.Parallel()

	for name, c := range map[string]struct {
		in   []int
		want float64
	}{
		"one":              {[]int{4}, 4},
		"odd":              {[]int{7, 3, 5}, 5},
		"even":             {[]int{4, 2}, 3},
		"even, a half":     {[]int{2, 3}, 2.5},
		"even, four":       {[]int{1, 9, 3, 5}, 4},
		"equal":            {[]int{6, 6}, 6},
		"a zero count":     {[]int{0, 4}, 2},
		"unsorted, repeat": {[]int{5, 1, 5, 1, 5}, 5},
	} {
		if got := median(c.in); got != c.want {
			t.Errorf("%s: median(%v) = %v, want %v", name, c.in, got, c.want)
		}
	}
}

// The median does not reorder the caller's slice.
func TestMedianLeavesItsInputAlone(t *testing.T) {
	t.Parallel()

	in := []int{9, 1, 5}
	median(in)
	if in[0] != 9 || in[1] != 1 || in[2] != 5 {
		t.Errorf("input was reordered: %v", in)
	}
}
