package address

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"coresystem/apps/application-plane/information-core/internal/cache"
	"coresystem/apps/application-plane/information-core/internal/config"
	"coresystem/apps/application-plane/information-core/internal/provenance"
)

const defaultSearchURL = "https://ws.geonorge.no/adresser/v1/sok"

var ErrInvalidQuery = errors.New("address: invalid query")

type Service struct {
	client    *http.Client
	cache     *cache.Store
	searchURL string
}

type Response struct {
	Source provenance.Source `json:"source"`
	Data   []Address         `json:"data"`
	Meta   Metadata          `json:"meta"`
}

type Address struct {
	AddressText         string        `json:"addressText,omitempty"`
	StreetName          string        `json:"streetName,omitempty"`
	HouseNumber         int           `json:"houseNumber,omitempty"`
	HouseLetter         string        `json:"houseLetter,omitempty"`
	MunicipalityCode    string        `json:"municipalityCode,omitempty"`
	MunicipalityName    string        `json:"municipalityName,omitempty"`
	PostalCode          string        `json:"postalCode,omitempty"`
	PostalPlace         string        `json:"postalPlace,omitempty"`
	ObjectType          string        `json:"objectType,omitempty"`
	CadastreUnit        *CadastreUnit `json:"cadastreUnit,omitempty"`
	RepresentationPoint *Point        `json:"representationPoint,omitempty"`
	VerifiedLocation    bool          `json:"verifiedLocation"`
	UpdatedAt           string        `json:"updatedAt,omitempty"`
}

// CadastreUnit is only the open address response's cadastral reference. It is
// not ownership, title, rights, valuation, or Grunnbok data.
type CadastreUnit struct {
	FarmNumber  int `json:"farmNumber,omitempty"`
	UsageNumber int `json:"usageNumber,omitempty"`
	LeaseNumber int `json:"leaseNumber,omitempty"`
	SubNumber   int `json:"subNumber,omitempty"`
}

type Point struct {
	EPSG string  `json:"epsg"`
	Lat  float64 `json:"lat"`
	Lon  float64 `json:"lon"`
}

type Metadata struct {
	Search      string `json:"search,omitempty"`
	TotalHits   int    `json:"totalHits"`
	PageSize    int    `json:"pageSize"`
	Page        int    `json:"page"`
	FirstResult int    `json:"firstResult,omitempty"`
	LastResult  int    `json:"lastResult,omitempty"`
}

type kartverketResponse struct {
	Metadata struct {
		Search      string `json:"sokeStreng"`
		TotalHits   int    `json:"totaltAntallTreff"`
		PageSize    int    `json:"treffPerSide"`
		Page        int    `json:"side"`
		FirstResult int    `json:"viserFra"`
		LastResult  int    `json:"viserTil"`
	} `json:"metadata"`
	Addresses []kartverketAddress `json:"adresser"`
}

type kartverketAddress struct {
	AddressName      string `json:"adressenavn"`
	AddressText      string `json:"adressetekst"`
	HouseNumber      int    `json:"nummer"`
	HouseLetter      string `json:"bokstav"`
	MunicipalityCode string `json:"kommunenummer"`
	MunicipalityName string `json:"kommunenavn"`
	FarmNumber       int    `json:"gardsnummer"`
	UsageNumber      int    `json:"bruksnummer"`
	LeaseNumber      int    `json:"festenummer"`
	SubNumber        int    `json:"undernummer"`
	PostalPlace      string `json:"poststed"`
	PostalCode       string `json:"postnummer"`
	ObjectType       string `json:"objtype"`
	VerifiedLocation bool   `json:"stedfestingverifisert"`
	UpdatedAt        string `json:"oppdateringsdato"`
	Point            *struct {
		EPSG string  `json:"epsg"`
		Lat  float64 `json:"lat"`
		Lon  float64 `json:"lon"`
	} `json:"representasjonspunkt"`
}

func NewService(client *http.Client, cacheStore *cache.Store) *Service {
	return &Service{client: client, cache: cacheStore, searchURL: defaultSearchURL}
}

func NewServiceWithURL(client *http.Client, cacheStore *cache.Store, searchURL string) *Service {
	service := NewService(client, cacheStore)
	service.searchURL = searchURL
	return service
}

func (s *Service) Lookup(ctx context.Context, query string, limit, page int, fuzzy bool) (Response, error) {
	query = strings.TrimSpace(query)
	if query == "" || len([]rune(query)) > 200 {
		return Response{}, fmt.Errorf("%w: query must be between 1 and 200 characters", ErrInvalidQuery)
	}
	if limit < 1 || limit > 50 {
		return Response{}, fmt.Errorf("%w: limit must be between 1 and 50", ErrInvalidQuery)
	}
	if page < 0 {
		return Response{}, fmt.Errorf("%w: page must be non-negative", ErrInvalidQuery)
	}

	key := fmt.Sprintf("address:%s:%d:%d:%t", strings.ToLower(query), limit, page, fuzzy)
	if cached, ok := s.cache.Get(key); ok {
		if payload, ok := cached.(Response); ok {
			return payload, nil
		}
	}

	params := url.Values{}
	params.Set("sok", query)
	params.Set("treffPerSide", strconv.Itoa(limit))
	params.Set("side", strconv.Itoa(page))
	params.Set("utkoordsys", "4258")
	if fuzzy {
		params.Set("fuzzy", "true")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, s.searchURL+"?"+params.Encode(), nil)
	if err != nil {
		return Response{}, fmt.Errorf("address: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "VerevonInformationCore/1.0")

	resp, err := s.client.Do(req)
	if err != nil {
		return Response{}, fmt.Errorf("address: upstream: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode < http.StatusOK || resp.StatusCode >= http.StatusMultipleChoices {
		return Response{}, fmt.Errorf("address: upstream returned HTTP %d", resp.StatusCode)
	}

	var raw kartverketResponse
	if err := json.NewDecoder(resp.Body).Decode(&raw); err != nil {
		return Response{}, fmt.Errorf("address: decode upstream response: %w", err)
	}

	retrievedAt := time.Now().UTC().Format(time.RFC3339)
	addresses := make([]Address, 0, len(raw.Addresses))
	for _, item := range raw.Addresses {
		var point *Point
		if item.Point != nil {
			point = &Point{EPSG: item.Point.EPSG, Lat: item.Point.Lat, Lon: item.Point.Lon}
		}
		addresses = append(addresses, Address{
			AddressText: item.AddressText, StreetName: item.AddressName,
			HouseNumber: item.HouseNumber, HouseLetter: item.HouseLetter,
			MunicipalityCode: item.MunicipalityCode, MunicipalityName: item.MunicipalityName,
			PostalCode: item.PostalCode, PostalPlace: item.PostalPlace, ObjectType: item.ObjectType,
			CadastreUnit: cadastreUnit(item), RepresentationPoint: point,
			VerifiedLocation: item.VerifiedLocation, UpdatedAt: item.UpdatedAt,
		})
	}

	result := Response{
		Source: provenance.Source{
			Provider: "kartverket", Dataset: "address-rest-v1.2.0", SourceURL: defaultSearchURL,
			License: "NLOD-2.0", RetrievedAt: retrievedAt, Quality: "authoritative_provider",
			Coverage: "national_address_register", Status: "measured", APIVersion: "1.2.0", CRS: "EPSG:4258",
		},
		Data: addresses,
		Meta: Metadata{Search: raw.Metadata.Search, TotalHits: raw.Metadata.TotalHits,
			PageSize: raw.Metadata.PageSize, Page: raw.Metadata.Page,
			FirstResult: raw.Metadata.FirstResult, LastResult: raw.Metadata.LastResult},
	}
	s.cache.Set(key, config.TTL(300), result)
	return result, nil
}

func cadastreUnit(item kartverketAddress) *CadastreUnit {
	if item.FarmNumber == 0 && item.UsageNumber == 0 && item.LeaseNumber == 0 && item.SubNumber == 0 {
		return nil
	}
	return &CadastreUnit{FarmNumber: item.FarmNumber, UsageNumber: item.UsageNumber, LeaseNumber: item.LeaseNumber, SubNumber: item.SubNumber}
}
