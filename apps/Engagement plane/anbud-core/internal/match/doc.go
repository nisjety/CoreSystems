// Package match decides whether a company can and should bid on a notice: a gate first, then a
// fit. The gate is hard: a passed deadline, a company the register blocks, an excluded CPV area or
// a qualification requirement that cannot be met in time is a fail, never a low score. The fit is
// soft: CPV, region, value and keywords, shown as a band (strong, good, stretch) with every check
// cited to the field it came from. No percentage is computed: there are no outcomes yet to
// calibrate one against.
//
// The rules are deterministic and the defaults are placeholders the owner is expected to replace:
// the requirement keywords, the lead-time table and the strictness levels. Nothing here calls a
// model or stores a profile; a profile is tenant data and its storage is not built.
//
// See docs/ARCHITECTURE.md, "Product layer (proposed)".
package match
