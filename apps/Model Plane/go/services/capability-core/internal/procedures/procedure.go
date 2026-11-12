// Package procedures is capability-core's durable store of saved browser recipes: BrowserProcedure
// v2 bodies that Quarry compiled from a verified run, kept as immutable numbered versions with
// movable tags, visible to their owner only (plan 3.4 E4.2 step 3; decision B2(b)).
//
// A stored body is an executable step list, so the guards live here and not at the caller:
//
//   - the vocabulary is closed (see allowedActionTypes), so a body cannot name an action Quarry
//     compiles away on purpose (evaluate, upload, download, dialog replies, frames, raw coordinates);
//   - no value a person typed is ever stored: a typed step carries empty text and names a slot that
//     the replay fills, and any approval_grant_id or "[redacted" marker anywhere in the body is a
//     refusal (a grant is single use and bound to one run; a marker means a secret was masked and the
//     step would replay the mask);
//   - the body is bounded (256 KiB, 100 steps) and is stored and digested in one canonical form, so
//     the bytes that were validated are the bytes that are kept.
//
// The store holds actions and locators only. Zero Data Retention: a recipe is org-owned
// configuration, not conversation content; the refusal for ZDR callers sits at the gateway.
package procedures

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"regexp"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"
)

const (
	// BodyVersion is the only BrowserProcedure wire version this store accepts.
	BodyVersion = 2
	// MaxBodyBytes bounds one stored body.
	MaxBodyBytes = 256 << 10
	// MaxSteps bounds the steps of one body.
	MaxSteps = 100
	// MaxSlots bounds the slots of one body.
	MaxSlots = 100
	// MaxNameRunes, MaxDescriptionRunes and MaxSourceRunIDBytes bound the metadata of a procedure
	// (the migration repeats the same limits as CHECK constraints).
	MaxNameRunes        = 120
	MaxDescriptionRunes = 500
	MaxSourceRunIDBytes = 128
	// MaxTagLen is the longest tag name.
	MaxTagLen = 32

	// maxDepth bounds JSON nesting. Real bodies nest about five levels (step, action, locator).
	maxDepth = 32
	// maxSlotNameLen bounds a slot name.
	maxSlotNameLen = 64
)

// Code is the closed reason a body, a tag or a piece of metadata was refused. It is a stable token
// a caller can put in an error envelope; the message never repeats any content of the body.
type Code string

const (
	// CodeBodyTooLarge means the body is over MaxBodyBytes.
	CodeBodyTooLarge Code = "procedure_body_too_large"
	// CodeInvalidJSON means the body is not one JSON object.
	CodeInvalidJSON Code = "procedure_invalid_json"
	// CodeBodyTooDeep means the body nests deeper than maxDepth.
	CodeBodyTooDeep Code = "procedure_body_too_deep"
	// CodeBadShape means a field has the wrong JSON type, is unknown, or is missing.
	CodeBadShape Code = "procedure_bad_shape"
	// CodeUnsupportedVersion means the body is not BrowserProcedure version 2.
	CodeUnsupportedVersion Code = "procedure_unsupported_version"
	// CodeBadStepCount means the body has no steps or more than MaxSteps.
	CodeBadStepCount Code = "procedure_bad_step_count"
	// CodeActionNotAllowed means a step action is outside the closed vocabulary.
	CodeActionNotAllowed Code = "procedure_action_not_allowed"
	// CodeForbiddenKey means an approval_grant_id key appears in the body.
	CodeForbiddenKey Code = "procedure_forbidden_key"
	// CodeTypedText means a typed step carries text; typed values are never stored.
	CodeTypedText Code = "procedure_typed_text"
	// CodeRedactedMarker means a string carries a redaction marker.
	CodeRedactedMarker Code = "procedure_redacted_marker"
	// CodeNulCharacter means a string or a key carries a NUL character, which jsonb cannot store.
	CodeNulCharacter Code = "procedure_nul_character"
	// CodeBadSlot means a slot is malformed, duplicated, unused, or a typed step has none.
	CodeBadSlot Code = "procedure_bad_slot"
	// CodeBadName means the procedure name is empty, too long, or has a control character.
	CodeBadName Code = "procedure_bad_name"
	// CodeBadDescription means the description is too long or has a control character.
	CodeBadDescription Code = "procedure_bad_description"
	// CodeBadSourceRun means the source run id is too long or has a control character.
	CodeBadSourceRun Code = "procedure_bad_source_run"
	// CodeBadTag means a tag does not match ^[a-z0-9._-]{1,32}$.
	CodeBadTag Code = "procedure_bad_tag"
)

// ValidationError is a refusal of a body, a tag or a piece of metadata. Message names the field and
// the step index but never a value from the body.
type ValidationError struct {
	Code    Code
	Message string
}

func (e *ValidationError) Error() string { return string(e.Code) + ": " + e.Message }

// AsValidation reports whether err is a ValidationError and returns it.
func AsValidation(err error) (*ValidationError, bool) {
	var v *ValidationError
	if errors.As(err, &v) {
		return v, true
	}
	return nil, false
}

func refuse(code Code, format string, args ...any) *ValidationError {
	return &ValidationError{Code: code, Message: fmt.Sprintf(format, args...)}
}

// allowedActionTypes is the closed vocabulary of a stored step. It is the output vocabulary of
// Quarry's BrowserProcedure v2 compile. Everything else (evaluate, upload*, download*, fill_otp_ref,
// respond_dialog, frame_*, click_point, mouse_wheel, forward, reload, screenshot, pdf and the raw
// ref forms) is refused, so no approval_grant_id is ever stored.
var allowedActionTypes = map[string]bool{
	"navigate":          true,
	"click":             true,
	"click_semantic":    true,
	"type":              true,
	"type_semantic":     true,
	"select_semantic":   true,
	"wait_for_semantic": true,
	"press":             true,
	"scroll":            true,
	"wait":              true,
	"back":              true,
	"get_content":       true,
}

// typedActionTypes are the actions that type a value, so they must name a slot and carry no text.
var typedActionTypes = map[string]bool{"type": true, "type_semantic": true}

// actionKeys is the closed field set of each action, by its type: the serde shape of the Quarry AgentAction
// variant the v2 compile keeps (its type tag and its own fields). A field outside the set is refused, so text
// is accepted only on a typed action, where it must be empty, and a type cannot carry a field it does not have.
var actionKeys = map[string]map[string]bool{
	"navigate":          {"type": true, "url": true},
	"click":             {"type": true, "selector": true},
	"click_semantic":    {"type": true, "snapshot_id": true, "generation": true, "locator": true},
	"type":              {"type": true, "selector": true, "text": true},
	"type_semantic":     {"type": true, "snapshot_id": true, "generation": true, "locator": true, "text": true},
	"select_semantic":   {"type": true, "snapshot_id": true, "generation": true, "locator": true, "value": true},
	"wait_for_semantic": {"type": true, "snapshot_id": true, "generation": true, "locator": true, "timeout_ms": true},
	"press":             {"type": true, "key": true},
	"scroll":            {"type": true, "target": true},
	"wait":              {"type": true, "ms": true},
	"back":              {"type": true},
	"get_content":       {"type": true},
}

// stepObjectKeys is the closed field set of a step's target (StepTarget) and expect (StepExpectation), the
// shapes Quarry writes.
var stepObjectKeys = map[string]map[string]bool{
	"target": {"locator": true, "fingerprint_id": true, "tag": true},
	"expect": {"all": true},
}

// locatorKeys is the closed field set of each SemanticLocator kind, by its serde tag (the enum in Quarry's
// quarry-core contracts.rs). The enum has no deny_unknown_fields, so a field outside its kind's set would be
// kept in the stored body and replayed without error. The locator of an action and of a target are both closed.
var locatorKeys = map[string]map[string]bool{
	"role":        {"kind": true, "role": true, "name": true, "exact": true},
	"text":        {"kind": true, "text": true, "exact": true},
	"label":       {"kind": true, "label": true, "exact": true},
	"placeholder": {"kind": true, "placeholder": true, "exact": true},
	"test_id":     {"kind": true, "test_id": true, "exact": true},
	"nth":         {"kind": true, "selector": true, "index": true},
}

// expectConditionKeys is the closed field set of each ExpectCondition kind, by its serde tag (the enum in Quarry's
// quarry-core step_expect.rs). Quarry denies unknown fields there, so a replay fails loudly, but the store must not
// keep an unvetted value until then. A condition's locator and within are closed as locators (checkLocator).
var expectConditionKeys = map[string]map[string]bool{
	"url_matches":     {"kind": true, "pattern": true},
	"element_present": {"kind": true, "locator": true},
	"element_absent":  {"kind": true, "locator": true},
	"text_present":    {"kind": true, "text": true, "within": true},
	"text_absent":     {"kind": true, "text": true, "within": true},
}

// fieldType is the JSON type a closed field holds: the scalar its Quarry serde field expects. A value of any other type
// is refused, so a typed value cannot sit under a field that holds a string, a flag or a number. The zero value holds
// no type, so a field with no entry in a type table is refused.
type fieldType uint8

const (
	typeNone fieldType = iota
	typeString
	typeBool
	// typeU32 is a non-negative integer no larger than math.MaxUint32, the serde type of Quarry's u32 fields.
	typeU32
	// typeObject is a JSON object. Its own closed set is checked by the rule of its kind (a locator, a target, an expect).
	typeObject
	typeArray
	// typeObjectOrNull is an object, or null where Quarry writes an Option without skip_serializing_if: a text condition
	// writes within as null when it names no element.
	typeObjectOrNull
	// typeOutcome is one of the ActionOutcomeStatus tokens: verified, failed or unknown.
	typeOutcome
)

// holds reports whether v, a value of a scanned body, has the type. An integer is an int64 after scan, and a number
// with a fraction or an exponent is a float64, which no type here holds.
func (t fieldType) holds(v any) bool {
	switch t {
	case typeString:
		_, ok := v.(string)
		return ok
	case typeBool:
		_, ok := v.(bool)
		return ok
	case typeU32:
		n, ok := v.(int64)
		return ok && n >= 0 && n <= math.MaxUint32
	case typeObject:
		_, ok := v.(map[string]any)
		return ok
	case typeArray:
		_, ok := v.([]any)
		return ok
	case typeObjectOrNull:
		if v == nil {
			return true
		}
		_, ok := v.(map[string]any)
		return ok
	case typeOutcome:
		s, ok := v.(string)
		return ok && (s == "verified" || s == "failed" || s == "unknown")
	default:
		return false
	}
}

// actionFieldTypes is the type each action field holds, by its serde name. A name means the same type in every action
// that carries it, so one table types the fields of all twelve. actionKeys says which fields each action may hold.
var actionFieldTypes = map[string]fieldType{
	"type": typeString, "url": typeString, "selector": typeString, "snapshot_id": typeString,
	"generation": typeU32, "locator": typeObject, "text": typeString, "value": typeString,
	"timeout_ms": typeU32, "key": typeString, "target": typeString, "ms": typeU32,
}

// locatorFieldTypes is the type each SemanticLocator field holds, by its serde name. A name means the same type in
// every kind, so one table types the fields of all six kinds. A role's name is written only when it is set.
var locatorFieldTypes = map[string]fieldType{
	"kind": typeString, "role": typeString, "name": typeString, "exact": typeBool,
	"text": typeString, "label": typeString, "placeholder": typeString, "test_id": typeString,
	"selector": typeString, "index": typeU32,
}

// expectFieldTypes is the type each ExpectCondition field holds, by its serde name. A within is an object or null.
var expectFieldTypes = map[string]fieldType{
	"kind": typeString, "pattern": typeString, "locator": typeObject, "text": typeString, "within": typeObjectOrNull,
}

// stepObjectFieldTypes is the type each field of a step's target and expect holds, by its serde name.
var stepObjectFieldTypes = map[string]fieldType{
	"locator": typeObject, "fingerprint_id": typeString, "tag": typeString, "all": typeArray,
}

var (
	topLevelKeys = map[string]bool{
		"procedure_id": true, "version": true, "source_receipt_ids": true, "steps": true, "slots": true,
	}
	stepKeys = map[string]bool{
		"step": true, "action": true, "expected_outcome": true, "redacted_input": true,
		"target": true, "slot": true, "expect": true,
	}
	slotKeys   = map[string]bool{"name": true, "step": true, "kind": true}
	slotKinds  = map[string]bool{"text": true, "secret": true}
	slotNameRE = regexp.MustCompile(`^[A-Za-z0-9_.-]{1,64}$`)
	tagRE      = regexp.MustCompile(`^[a-z0-9._-]{1,32}$`)
	idRE       = regexp.MustCompile(`^proc_[0-9a-f-]{36}$`)
)

// Summary is what Validate learned about an accepted body.
type Summary struct {
	// Digest is the lowercase hex sha256 of Canonical.
	Digest string
	// StepCount is the number of steps.
	StepCount int
	// SlotNames are the slot names in declaration order; empty when the body types nothing.
	SlotNames []string
	// Canonical is the body re-marshalled with sorted object keys and normalised numbers. It is the
	// form the store keeps, so the digest always describes the stored bytes.
	Canonical []byte
}

// Validate accepts only a BrowserProcedure v2 body (version 2, 1..MaxSteps steps, at most
// MaxBodyBytes, the closed action vocabulary) that holds no typed value, no approval_grant_id and no
// redaction marker. It returns the digest and the canonical form to store. Every refusal is a
// *ValidationError.
func Validate(body []byte) (Summary, error) {
	if len(body) > MaxBodyBytes {
		return Summary{}, refuse(CodeBodyTooLarge, "a procedure body is at most %d bytes", MaxBodyBytes)
	}
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.UseNumber()
	var raw any
	if err := dec.Decode(&raw); err != nil {
		return Summary{}, refuse(CodeInvalidJSON, "the body is not valid JSON")
	}
	if _, err := dec.Token(); !errors.Is(err, io.EOF) {
		return Summary{}, refuse(CodeInvalidJSON, "the body is not exactly one JSON value")
	}
	root, ok := raw.(map[string]any)
	if !ok {
		return Summary{}, refuse(CodeInvalidJSON, "the body must be a JSON object")
	}

	// Pass 1, over the whole tree: forbidden keys, redaction markers, depth, and number
	// normalisation (so the canonical form does not depend on how a number was spelled).
	normalised, err := scan(root, 0)
	if err != nil {
		return Summary{}, err
	}
	root = normalised.(map[string]any)

	// Pass 2: the shape of a BrowserProcedure v2.
	summary, err := checkShape(root)
	if err != nil {
		return Summary{}, err
	}

	canonical, err := marshalCanonical(root)
	if err != nil {
		return Summary{}, refuse(CodeInvalidJSON, "the body cannot be re-encoded")
	}
	if len(canonical) > MaxBodyBytes {
		return Summary{}, refuse(CodeBodyTooLarge, "a procedure body is at most %d bytes", MaxBodyBytes)
	}
	summary.Canonical = canonical
	summary.Digest = DigestOf(canonical)
	return summary, nil
}

// Canonicalize re-encodes a stored body (JSON bytes read back from the database) in canonical form
// and returns it with its digest, without re-running the shape checks. A reader uses it to verify
// that a version still matches its recorded digest.
func Canonicalize(body []byte) (canonical []byte, digest string, err error) {
	dec := json.NewDecoder(bytes.NewReader(body))
	dec.UseNumber()
	var raw any
	if err := dec.Decode(&raw); err != nil {
		return nil, "", refuse(CodeInvalidJSON, "the body is not valid JSON")
	}
	normalised, verr := scan(raw, 0)
	if verr != nil {
		return nil, "", verr
	}
	canonical, merr := marshalCanonical(normalised)
	if merr != nil {
		return nil, "", refuse(CodeInvalidJSON, "the body cannot be re-encoded")
	}
	return canonical, DigestOf(canonical), nil
}

// DigestOf is the lowercase hex sha256 of a canonical body.
func DigestOf(canonical []byte) string {
	sum := sha256.Sum256(canonical)
	return hex.EncodeToString(sum[:])
}

// marshalCanonical encodes a decoded tree with sorted object keys (encoding/json sorts map keys),
// no HTML escaping and no trailing newline.
func marshalCanonical(v any) ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(v); err != nil {
		return nil, err
	}
	return bytes.TrimRight(buf.Bytes(), "\n"), nil
}

// scan walks a decoded tree and returns it with numbers normalised. It refuses an
// approval_grant_id key anywhere, a string (or key) containing "[redacted" in any case, a string (or
// key) with a NUL character (jsonb cannot store one), and nesting deeper than maxDepth. A refusal
// never repeats the offending value.
func scan(v any, depth int) (any, error) {
	if depth > maxDepth {
		return nil, refuse(CodeBodyTooDeep, "the body nests deeper than %d levels", maxDepth)
	}
	switch t := v.(type) {
	case map[string]any:
		for k, child := range t {
			if strings.EqualFold(k, "approval_grant_id") {
				return nil, refuse(CodeForbiddenKey, "a procedure never stores an approval_grant_id")
			}
			if hasRedactionMarker(k) {
				return nil, refuse(CodeRedactedMarker, "a key carries a redaction marker")
			}
			if strings.ContainsRune(k, 0) {
				return nil, refuse(CodeNulCharacter, "a key carries a NUL character")
			}
			out, err := scan(child, depth+1)
			if err != nil {
				return nil, err
			}
			t[k] = out
		}
		return t, nil
	case []any:
		for i, child := range t {
			out, err := scan(child, depth+1)
			if err != nil {
				return nil, err
			}
			t[i] = out
		}
		return t, nil
	case string:
		if hasRedactionMarker(t) {
			return nil, refuse(CodeRedactedMarker, "a string carries a redaction marker")
		}
		if strings.ContainsRune(t, 0) {
			return nil, refuse(CodeNulCharacter, "a string carries a NUL character")
		}
		return t, nil
	case json.Number:
		if i, err := strconv.ParseInt(string(t), 10, 64); err == nil {
			return i, nil
		}
		f, err := strconv.ParseFloat(string(t), 64)
		if err != nil {
			return nil, refuse(CodeInvalidJSON, "a number is out of range")
		}
		if f == 0 {
			// A negative zero encodes as -0, which jsonb reads back as 0 and which re-canonicalizes to a
			// different digest. Keep the one positive zero so the digest survives the round trip.
			return float64(0), nil
		}
		return f, nil
	default:
		return v, nil
	}
}

func hasRedactionMarker(s string) bool {
	return strings.Contains(strings.ToLower(s), "[redacted")
}

// checkShape checks the BrowserProcedure v2 structure of an already scanned root.
func checkShape(root map[string]any) (Summary, error) {
	for k := range root {
		if !topLevelKeys[k] {
			return Summary{}, refuse(CodeBadShape, "unknown top-level field %q", k)
		}
	}
	if v, ok := root["version"].(int64); !ok || v != BodyVersion {
		return Summary{}, refuse(CodeUnsupportedVersion, "only BrowserProcedure version %d is stored", BodyVersion)
	}
	if v, present := root["procedure_id"]; present {
		if _, ok := v.(string); !ok {
			return Summary{}, refuse(CodeBadShape, "procedure_id must be a string")
		}
	}
	if v, present := root["source_receipt_ids"]; present {
		ids, ok := v.([]any)
		if !ok {
			return Summary{}, refuse(CodeBadShape, "source_receipt_ids must be an array of strings")
		}
		for _, id := range ids {
			if _, ok := id.(string); !ok {
				return Summary{}, refuse(CodeBadShape, "source_receipt_ids must be an array of strings")
			}
		}
	}

	steps, ok := root["steps"].([]any)
	if !ok {
		return Summary{}, refuse(CodeBadShape, "steps must be an array")
	}
	if len(steps) < 1 || len(steps) > MaxSteps {
		return Summary{}, refuse(CodeBadStepCount, "a procedure has 1 to %d steps", MaxSteps)
	}

	slotStep, slotNames, err := readSlots(root)
	if err != nil {
		return Summary{}, err
	}

	used := make(map[string]bool, len(slotNames))
	for i, rawStep := range steps {
		step, ok := rawStep.(map[string]any)
		if !ok {
			return Summary{}, refuse(CodeBadShape, "step %d must be an object", i)
		}
		for k := range step {
			if !stepKeys[k] {
				return Summary{}, refuse(CodeBadShape, "step %d has an unknown field %q", i, k)
			}
		}
		stepNo, hasNo, err := optionalInt(step, "step")
		if err != nil {
			return Summary{}, refuse(CodeBadShape, "step %d: step must be a non-negative integer", i)
		}
		action, ok := step["action"].(map[string]any)
		if !ok {
			return Summary{}, refuse(CodeBadShape, "step %d: action must be an object", i)
		}
		actionType, ok := action["type"].(string)
		if !ok || !allowedActionTypes[actionType] {
			return Summary{}, refuse(CodeActionNotAllowed, "step %d: the action type is not in the allowed vocabulary", i)
		}
		for k := range action {
			if !actionKeys[actionType][k] {
				return Summary{}, refuse(CodeBadShape, "step %d: the action has an unknown field %q", i, k)
			}
		}
		if loc, present := action["locator"]; present {
			if err := checkLocator(i, "action", loc); err != nil {
				return Summary{}, err
			}
		}
		for _, field := range []string{"target", "expect"} {
			if v, present := step[field]; present {
				obj, ok := v.(map[string]any)
				if !ok {
					return Summary{}, refuse(CodeBadShape, "step %d: %s must be an object", i, field)
				}
				for k, child := range obj {
					if !stepObjectKeys[field][k] {
						return Summary{}, refuse(CodeBadShape, "step %d: %s has an unknown field %q", i, field, k)
					}
					if !stepObjectFieldTypes[k].holds(child) {
						return Summary{}, refuse(CodeBadShape, "step %d: %s field %q has the wrong type", i, field, k)
					}
				}
				if loc, present := obj["locator"]; present {
					if err := checkLocator(i, field, loc); err != nil {
						return Summary{}, err
					}
				}
				if field == "expect" {
					if err := checkExpectConditions(i, obj["all"]); err != nil {
						return Summary{}, err
					}
				}
			}
		}
		if v, present := step["redacted_input"]; present {
			if _, ok := v.(bool); !ok {
				return Summary{}, refuse(CodeBadShape, "step %d: redacted_input must be a boolean", i)
			}
		}
		if v, present := step["expected_outcome"]; present && !typeOutcome.holds(v) {
			return Summary{}, refuse(CodeBadShape, "step %d: expected_outcome must be verified, failed or unknown", i)
		}

		slotRaw, hasSlot := step["slot"]
		if typedActionTypes[actionType] {
			// Typed values are never stored: empty text, and a slot the replay fills.
			if text, present := action["text"]; present {
				s, isString := text.(string)
				if !isString || s != "" {
					return Summary{}, refuse(CodeTypedText, "step %d: a typed step carries no text", i)
				}
			}
			name, isString := slotRaw.(string)
			if !hasSlot || !isString || name == "" {
				return Summary{}, refuse(CodeBadSlot, "step %d: a typed step names a slot", i)
			}
			declaredAt, declared := slotStep[name]
			if !declared {
				return Summary{}, refuse(CodeBadSlot, "step %d: the slot is not listed in slots", i)
			}
			if used[name] {
				return Summary{}, refuse(CodeBadSlot, "step %d: a slot is used by one step only", i)
			}
			if declaredAt >= 0 && hasNo && declaredAt != stepNo {
				return Summary{}, refuse(CodeBadSlot, "step %d: the slot belongs to another step", i)
			}
			used[name] = true
		} else if hasSlot {
			return Summary{}, refuse(CodeBadSlot, "step %d: only a typed step names a slot", i)
		}
		// Each field of the action holds the scalar its Quarry field expects. A typed step's text was checked above, so a
		// text that is not an empty string still reports CodeTypedText.
		for k, v := range action {
			if !actionFieldTypes[k].holds(v) {
				return Summary{}, refuse(CodeBadShape, "step %d: the action field %q has the wrong type", i, k)
			}
		}
	}
	for _, name := range slotNames {
		if !used[name] {
			return Summary{}, refuse(CodeBadSlot, "a listed slot is used by no typed step")
		}
	}

	names := slotNames
	if names == nil {
		names = []string{}
	}
	return Summary{StepCount: len(steps), SlotNames: names}, nil
}

// checkLocator refuses a locator (owner names whether it is an action's or a target's) that is not an object of
// a known kind, that holds a field its kind does not allow, or that holds a field of the wrong type. The refusal
// names the step and no field, since a typed value can sit under any key; a wrong type names the field, which the
// kind's closed set has already admitted, and never the value.
func checkLocator(step int, owner string, v any) error {
	loc, ok := v.(map[string]any)
	if !ok {
		return refuse(CodeBadShape, "step %d: the %s locator must be an object", step, owner)
	}
	kind, _ := loc["kind"].(string)
	fields, known := locatorKeys[kind]
	if !known {
		return refuse(CodeBadShape, "step %d: the %s locator has an unknown kind", step, owner)
	}
	for k, child := range loc {
		if !fields[k] {
			return refuse(CodeBadShape, "step %d: the %s locator has a field its kind does not allow", step, owner)
		}
		if !locatorFieldTypes[k].holds(child) {
			return refuse(CodeBadShape, "step %d: the %s locator field %q has the wrong type", step, owner, k)
		}
	}
	return nil
}

// checkExpectConditions refuses an expect.all that is not an array of condition objects, a condition of an unknown
// kind, and a condition that holds a field its kind does not allow or a field of the wrong type. A condition's locator,
// and a within that is set, are checked as locators. Quarry writes within as null when a text condition names no
// element, so a null within is absent.
func checkExpectConditions(step int, all any) error {
	conditions, ok := all.([]any)
	if !ok {
		return refuse(CodeBadShape, "step %d: expect.all must be an array of conditions", step)
	}
	for _, raw := range conditions {
		cond, ok := raw.(map[string]any)
		if !ok {
			return refuse(CodeBadShape, "step %d: an expect condition must be an object", step)
		}
		kind, _ := cond["kind"].(string)
		fields, known := expectConditionKeys[kind]
		if !known {
			return refuse(CodeBadShape, "step %d: an expect condition has an unknown kind", step)
		}
		for k, child := range cond {
			if !fields[k] {
				return refuse(CodeBadShape, "step %d: an expect condition has a field its kind does not allow", step)
			}
			if !expectFieldTypes[k].holds(child) {
				return refuse(CodeBadShape, "step %d: an expect condition field %q has the wrong type", step, k)
			}
		}
		if loc, present := cond["locator"]; present {
			if err := checkLocator(step, "expect", loc); err != nil {
				return err
			}
		}
		if loc, present := cond["within"]; present && loc != nil {
			if err := checkLocator(step, "expect", loc); err != nil {
				return err
			}
		}
	}
	return nil
}

// readSlots reads the optional slots array. It returns, per slot name, the step number the slot
// declares (-1 when it declares none) and the names in declaration order.
func readSlots(root map[string]any) (map[string]int64, []string, error) {
	rawSlots, present := root["slots"]
	if !present {
		return map[string]int64{}, nil, nil
	}
	list, ok := rawSlots.([]any)
	if !ok {
		return nil, nil, refuse(CodeBadShape, "slots must be an array")
	}
	if len(list) > MaxSlots {
		return nil, nil, refuse(CodeBadSlot, "a procedure has at most %d slots", MaxSlots)
	}
	declared := make(map[string]int64, len(list))
	names := make([]string, 0, len(list))
	for i, rawSlot := range list {
		slot, ok := rawSlot.(map[string]any)
		if !ok {
			return nil, nil, refuse(CodeBadSlot, "slot %d must be an object", i)
		}
		for k := range slot {
			if !slotKeys[k] {
				return nil, nil, refuse(CodeBadSlot, "slot %d has an unknown field %q", i, k)
			}
		}
		name, ok := slot["name"].(string)
		if !ok || len(name) > maxSlotNameLen || !slotNameRE.MatchString(name) {
			return nil, nil, refuse(CodeBadSlot, "slot %d: name must match %s", i, slotNameRE.String())
		}
		if _, dup := declared[name]; dup {
			return nil, nil, refuse(CodeBadSlot, "slot %d: slot names are unique", i)
		}
		kind, ok := slot["kind"].(string)
		if !ok || !slotKinds[kind] {
			return nil, nil, refuse(CodeBadSlot, "slot %d: kind is text or secret", i)
		}
		at := int64(-1)
		if n, has, err := optionalInt(slot, "step"); err != nil {
			return nil, nil, refuse(CodeBadSlot, "slot %d: step must be a non-negative integer", i)
		} else if has {
			at = n
		}
		declared[name] = at
		names = append(names, name)
	}
	return declared, names, nil
}

// optionalInt reads an optional field of an already scanned object that Quarry holds as a u32: a non-negative integer
// no larger than math.MaxUint32.
func optionalInt(m map[string]any, key string) (int64, bool, error) {
	v, present := m[key]
	if !present {
		return 0, false, nil
	}
	n, ok := v.(int64)
	if !ok || n < 0 || n > math.MaxUint32 {
		return 0, true, errors.New("not a non-negative integer")
	}
	return n, true, nil
}

// ValidateTag checks a tag name against ^[a-z0-9._-]{1,32}$.
func ValidateTag(tag string) error {
	if len(tag) > MaxTagLen || !tagRE.MatchString(tag) {
		return refuse(CodeBadTag, "a tag matches %s", tagRE.String())
	}
	return nil
}

// ValidID reports whether id has the shape of a procedure id ("proc_" and a UUID).
func ValidID(id string) bool { return idRE.MatchString(id) }

// Meta is the descriptive metadata of a procedure.
type Meta struct {
	Name        string
	Description string
	SourceRunID string
}

// NormalizeMeta trims the metadata and checks it against the limits the migration repeats as CHECK
// constraints: a name of 1..120 characters, a description of at most 500, a source run id of at most
// 128 bytes, and no control character in any of them (a description may hold a newline or tab).
func NormalizeMeta(m Meta) (Meta, error) {
	m.Name = strings.TrimSpace(m.Name)
	m.Description = strings.TrimSpace(m.Description)
	m.SourceRunID = strings.TrimSpace(m.SourceRunID)
	if n := utf8.RuneCountInString(m.Name); n < 1 || n > MaxNameRunes || !utf8.ValidString(m.Name) || hasControl(m.Name, false) {
		return Meta{}, refuse(CodeBadName, "a name is 1 to %d characters without control characters", MaxNameRunes)
	}
	if utf8.RuneCountInString(m.Description) > MaxDescriptionRunes || !utf8.ValidString(m.Description) || hasControl(m.Description, true) {
		return Meta{}, refuse(CodeBadDescription, "a description is at most %d characters", MaxDescriptionRunes)
	}
	if len(m.SourceRunID) > MaxSourceRunIDBytes || !utf8.ValidString(m.SourceRunID) || hasControl(m.SourceRunID, false) {
		return Meta{}, refuse(CodeBadSourceRun, "a source run id is at most %d bytes", MaxSourceRunIDBytes)
	}
	return m, nil
}

func hasControl(s string, allowWhitespace bool) bool {
	for _, r := range s {
		if !unicode.IsControl(r) {
			continue
		}
		if allowWhitespace && (r == '\n' || r == '\t' || r == '\r') {
			continue
		}
		return true
	}
	return false
}
