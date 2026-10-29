package events

import "testing"

func TestSubjectsDefaultPrefix(t *testing.T) {
	t.Parallel()

	s := NewSubjects("")
	cases := map[string]string{
		"item.upserted":  s.ItemUpserted(),
		"item.deleted":   s.ItemDeleted(),
		"source.synced":  s.SourceSynced(),
		"audit.emitted":  s.AuditEmitted(),
	}
	for suffix, got := range cases {
		want := "finspo." + suffix
		if got != want {
			t.Errorf("%s = %q, want %q", suffix, got, want)
		}
	}
}

func TestSubjectsCustomPrefix(t *testing.T) {
	t.Parallel()

	s := NewSubjects("finspo-prod")
	if got, want := s.ItemUpserted(), "finspo-prod.item.upserted"; got != want {
		t.Errorf("ItemUpserted = %q, want %q", got, want)
	}
}

func TestNilPublisherIsNoop(t *testing.T) {
	t.Parallel()

	var p *Publisher
	if err := p.Publish("finspo.test", map[string]string{"k": "v"}); err != nil {
		t.Errorf("nil Publish error = %v, want nil", err)
	}
	if !p.Healthy() {
		t.Errorf("nil Publisher must report healthy")
	}
	if err := p.Drain(); err != nil {
		t.Errorf("nil Drain error = %v, want nil", err)
	}
}

func TestEmptyURLProducesNoopPublisher(t *testing.T) {
	t.Parallel()

	p, err := Connect(t.Context(), "", "finspo-test")
	if err != nil {
		t.Fatalf("Connect with empty URL: %v", err)
	}
	if !p.Healthy() {
		t.Errorf("empty-URL Publisher must report healthy")
	}
	if err := p.Publish("finspo.test", map[string]string{"k": "v"}); err != nil {
		t.Errorf("Publish on no-op = %v, want nil", err)
	}
}
