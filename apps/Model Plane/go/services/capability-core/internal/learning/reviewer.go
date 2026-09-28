package learning

import (
	"encoding/json"
	"errors"
	"strings"
)

// Bounds on a review reply. They mirror the submit_skill_review schema the
// reviewer sends (llmreviewer's reviewSchema), so a reply the model produced
// within the schema is never rejected here.
const (
	maxReviewResponseBytes = 64 * 1024
	maxReviewSkills        = 8
	maxSkillNameBytes      = 160
	maxSkillContentBytes   = 8000
	maxSkillDescBytes      = 1000
	maxSkillKeywords       = 12
	maxSkillKeywordBytes   = 120
)

// ParseReason classifies why a review reply was rejected. It is a fixed label
// that carries nothing from the reply, so diagnostics can report it without
// logging model output derived from a customer conversation.
type ParseReason string

const (
	// ReasonNoJSON: no JSON object with a "skills" key was found.
	ReasonNoJSON ParseReason = "no_json"
	// ReasonSyntax: an object was there but is not valid JSON (typically cut
	// off mid-reply).
	ReasonSyntax ParseReason = "syntax"
	// ReasonMissingSkills: valid JSON, but "skills" is absent, null or not an
	// array.
	ReasonMissingSkills ParseReason = "missing_skills"
	// ReasonCandidateInvalid: a candidate breaks the schema or a field bound.
	ReasonCandidateInvalid ParseReason = "candidate_invalid"
	// ReasonSize: the reply, or its skills array, is over its bound.
	ReasonSize ParseReason = "size"
)

// ParseError is the rejection [ParseReviewResponse] and [ParseReviewJSON]
// return. Its message is fixed per reason and never quotes the reply.
type ParseError struct {
	Reason ParseReason
}

func (e *ParseError) Error() string {
	switch e.Reason {
	case ReasonNoJSON:
		return "review response contained no JSON object"
	case ReasonSyntax:
		return "review response is not valid JSON"
	case ReasonMissingSkills:
		return "review response requires a skills array"
	case ReasonCandidateInvalid:
		return "review candidate schema invalid"
	case ReasonSize:
		return "review response exceeded size limit"
	default:
		return "review response invalid"
	}
}

// ParseReviewResponse turns an LLM review's free-text reply into typed skill
// candidates. This is the risky, testable half of the G7 Reviewer: the model
// emits untrusted text, and we must extract well-formed candidates from it.
// The actual model call (inference-core) is thin glue layered on top.
//
// The model is asked (via ReviewPrompt) to emit JSON shaped as:
//
//	{"skills":[{"name","description","content","trigger_keywords":[...],"confidence":0.0-1.0}]}
//
// Real models routinely wrap JSON in ```json fences or surround it with prose;
// [extractJSONObject] tolerates both. A structured tool-call reply goes
// through [ParseReviewJSON] instead. Every returned candidate is forced to
// Origin=background_review (the producer can never mint a "user" skill).
func ParseReviewResponse(raw string) ([]SkillCandidate, error) {
	if len(raw) > maxReviewResponseBytes {
		return nil, &ParseError{Reason: ReasonSize}
	}
	js, ok := extractJSONObject(raw)
	if !ok {
		// A "skills" key that never decoded means the object was there but
		// broken; with no key at all there was no review object to find.
		if strings.Contains(raw, `"skills"`) {
			return nil, &ParseError{Reason: ReasonSyntax}
		}
		return nil, &ParseError{Reason: ReasonNoJSON}
	}
	return ParseReviewJSON(js)
}

// ParseReviewJSON validates a reply that is already exactly one JSON object:
// the arguments of the forced submit_skill_review tool call. It is strict —
// no fence stripping and no scanning; anything but a single object is
// rejected — because tool arguments are structured data, and text-style
// repair on them is what used to cut a skill's Markdown content apart.
func ParseReviewJSON(js string) ([]SkillCandidate, error) {
	if len(js) > maxReviewResponseBytes {
		return nil, &ParseError{Reason: ReasonSize}
	}
	var doc struct {
		Skills *[]json.RawMessage `json:"skills"`
	}
	if err := json.Unmarshal([]byte(js), &doc); err != nil {
		return nil, &ParseError{Reason: documentErrorReason(err)}
	}
	if doc.Skills == nil {
		return nil, &ParseError{Reason: ReasonMissingSkills}
	}
	if len(*doc.Skills) > maxReviewSkills {
		return nil, &ParseError{Reason: ReasonSize}
	}
	out := make([]SkillCandidate, 0, len(*doc.Skills))
	for _, item := range *doc.Skills {
		var s struct {
			Name            string   `json:"name"`
			Description     string   `json:"description"`
			Content         string   `json:"content"`
			TriggerKeywords []string `json:"trigger_keywords"`
			Confidence      *float64 `json:"confidence"`
		}
		if err := json.Unmarshal(item, &s); err != nil {
			return nil, &ParseError{Reason: ReasonCandidateInvalid}
		}
		if strings.TrimSpace(s.Name) == "" || len(s.Name) > maxSkillNameBytes || strings.TrimSpace(s.Content) == "" || len(s.Content) > maxSkillContentBytes || len(s.Description) > maxSkillDescBytes || s.Confidence == nil || *s.Confidence < 0 || *s.Confidence > 1 || len(s.TriggerKeywords) > maxSkillKeywords {
			return nil, &ParseError{Reason: ReasonCandidateInvalid}
		}
		for _, keyword := range s.TriggerKeywords {
			if len(keyword) > maxSkillKeywordBytes {
				return nil, &ParseError{Reason: ReasonCandidateInvalid}
			}
		}
		out = append(out, SkillCandidate{
			Name:            strings.TrimSpace(s.Name),
			Description:     s.Description,
			Content:         s.Content,
			TriggerKeywords: s.TriggerKeywords,
			Confidence:      *s.Confidence,
			Origin:          OriginBackgroundReview,
		})
	}
	return out, nil
}

// documentErrorReason classifies a failed decode of the whole review object.
// A type error at the top level means the reply was JSON but not an object; a
// type error on a field can only be "skills" (the one field decoded here)
// holding something other than an array.
func documentErrorReason(err error) ParseReason {
	var typeErr *json.UnmarshalTypeError
	if errors.As(err, &typeErr) {
		if typeErr.Field == "" {
			return ReasonNoJSON
		}
		return ReasonMissingSkills
	}
	return ReasonSyntax
}

// extractJSONObject finds the review object in free model text: the first
// complete JSON object, starting at any '{', that decodes and has a "skills"
// key. It decodes instead of slicing between ``` fences or from the first '{'
// to the last '}', so a skill whose content is Markdown — a fenced code block,
// an inline ```, a ```json example, literal braces — stays intact: those
// characters sit inside JSON strings, where the decoder never takes them for
// structure. Prose and fences around the object are skipped the same way.
//
// Every '{' starts a fresh decode, so text made of unclosed nested objects
// would cost time quadratic in its length; [scanBudget] caps the bytes decoded
// across all attempts, which a genuine reply never comes near.
func extractJSONObject(raw string) (string, bool) {
	budget := scanBudget
	for i := strings.IndexByte(raw, '{'); i >= 0 && budget > 0; {
		dec := json.NewDecoder(strings.NewReader(raw[i:]))
		var obj map[string]json.RawMessage
		if err := dec.Decode(&obj); err == nil {
			if _, ok := obj["skills"]; ok {
				return raw[i : i+int(dec.InputOffset())], true
			}
			budget -= int(dec.InputOffset())
		} else {
			budget -= decodedBytes(err, len(raw)-i)
		}
		next := strings.IndexByte(raw[i+1:], '{')
		if next < 0 {
			break
		}
		i += 1 + next
	}
	return "", false
}

// scanBudget is the most extractJSONObject decodes for one reply: four full
// passes over the largest reply accepted.
const scanBudget = 4 * maxReviewResponseBytes

// decodedBytes is how far a failed decode read: a syntax error reports its
// offset, and running out of input means it read everything that was left.
func decodedBytes(err error, remaining int) int {
	var syntaxErr *json.SyntaxError
	if errors.As(err, &syntaxErr) && syntaxErr.Offset > 0 {
		return int(syntaxErr.Offset)
	}
	return remaining
}
