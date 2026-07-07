package fedex

// Wire types for FedEx's Rates and Transit Times API
// (POST {base}/rate/v1/rates/quotes). Verified 2026-07 against FedEx's
// developer-portal docs and multiple published request/response examples:
// the request envelope (accountNumber.value, requestedShipment with
// shipper/recipient addresses, rateRequestType, requestedPackageLineItems
// with weight{units,value}) and the response envelope
// (output.rateReplyDetails[].{serviceType, serviceName,
// ratedShipmentDetails[].{rateType, totalNetCharge, currency}}) are both
// confirmed shapes.
//
// Best-effort (documented, not independently confirmed): pickupType
// values and the commit/dateDetail transit-info structure, which varies
// by service and route in published examples. Transit days are therefore
// left unset (0 = unknown) unless commit.transitDays parses — a visible
// gap in the UI rather than a fabricated number.
//
// The OAuth token endpoint (POST {base}/oauth/token, form-encoded
// grant_type=client_credentials&client_id&client_secret) is confirmed
// from FedEx's API Authorization docs.

type rateRequest struct {
	AccountNumber     accountNumber     `json:"accountNumber"`
	RequestedShipment requestedShipment `json:"requestedShipment"`
}

type accountNumber struct {
	Value string `json:"value"`
}

type requestedShipment struct {
	Shipper                   partyWithAddress `json:"shipper"`
	Recipient                 partyWithAddress `json:"recipient"`
	PickupType                string           `json:"pickupType"`
	RateRequestType           []string         `json:"rateRequestType"`
	RequestedPackageLineItems []packageLine    `json:"requestedPackageLineItems"`
}

type partyWithAddress struct {
	Address address `json:"address"`
}

type address struct {
	City        string `json:"city,omitempty"`
	PostalCode  string `json:"postalCode"`
	CountryCode string `json:"countryCode"`
}

type packageLine struct {
	Weight     weight      `json:"weight"`
	Dimensions *dimensions `json:"dimensions,omitempty"`
}

type weight struct {
	Units string  `json:"units"` // "KG"
	Value float64 `json:"value"`
}

// FedEx requires integer dimensions.
type dimensions struct {
	Length int    `json:"length"`
	Width  int    `json:"width"`
	Height int    `json:"height"`
	Units  string `json:"units"` // "CM"
}

type rateResponse struct {
	Output rateOutput `json:"output"`
}

type rateOutput struct {
	RateReplyDetails []rateReplyDetail `json:"rateReplyDetails"`
}

type rateReplyDetail struct {
	ServiceType          string                `json:"serviceType"`
	ServiceName          string                `json:"serviceName"`
	RatedShipmentDetails []ratedShipmentDetail `json:"ratedShipmentDetails"`
	Commit               *commit               `json:"commit"`
}

type ratedShipmentDetail struct {
	RateType       string  `json:"rateType"` // "ACCOUNT" or "LIST"
	TotalNetCharge float64 `json:"totalNetCharge"`
	Currency       string  `json:"currency"`
}

type commit struct {
	// TransitDays' inner shape varies by route in published examples;
	// description strings like "TWO_DAYS" are handled in fedex.go.
	TransitDays *transitDays `json:"transitDays"`
}

type transitDays struct {
	MinimumTransitTime string `json:"minimumTransitTime"` // e.g. "TWO_DAYS"
	Description        string `json:"description"`
}
