// Package eforms will parse Norwegian eForms notices (UBL XML) into the
// canonical procedure -> notice -> lot model.
//
// It is SDK-version-aware: the eForms SDK version and the national flavour
// (…:eforms:eu, …:eforms:national, …:2026:eforms:national-e) are read from
// cbc:CustomizationID and select the field map and code lists; an unknown
// version fails closed instead of guessing. It performs no I/O and holds no
// tenant data.
//
// Not implemented yet: it is built against real notices captured with
// cmd/doffin-probe. See docs/ARCHITECTURE.md, "Build order".
package eforms
