package conversation

import (
	"context"
	"math"
	"strings"
	"time"
)

// SupportRecurrenceMaxCandidates and SupportRecurrenceSimilarityThreshold
// bound the response per the design gate: at most 3 authorized ticket
// identifiers as evidence, and a cosine-similarity floor below which a
// result is not a candidate at all.
const (
	SupportRecurrenceMaxCandidates        = 3
	SupportRecurrenceSimilarityThreshold  = 0.82
	supportRecurrenceStatusCandidateFound = "candidate_found"
	supportRecurrenceStatusNoCandidate    = "no_candidate"
	supportRecurrenceStatusUnavailable    = "unavailable"
)

// SupportRecurrenceCandidate is one bounded piece of evidence: a ticket ID
// the caller is already authorized to see (re-derived from the anchor
// ticket's own org, never trusted from client input). Deliberately nothing
// more — no score, no inferred relationship — matching the design gate's
// "similarity candidates, never a shared cause" requirement.
type SupportRecurrenceCandidate struct {
	TicketID string `json:"ticket_id"`
}

// SupportRecurrenceResult is the full bounded-evidence response, modeled on
// the Verified Outcome Foundation's VerificationResult shape (status +
// method/version + bounded evidence, never a stronger claim than the method
// supports).
type SupportRecurrenceResult struct {
	// Status is "candidate_found" | "no_candidate" | "unavailable" — never a
	// stronger claim (e.g. never "confirmed" or "same cause").
	Status              string                       `json:"status"`
	Candidates          []SupportRecurrenceCandidate `json:"candidates"`
	AlgorithmVersion    string                       `json:"algorithm_version,omitempty"`
	CorpusWindowStart   *time.Time                   `json:"corpus_window_start,omitempty"`
	SimilarityThreshold float64                      `json:"similarity_threshold,omitempty"`
}

// FindSupportRecurrenceCandidates returns bounded semantic similarity
// candidates for one anchor ticket. The anchor is always re-derived from
// orgID via the existing org-scoped GetTicket accessor — anchorTicketID is
// never trusted as a bare lookup key, the same pattern
// ai_action_executor.go's executeTicketUpdate uses for ticket.update
// proposals. A ZDR-enabled org's corpus is never populated in the first
// place (see consumers.SupportRecurrenceCorpusBuilder), so this naturally
// returns "unavailable" for such an org without needing its own ZDR check.
func (s *Service) FindSupportRecurrenceCandidates(ctx context.Context, orgID, anchorTicketID string) (*SupportRecurrenceResult, error) {
	orgID = strings.TrimSpace(orgID)
	anchorTicketID = strings.TrimSpace(anchorTicketID)
	if orgID == "" || anchorTicketID == "" {
		return nil, ErrInvalidInput
	}

	// Confirms the anchor ticket is real and belongs to this org before doing
	// anything else — GetTicket is already org-scoped (WHERE org_id = $1 AND
	// id = $2), so a foreign-org or nonexistent ticket id returns ErrNotFound
	// here rather than silently falling through to an empty corpus lookup.
	if _, err := s.GetTicket(ctx, orgID, anchorTicketID); err != nil {
		return nil, err
	}

	corpus, err := s.repository.ListSupportRecurrenceCorpus(ctx, orgID)
	if err != nil {
		return nil, err
	}
	if len(corpus) == 0 {
		return &SupportRecurrenceResult{Status: supportRecurrenceStatusUnavailable}, nil
	}

	var anchorEmbedding []float32
	found := false
	for _, entry := range corpus {
		if entry.TicketID == anchorTicketID {
			anchorEmbedding = entry.Embedding
			found = true
			break
		}
	}
	if !found {
		// The anchor ticket exists but has no corpus entry yet (e.g. created
		// since the last sweep, or filtered out by supportRecurrenceEmbeddingText
		// having nothing to embed). Honest "not yet available", not "no match".
		return &SupportRecurrenceResult{Status: supportRecurrenceStatusUnavailable}, nil
	}

	type scored struct {
		ticketID   string
		similarity float64
	}
	var candidates []scored
	for _, entry := range corpus {
		if entry.TicketID == anchorTicketID {
			continue
		}
		similarity := cosineSimilarity(anchorEmbedding, entry.Embedding)
		if similarity >= SupportRecurrenceSimilarityThreshold {
			candidates = append(candidates, scored{ticketID: entry.TicketID, similarity: similarity})
		}
	}
	// Simple insertion sort by descending similarity — candidates is bounded
	// to a handful of matches above threshold in practice, so this stays
	// cheap without pulling in sort for one bounded slice.
	for i := 1; i < len(candidates); i++ {
		for j := i; j > 0 && candidates[j].similarity > candidates[j-1].similarity; j-- {
			candidates[j], candidates[j-1] = candidates[j-1], candidates[j]
		}
	}
	if len(candidates) > SupportRecurrenceMaxCandidates {
		candidates = candidates[:SupportRecurrenceMaxCandidates]
	}

	result := &SupportRecurrenceResult{
		Candidates:          make([]SupportRecurrenceCandidate, 0, len(candidates)),
		SimilarityThreshold: SupportRecurrenceSimilarityThreshold,
	}
	for _, c := range candidates {
		result.Candidates = append(result.Candidates, SupportRecurrenceCandidate{TicketID: c.ticketID})
	}
	if len(result.Candidates) == 0 {
		result.Status = supportRecurrenceStatusNoCandidate
	} else {
		result.Status = supportRecurrenceStatusCandidateFound
	}
	return result, nil
}

// cosineSimilarity returns 0 for mismatched dimensions or a zero vector
// rather than an error — an embedding-provider drift or corrupt row must
// never propagate as a false-positive match.
func cosineSimilarity(a, b []float32) float64 {
	if len(a) != len(b) || len(a) == 0 {
		return 0
	}
	var dot, normA, normB float64
	for i := range a {
		dot += float64(a[i]) * float64(b[i])
		normA += float64(a[i]) * float64(a[i])
		normB += float64(b[i]) * float64(b[i])
	}
	if normA == 0 || normB == 0 {
		return 0
	}
	return dot / (math.Sqrt(normA) * math.Sqrt(normB))
}
