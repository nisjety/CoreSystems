package ups

// Wire types for UPS's Rating API. Shapes verified against UPS's official
// OpenAPI spec (github.com/UPS-API/api-documentation, Rating.yaml,
// fetched 2026-07-02): servers wwwcie.ups.com/api (CIE/test) and
// onlinetools.ups.com/api (production); path /rating/{version}/{requestoption}
// with version=v2409 (the only documented valid value) and
// requestoption=Shop (all services, no transit) — Shoptimeintransit is the
// documented upgrade path when we want transit times for every service.
// In v2409, RatedShipment is always a JSON array.
//
// The OAuth token endpoint (/security/v1/oauth/token, Basic auth,
// grant_type=client_credentials) is confirmed from UPS's
// OAuthClientCredentials.yaml in the same repo.

type rateRequestEnvelope struct {
	RateRequest rateRequest `json:"RateRequest"`
}

type rateRequest struct {
	Request  requestSection `json:"Request"`
	Shipment shipment       `json:"Shipment"`
}

type requestSection struct {
	TransactionReference transactionReference `json:"TransactionReference"`
}

type transactionReference struct {
	CustomerContext string `json:"CustomerContext"`
}

type shipment struct {
	Shipper  party         `json:"Shipper"`
	ShipTo   party         `json:"ShipTo"`
	ShipFrom party         `json:"ShipFrom"`
	Package  []packageItem `json:"Package"`
}

type party struct {
	Name          string  `json:"Name,omitempty"`
	ShipperNumber string  `json:"ShipperNumber,omitempty"`
	Address       address `json:"Address"`
}

type address struct {
	City        string `json:"City,omitempty"`
	PostalCode  string `json:"PostalCode"`
	CountryCode string `json:"CountryCode"`
}

type packageItem struct {
	PackagingType codeDescription `json:"PackagingType"`
	Dimensions    dimensions      `json:"Dimensions"`
	PackageWeight packageWeight   `json:"PackageWeight"`
}

type codeDescription struct {
	Code        string `json:"Code"`
	Description string `json:"Description,omitempty"`
}

type dimensions struct {
	UnitOfMeasurement codeDescription `json:"UnitOfMeasurement"`
	Length            string          `json:"Length"`
	Width             string          `json:"Width"`
	Height            string          `json:"Height"`
}

type packageWeight struct {
	UnitOfMeasurement codeDescription `json:"UnitOfMeasurement"`
	Weight            string          `json:"Weight"`
}

type rateResponseEnvelope struct {
	RateResponse rateResponse `json:"RateResponse"`
}

type rateResponse struct {
	RatedShipment []ratedShipment `json:"RatedShipment"`
}

type ratedShipment struct {
	Service            codeDescription     `json:"Service"`
	TotalCharges       *totalCharges       `json:"TotalCharges"`
	GuaranteedDelivery *guaranteedDelivery `json:"GuaranteedDelivery"`
}

type totalCharges struct {
	CurrencyCode  string `json:"CurrencyCode"`
	MonetaryValue string `json:"MonetaryValue"`
}

type guaranteedDelivery struct {
	BusinessDaysInTransit string `json:"BusinessDaysInTransit"`
	ScheduledDeliveryDate string `json:"ScheduledDeliveryDate"`
}

// serviceNames maps UPS service codes to display names for the services
// most relevant from a Norwegian origin. Codes not in the map fall back
// to "UPS <code>". Source: UPS Rating API appendix (service codes).
var serviceNames = map[string]string{
	"07": "UPS Worldwide Express",
	"08": "UPS Worldwide Expedited",
	"11": "UPS Standard",
	"54": "UPS Worldwide Express Plus",
	"65": "UPS Express Saver",
	"70": "UPS Access Point Economy",
	"96": "UPS Worldwide Express Freight",
}
