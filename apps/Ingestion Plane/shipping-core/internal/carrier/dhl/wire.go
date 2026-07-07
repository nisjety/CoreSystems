package dhl

import "encoding/json"

// Wire shapes for MyDHL API's Rating service (POST /rates). VERIFICATION
// STATUS: developer.dhl.com's own reference pages render their endpoint
// schemas via a JS-driven Swagger console that isn't scrapable as static
// docs. These field names are instead cross-verified against THREE
// independent, real, currently-maintained implementations that all talk to
// production MyDHL API: (1) Magento 2's official Dhl module (config.xml
// gateway URLs), (2) karrio (github.com/karrioapi/karrio — an actively
// maintained open-source multi-carrier shipping API with real, tested
// request/response field mappings for MyDHL), and (3) independent
// confirmation of the base URLs from sonnenglas/mydhl-php-sdk,
// booni3/dhl-express-rest, ripe-tech/dhl-api-js, and Odoo's DHL delivery
// connector. This is a stronger verification basis than a single source,
// but — like the Bring adapter — it has NOT been exercised against a live
// DHL sandbox call yet; that is the first thing to confirm once real
// credentials are in .env.

type rateRequest struct {
	CustomerDetails            customerDetails `json:"customerDetails"`
	Accounts                   []account       `json:"accounts,omitempty"`
	ProductCode                string          `json:"productCode,omitempty"`
	PlannedShippingDateAndTime string          `json:"plannedShippingDateAndTime"`
	UnitOfMeasurement          string          `json:"unitOfMeasurement"`
	IsCustomsDeclarable        bool            `json:"isCustomsDeclarable"`
	Packages                   []ratePackage   `json:"packages"`
}

type customerDetails struct {
	ShipperDetails  addressDetails `json:"shipperDetails"`
	ReceiverDetails addressDetails `json:"receiverDetails"`
}

// addressDetails is MyDHL's "ErDetailsType" shape (shared by shipper and
// receiver in a rate request).
type addressDetails struct {
	PostalCode   string `json:"postalCode"`
	CityName     string `json:"cityName"`
	CountryCode  string `json:"countryCode"`
	ProvinceCode string `json:"provinceCode,omitempty"`
	AddressLine1 string `json:"addressLine1,omitempty"`
}

type account struct {
	TypeCode string `json:"typeCode"` // "shipper" for the paying account on a rate request
	Number   string `json:"number"`
}

type ratePackage struct {
	Weight     float64     `json:"weight"`
	Dimensions *dimensions `json:"dimensions,omitempty"`
}

type dimensions struct {
	Length int `json:"length"`
	Width  int `json:"width"`
	Height int `json:"height"`
}

// rateResponse — a request with no productCode returns every product DHL
// can offer for the route (the "RATING" service description: "return DHL
// EXPRESS product capabilities"), which is exactly the shape a comparison
// aggregator wants.
type rateResponse struct {
	Products []product `json:"products"`
	// Error responses carry a top-level "status"/"detail" shape instead of
	// "products" (matches karrio's error.py dispatch: presence of "status"
	// with no "products" means the response is an error, not a rate list).
	Status string `json:"status,omitempty"`
	Detail string `json:"detail,omitempty"`
}

type product struct {
	ProductCode          string                `json:"productCode"`
	ProductName          string                `json:"productName"`
	TotalPrice           []priceEntry          `json:"totalPrice,omitempty"`
	DeliveryCapabilities *deliveryCapabilities `json:"deliveryCapabilities,omitempty"`
}

type priceEntry struct {
	// Price is observed as either a numeric-looking string or a bare JSON
	// number depending on account configuration; json.Number accepts both
	// representations without a custom unmarshaler.
	Price         json.Number `json:"price"`
	PriceCurrency string      `json:"priceCurrency"`
}

type deliveryCapabilities struct {
	TotalTransitDays             int    `json:"totalTransitDays"`
	EstimatedDeliveryDateAndTime string `json:"estimatedDeliveryDateAndTime"`
}
