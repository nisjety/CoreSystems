// Package normalize cleans parsed notices: CPV and NUTS codes, Norwegian organisation numbers,
// and deadlines as Oslo calendar days. Resolving an organisation number against Brønnøysund and
// text clean-up are not done here yet.
//
// Pure functions only, and the input is never changed. Every correction records what changed and
// from what, so the validation stages and the UI can show provenance instead of silently
// rewritten data. A value that cannot be normalised is dropped and a problem says so, in fixed
// text without the value.
//
// See docs/ARCHITECTURE.md, "Build order", step 4.
package normalize
