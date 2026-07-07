package carrier

import "strconv"

// ParseDecimalToCents converts a decimal-string amount (e.g. "285.96", the
// format UPS and Bring return prices in) to integer cents, matching
// Money's AmountCents convention. Returns 0 on a malformed amount rather
// than erroring — a price of 0 is visibly wrong in the UI and easy to
// spot, whereas failing an entire adapter over one unparseable field
// would hide every other valid quote in the same response.
func ParseDecimalToCents(amount string) int64 {
	f, err := strconv.ParseFloat(amount, 64)
	if err != nil {
		return 0
	}
	return FloatToCents(f)
}

// FloatToCents converts a decimal amount (e.g. FedEx's numeric
// totalNetCharge) to integer cents, rounding to the nearest cent.
func FloatToCents(f float64) int64 {
	if f < 0 {
		return int64(f*100 - 0.5)
	}
	return int64(f*100 + 0.5)
}
