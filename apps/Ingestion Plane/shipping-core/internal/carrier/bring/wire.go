package bring

// Wire types for Bring's Shipping Guide API 2.0 — these mirror Bring's own
// JSON shape, not our domain model (see carrier.Quote etc. for that).
// Kept in their own file so the adapter logic in bring.go stays readable.
//
// Verification status (2026-07-04): CONFIRMED against Bring's own published
// OpenAPI spec (github.com/jentic/jentic-public-apis, apis/openapi/bring.com,
// components.schemas.{Shipping_guide_request,Consignment,Package,
// ProductIdType,ProductOutputType,PriceType,ErrorType}), and cross-checked
// against two independent production implementations that call the same
// endpoint and parse the same response shape: verbb/shippy (PHP, MIT) and
// joroinnoroff/ehtepper (TypeScript). This replaced an earlier best-effort
// reconstruction that had three bugs a live test caught on 2026-07-04:
// the wrong base path (missing /api/v2), a non-existent consignment-level
// customerNumber (it actually lives per-requested-product), and missing
// package dimensions (Bring requires height/width/length, not just weight).
type shippingGuideRequest struct {
	Consignments []consignmentRequest `json:"consignments"`
}

// consignmentRequest's required fields per the OpenAPI schema:
// fromCountryCode, fromPostalCode, toCountryCode, toPostalCode, packages,
// products. shippingDate is optional — omitted here, Bring defaults it to
// now().
type consignmentRequest struct {
	ConsignmentID   string            `json:"id"`
	FromPostalCode  string            `json:"fromPostalCode"`
	FromCountryCode string            `json:"fromCountryCode"`
	ToPostalCode    string            `json:"toPostalCode"`
	ToCountryCode   string            `json:"toCountryCode"`
	Packages        []packageRequest  `json:"packages"`
	Products        []productRequest  `json:"products"`
}

// packageRequest — Height/Width/Length in centimeters, GrossWeight in
// GRAMS (schema: "The weight of the package in gram" — a kg value here
// would silently under-price by 1000x, so buildRequest converts).
type packageRequest struct {
	ID          string  `json:"id"`
	GrossWeight float64 `json:"grossWeight"`
	Height      float64 `json:"height"`
	Width       float64 `json:"width"`
	Length      float64 `json:"length"`
}

// productRequest asks Bring to price one named service. Unlike UPS/DHL/
// FedEx's Shop-style endpoints, Bring has no "quote every product for this
// lane" mode — ProductIdType.id is a required field, so a rate-shopping
// caller must enumerate candidate product codes and let Bring report which
// ones don't apply (via productResponse.Errors) rather than expect Bring to
// pick them. See knownProductCodes in bring.go for the enumerated list.
type productRequest struct {
	ID             string `json:"id"`
	CustomerNumber string `json:"customerNumber,omitempty"`
}

// shippingGuideResponse mirrors the confirmed response shape: a list of
// consignments, each with priced (or per-product-errored) products.
type shippingGuideResponse struct {
	Consignments []consignmentResponse `json:"consignments"`
	UniqueID     string                `json:"uniqueId"`
}

type consignmentResponse struct {
	ConsignmentID string            `json:"id"`
	Products      []productResponse `json:"products"`
}

// productResponse: a product with no valid price/errors for this lane is
// normal (e.g. OUTSIDE_COVERAGE_AREA, INVALID_COUNTRY_PAIR) — Bring reports
// this per-product inside a 200 response rather than failing the request.
type productResponse struct {
	ID               string                    `json:"id"`
	ProductionCode   string                    `json:"productionCode"`
	Price            *priceResponse            `json:"price"`
	ExpectedDelivery *expectedDeliveryResponse `json:"expectedDelivery"`
	Errors           []productErrorResponse    `json:"errors"`
}

type productErrorResponse struct {
	Code        string `json:"code"`
	ErrorCode   string `json:"errorCode"`
	Description string `json:"description"`
}

type priceResponse struct {
	ListPrice struct {
		CurrencyCode                string `json:"currencyCode"`
		PriceWithAdditionalServices struct {
			AmountWithVAT string `json:"amountWithVAT"`
		} `json:"priceWithAdditionalServices"`
	} `json:"listPrice"`
}

type expectedDeliveryResponse struct {
	AlternativeDeliveryDates []struct {
		WorkingDays                   string `json:"workingDays"`
		FormattedExpectedDeliveryDate string `json:"formattedExpectedDeliveryDate"`
	} `json:"alternativeDeliveryDates"`
}
