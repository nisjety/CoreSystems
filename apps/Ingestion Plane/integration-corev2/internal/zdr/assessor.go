// Package zdr assesses whether a user-owned model connection can give zero data
// retention (ZDR), and computes the opaque digest that ties a user's acknowledgement to the
// exact assessment they were shown.
//
// integration-corev2 owns the per-connection record. This package is the only place the
// digest is computed: Auth Core compares it as an opaque string, so no second serialiser
// can drift from this one. The rule table is the shared capability contract
// (apps/Model Plane/api/contracts/connection-zdr-capabilities.json), embedded from a
// byte-identical, sha256-pinned vendored copy; Load re-hashes the bytes against the pin and
// refuses to start on any difference or any rule the strict loader rejects.
//
// Honest by construction: an assessment is one of three states and cannot_verify is a
// first-class state, never rounded up to supported. Only the exact (provider_key, auth_mode)
// pair of a contract row is assessed as that row; anything else gets the fallback
// (cannot_verify, no waiver route).
package zdr

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"sync"
	"unicode/utf8"

	_ "embed"
)

//go:embed testdata/connection-zdr-capabilities.json
var embeddedContract []byte

//go:embed testdata/connection-zdr-capabilities.json.sha256
var embeddedPin []byte

// Capability is whether a connection can give ZDR.
type Capability string

const (
	CapabilitySupported    Capability = "supported"
	CapabilityNotSupported Capability = "not_supported"
	CapabilityCannotVerify Capability = "cannot_verify"
)

// Valid reports whether c is one of the three states.
func (c Capability) Valid() bool {
	return c == CapabilitySupported || c == CapabilityNotSupported || c == CapabilityCannotVerify
}

// KeepsZDR is the run rule: only a supported connection keeps ZDR on for the run that uses it.
// cannot_verify is treated exactly like not_supported.
func (c Capability) KeepsZDR() bool { return c == CapabilitySupported }

// RequiresAcknowledgement is the other half of the run rule.
func (c Capability) RequiresAcknowledgement() bool { return c.Valid() && !c.KeepsZDR() }

// Basis is what an assessment rests on.
type Basis string

const (
	BasisVerevonAttestedDeployment Basis = "verevon_attested_deployment"
	BasisProviderTermsKnown        Basis = "provider_terms_known"
	BasisProviderByContract        Basis = "provider_by_contract"
	BasisCustomerAsserted          Basis = "customer_asserted"
	BasisNotAssessed               Basis = "not_assessed"
)

// Valid reports whether b is one of the five bases.
func (b Basis) Valid() bool { return contains(basesList, string(b)) }

// Assessment is the outcome for one connection. It carries everything an acknowledgement is
// bound to; Digest changes whenever any bound field does.
type Assessment struct {
	ProviderKey    string
	AuthMode       string
	Capability     Capability
	Basis          Basis
	BasisRef       string
	WaiverRoute    bool
	WordingKey     string
	CatalogVersion int
	Digest         string
}

// Assessor applies the contract. It is immutable after Load and safe for concurrent use.
type Assessor struct {
	doc      *contractDoc
	rows     rowIndex
	pinned   string
	provider []ProviderRow
}

// ProviderRow is a read-only view of one assessed (provider_key, auth_mode) row.
type ProviderRow struct {
	BasisRef    string
	ProviderKey string
	AuthMode    string
	Capability  Capability
	Basis       Basis
	WaiverRoute bool
	WordingKey  string
	// InferenceProviderID and InferenceSupportsZDR name the inference-core provider the row is
	// cross-checked against; empty and false when the row has none.
	InferenceProviderID  string
	InferenceSupportsZDR bool
}

var (
	defaultOnce     sync.Once
	defaultAssessor *Assessor
	defaultErr      error
)

// Default is the assessor over the embedded contract. It is loaded and verified once; a
// service calls it at boot and refuses to start when it errors.
func Default() (*Assessor, error) {
	defaultOnce.Do(func() {
		defaultAssessor, defaultErr = Load(embeddedContract, string(embeddedPin))
	})
	return defaultAssessor, defaultErr
}

// Load verifies raw against pin (a "<sha256>  <name>" line as written by sha256sum, hashed
// over the file with CRLF normalised to LF), then loads it strictly.
func Load(raw []byte, pin string) (*Assessor, error) {
	pinned := strings.ToLower(strings.TrimSpace(firstField(pin)))
	if !sha256Pattern.MatchString(pinned) {
		return nil, errors.New("connection ZDR capability contract pin is not a sha256 digest")
	}
	actual := sha256Hex(strings.ReplaceAll(string(raw), "\r\n", "\n"))
	if actual != pinned {
		return nil, fmt.Errorf("connection ZDR capability contract does not match its pinned digest (pinned %s, actual %s): re-vendor it from apps/Model Plane/api/contracts", pinned, actual)
	}
	doc, err := parseContract(raw)
	if err != nil {
		return nil, err
	}
	assessor := &Assessor{doc: doc, rows: newRowIndex(doc), pinned: pinned}
	for _, p := range doc.Providers {
		view := ProviderRow{
			BasisRef: p.BasisRef, ProviderKey: p.ProviderKey, AuthMode: p.AuthMode,
			Capability: Capability(p.Capability), Basis: Basis(p.Basis), WaiverRoute: p.WaiverRoute, WordingKey: p.WordingKey,
		}
		if p.InferenceProviderID != nil {
			view.InferenceProviderID = *p.InferenceProviderID
		}
		if p.InferenceSupportsZDR != nil {
			view.InferenceSupportsZDR = *p.InferenceSupportsZDR
		}
		assessor.provider = append(assessor.provider, view)
	}
	return assessor, nil
}

// CatalogVersion is the contract's catalog_version; it is part of every digest.
func (a *Assessor) CatalogVersion() int { return a.doc.CatalogVersion }

// PinnedSHA256 is the digest the loaded contract was verified against.
func (a *Assessor) PinnedSHA256() string { return a.pinned }

// Providers lists the assessed rows (a copy).
func (a *Assessor) Providers() []ProviderRow { return append([]ProviderRow(nil), a.provider...) }

// WordingKeys lists the acknowledgement wording keys and the states they belong to.
func (a *Assessor) WordingKeys() map[string]Capability {
	out := make(map[string]Capability, len(a.doc.WordingKeys))
	for _, w := range a.doc.WordingKeys {
		out[w.Key] = Capability(w.State)
	}
	return out
}

// Assess returns the assessment of a connection of the given provider and auth mode. Both
// come from server-side catalog data, never from anything a client can write. It returns an
// error only for a value that cannot be hashed (invalid UTF-8), never a guess.
func (a *Assessor) Assess(providerKey, authMode string) (Assessment, error) {
	found := a.rows.lookup(providerKey, authMode)
	digest, err := Digest(providerKey, authMode, found.capability, found.basis, a.doc.CatalogVersion)
	if err != nil {
		return Assessment{}, err
	}
	return Assessment{
		ProviderKey:    providerKey,
		AuthMode:       authMode,
		Capability:     found.capability,
		Basis:          found.basis,
		BasisRef:       found.basisRef,
		WaiverRoute:    found.waiverRoute,
		WordingKey:     found.wordingKey,
		CatalogVersion: a.doc.CatalogVersion,
		Digest:         digest,
	}, nil
}

// Digest is sha256 (lowercase hex) of the canonical JSON of the fields an acknowledgement is
// bound to: the object holding exactly auth_mode, basis, capability, catalog_version and
// provider_key, keys ascending, no whitespace outside strings, UTF-8, catalog_version an
// integer. Any change to any of them is a different digest, which invalidates every
// acknowledgement given against the old one.
func Digest(providerKey, authMode string, capability Capability, basis Basis, catalogVersion int) (string, error) {
	canonical, err := canonicalDigestInput(providerKey, authMode, capability, basis, catalogVersion)
	if err != nil {
		return "", err
	}
	return sha256Hex(canonical), nil
}

// IsDigest reports whether s has the shape of a digest (64 lowercase hex characters).
func IsDigest(s string) bool { return sha256Pattern.MatchString(s) }

func canonicalDigestInput(providerKey, authMode string, capability Capability, basis Basis, catalogVersion int) (string, error) {
	providerKeyJSON, err := jsonString(providerKey)
	if err != nil {
		return "", fmt.Errorf("provider_key: %w", err)
	}
	authModeJSON, err := jsonString(authMode)
	if err != nil {
		return "", fmt.Errorf("auth_mode: %w", err)
	}
	capabilityJSON, err := jsonString(string(capability))
	if err != nil {
		return "", fmt.Errorf("capability: %w", err)
	}
	basisJSON, err := jsonString(string(basis))
	if err != nil {
		return "", fmt.Errorf("basis: %w", err)
	}
	var out strings.Builder
	out.WriteString(`{"auth_mode":`)
	out.WriteString(authModeJSON)
	out.WriteString(`,"basis":`)
	out.WriteString(basisJSON)
	out.WriteString(`,"capability":`)
	out.WriteString(capabilityJSON)
	out.WriteString(`,"catalog_version":`)
	out.WriteString(strconv.Itoa(catalogVersion))
	out.WriteString(`,"provider_key":`)
	out.WriteString(providerKeyJSON)
	out.WriteString(`}`)
	return out.String(), nil
}

// jsonString encodes s as ECMAScript JSON.stringify does, which is what the contract's
// canonical form is defined against: quote, backslash and the C0 controls are escaped
// (\b \f \n \r \t, otherwise \u00xx in lowercase hex); every other character, including
// < > & U+2028 and U+2029, is written as itself. Go's encoding/json would escape the
// last four, so it is not used here. A string that is not valid UTF-8 has no JSON form.
func jsonString(s string) (string, error) {
	if !utf8.ValidString(s) {
		return "", errors.New("is not valid UTF-8")
	}
	var out strings.Builder
	out.Grow(len(s) + 2)
	out.WriteByte('"')
	for _, r := range s {
		switch r {
		case '"':
			out.WriteString(`\"`)
		case '\\':
			out.WriteString(`\\`)
		case '\b':
			out.WriteString(`\b`)
		case '\f':
			out.WriteString(`\f`)
		case '\n':
			out.WriteString(`\n`)
		case '\r':
			out.WriteString(`\r`)
		case '\t':
			out.WriteString(`\t`)
		default:
			if r < 0x20 {
				out.WriteString(`\u00`)
				out.WriteByte(hexDigits[r>>4])
				out.WriteByte(hexDigits[r&0xf])
			} else {
				out.WriteRune(r)
			}
		}
	}
	out.WriteByte('"')
	return out.String(), nil
}

const hexDigits = "0123456789abcdef"

func sha256Hex(text string) string {
	sum := sha256.Sum256([]byte(text))
	return hex.EncodeToString(sum[:])
}

func firstField(s string) string {
	fields := strings.Fields(s)
	if len(fields) == 0 {
		return ""
	}
	return fields[0]
}
