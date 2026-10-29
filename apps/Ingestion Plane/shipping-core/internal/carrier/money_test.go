package carrier

import "testing"

func TestParseDecimalToCents(t *testing.T) {
	tests := []struct {
		in   string
		want int64
	}{
		{"285.96", 28596},
		{"0.00", 0},
		{"100", 10000},
		{"0.005", 1}, // rounds to nearest cent
		{"not-a-number", 0},
		{"", 0},
	}
	for _, tt := range tests {
		if got := ParseDecimalToCents(tt.in); got != tt.want {
			t.Errorf("ParseDecimalToCents(%q) = %d, want %d", tt.in, got, tt.want)
		}
	}
}

func TestFloatToCents(t *testing.T) {
	tests := []struct {
		in   float64
		want int64
	}{
		{131.55, 13155},
		{0, 0},
		{99.999, 10000},
		{-10.50, -1050},
	}
	for _, tt := range tests {
		if got := FloatToCents(tt.in); got != tt.want {
			t.Errorf("FloatToCents(%v) = %d, want %d", tt.in, got, tt.want)
		}
	}
}
