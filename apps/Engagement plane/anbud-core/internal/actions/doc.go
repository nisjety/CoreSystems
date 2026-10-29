// Package actions will expose the typed operations the Model Plane may invoke
// on tenders (find, assess, shortlist, set a reminder, prepare a draft).
//
// Every operation must map to a capability id that capability-core actually
// seeds, in both registry.go and the capabilities migrations; an unseeded id
// fails closed at runtime and looks identical to no binding. Anything with an
// outward-facing effect needs human approval.
//
// Not implemented yet. See docs/ARCHITECTURE.md, "Build order".
package actions
