package conversation

import (
	"errors"
	"testing"
)

func TestCosineSimilarity(t *testing.T) {
	cases := []struct {
		name string
		a, b []float32
		want float64
	}{
		{"identical vectors", []float32{1, 0, 0}, []float32{1, 0, 0}, 1},
		{"orthogonal vectors", []float32{1, 0}, []float32{0, 1}, 0},
		{"opposite vectors", []float32{1, 0}, []float32{-1, 0}, -1},
		{"mismatched lengths", []float32{1, 0}, []float32{1, 0, 0}, 0},
		{"zero vector", []float32{0, 0}, []float32{1, 1}, 0},
		{"empty vectors", nil, nil, 0},
	}
	for _, tc := range cases {
		if got := cosineSimilarity(tc.a, tc.b); got != tc.want {
			t.Errorf("%s: cosineSimilarity() = %v, want %v", tc.name, got, tc.want)
		}
	}
}

func TestFindSupportRecurrenceCandidates_AnchorNotFound(t *testing.T) {
	repo := newFakeRepository()
	service := NewService(repo, &fakePublisher{})

	_, err := service.FindSupportRecurrenceCandidates(t.Context(), "org-1", "missing-ticket")
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("err = %v, want ErrNotFound", err)
	}
}

func TestFindSupportRecurrenceCandidates_AnchorNotYetInCorpus(t *testing.T) {
	repo := newFakeRepository()
	repo.tickets["t1"] = &Ticket{ID: "t1", OrgID: "org-1"}
	service := NewService(repo, &fakePublisher{})

	result, err := service.FindSupportRecurrenceCandidates(t.Context(), "org-1", "t1")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Status != supportRecurrenceStatusUnavailable {
		t.Fatalf("status = %q, want %q", result.Status, supportRecurrenceStatusUnavailable)
	}
	if len(result.Candidates) != 0 {
		t.Fatalf("candidates = %#v, want none", result.Candidates)
	}
}

func TestFindSupportRecurrenceCandidates_ReturnsBoundedSortedCandidatesAboveThreshold(t *testing.T) {
	repo := newFakeRepository()
	repo.tickets["anchor"] = &Ticket{ID: "anchor", OrgID: "org-1"}
	repo.supportRecurrenceCorpus = []SupportRecurrenceCorpusEntry{
		{TicketID: "anchor", Embedding: []float32{1, 0, 0}},
		{TicketID: "near-1", Embedding: []float32{0.99, 0.01, 0}},   // very close
		{TicketID: "near-2", Embedding: []float32{0.95, 0.05, 0.1}}, // close
		{TicketID: "near-3", Embedding: []float32{0.9, 0.1, 0.2}},   // above threshold, lowest of the three
		{TicketID: "far", Embedding: []float32{0, 1, 0}},            // orthogonal, well below threshold
	}
	service := NewService(repo, &fakePublisher{})

	result, err := service.FindSupportRecurrenceCandidates(t.Context(), "org-1", "anchor")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Status != supportRecurrenceStatusCandidateFound {
		t.Fatalf("status = %q, want %q", result.Status, supportRecurrenceStatusCandidateFound)
	}
	if len(result.Candidates) != 3 {
		t.Fatalf("candidates = %#v, want exactly 3 (bounded, excluding the anchor and the below-threshold entry)", result.Candidates)
	}
	// Descending similarity: near-1 (closest) first, near-3 (least close of the three) last.
	if result.Candidates[0].TicketID != "near-1" || result.Candidates[2].TicketID != "near-3" {
		t.Fatalf("candidates not sorted by descending similarity: %#v", result.Candidates)
	}
	for _, c := range result.Candidates {
		if c.TicketID == "anchor" {
			t.Fatalf("anchor ticket must never appear in its own candidate list: %#v", result.Candidates)
		}
		if c.TicketID == "far" {
			t.Fatalf("below-threshold ticket must not appear as a candidate: %#v", result.Candidates)
		}
	}
	if result.SimilarityThreshold != SupportRecurrenceSimilarityThreshold {
		t.Fatalf("similarity threshold = %v, want %v", result.SimilarityThreshold, SupportRecurrenceSimilarityThreshold)
	}
}

func TestFindSupportRecurrenceCandidates_NoCandidateAboveThreshold(t *testing.T) {
	repo := newFakeRepository()
	repo.tickets["anchor"] = &Ticket{ID: "anchor", OrgID: "org-1"}
	repo.supportRecurrenceCorpus = []SupportRecurrenceCorpusEntry{
		{TicketID: "anchor", Embedding: []float32{1, 0}},
		{TicketID: "far", Embedding: []float32{0, 1}},
	}
	service := NewService(repo, &fakePublisher{})

	result, err := service.FindSupportRecurrenceCandidates(t.Context(), "org-1", "anchor")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Status != supportRecurrenceStatusNoCandidate {
		t.Fatalf("status = %q, want %q", result.Status, supportRecurrenceStatusNoCandidate)
	}
	if len(result.Candidates) != 0 {
		t.Fatalf("candidates = %#v, want none", result.Candidates)
	}
}
