// Package brreg provides a thin client for the open Enhetsregisteret API
// published by Brønnøysundregistrene.
//
// Documentation: https://data.brreg.no/enhetsregisteret/api/dokumentasjon/no/index.html
//
// No API key is required – all endpoints used here are publicly accessible.
package brreg

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

const baseURL = "https://data.brreg.no/enhetsregisteret/api"

// Adresse mirrors the address sub-object returned by Enhetsregisteret.
type Adresse struct {
	Adresse     []string `json:"adresse,omitempty"`
	Postnummer  string   `json:"postnummer,omitempty"`
	Poststed    string   `json:"poststed,omitempty"`
	Kommune     string   `json:"kommune,omitempty"`
	Kommunenr   string   `json:"kommunenummer,omitempty"`
	Land        string   `json:"land,omitempty"`
	Landkode    string   `json:"landkode,omitempty"`
}

// OrgForm mirrors the organisasjonsform sub-object.
type OrgForm struct {
	Kode        string `json:"kode"`
	Beskrivelse string `json:"beskrivelse"`
}

// NaeringskodeRef mirrors one næringskode reference.
type NaeringskodeRef struct {
	Kode        string `json:"kode"`
	Beskrivelse string `json:"beskrivelse"`
}

// Enhet is the organisation record returned by Enhetsregisteret.
// Only the fields we use are mapped; the rest round-trip transparently
// through the RawData field stored in org metadata.
type Enhet struct {
	Organisasjonsnummer             string          `json:"organisasjonsnummer"`
	Navn                            string          `json:"navn"`
	Organisasjonsform               OrgForm         `json:"organisasjonsform"`
	Forretningsadresse              *Adresse        `json:"forretningsadresse,omitempty"`
	Postadresse                     *Adresse        `json:"postadresse,omitempty"`
	RegistreringsdatoEnhetsreg      string          `json:"registreringsdatoEnhetsregisteret,omitempty"`
	Hjemmeside                      string          `json:"hjemmeside,omitempty"`
	Naeringskode1                   *NaeringskodeRef `json:"naeringskode1,omitempty"`
	AntallAnsatte                   *int             `json:"antallAnsatte,omitempty"`
	Konkurs                         bool            `json:"konkurs"`
	UnderAvvikling                  bool            `json:"underAvvikling"`
	UnderTvangsavviklingEllerTvang  bool            `json:"underTvangsavviklingEllerTvangsopplosning"`
	Stiftelsesdato                  string          `json:"stiftelsesdato,omitempty"`
	Epostadresse                    string          `json:"epostadresse,omitempty"`
	Telefon                         string          `json:"telefon,omitempty"`
}

// SearchResult is the paginated response from GET /enheter.
type SearchResult struct {
	Embedded struct {
		Enheter []Enhet `json:"enheter"`
	} `json:"_embedded"`
	Page struct {
		Number        int `json:"number"`
		Size          int `json:"size"`
		TotalPages    int `json:"totalPages"`
		TotalElements int `json:"totalElements"`
	} `json:"page"`
}

// Client is a lightweight HTTP client for the open Enhetsregisteret API.
type Client struct {
	httpClient *http.Client
}

// NewClient returns a Client with a 10-second timeout.
func NewClient() *Client {
	return &Client{
		httpClient: &http.Client{
			Timeout: 10 * time.Second,
		},
	}
}

// SearchByName calls GET /enheter?navn=<name>&size=<size> and returns
// the matching entities (up to size, max 20 by default).
func (c *Client) SearchByName(ctx context.Context, name string, size int) ([]Enhet, error) {
	if size <= 0 {
		size = 10
	}
	if size > 20 {
		size = 20
	}

	endpoint := fmt.Sprintf("%s/enheter?navn=%s&size=%d",
		baseURL, url.QueryEscape(name), size)

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("brreg: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("brreg: search request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("brreg: search returned %d", resp.StatusCode)
	}

	var result SearchResult
	if err := json.NewDecoder(resp.Body).Decode(&result); err != nil {
		return nil, fmt.Errorf("brreg: decode search response: %w", err)
	}

	return result.Embedded.Enheter, nil
}

// LookupByOrgNr calls GET /enheter/{orgnr} and returns the matching entity.
// Returns (nil, nil) when the entity was deleted (410 Gone or 404).
func (c *Client) LookupByOrgNr(ctx context.Context, orgnr string) (*Enhet, error) {
	endpoint := fmt.Sprintf("%s/enheter/%s", baseURL, url.PathEscape(orgnr))

	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return nil, fmt.Errorf("brreg: build request: %w", err)
	}
	req.Header.Set("Accept", "application/json")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return nil, fmt.Errorf("brreg: lookup request: %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode == http.StatusNotFound || resp.StatusCode == http.StatusGone {
		return nil, nil
	}
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("brreg: lookup returned %d", resp.StatusCode)
	}

	var enhet Enhet
	if err := json.NewDecoder(resp.Body).Decode(&enhet); err != nil {
		return nil, fmt.Errorf("brreg: decode lookup response: %w", err)
	}

	return &enhet, nil
}
