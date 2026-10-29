package jetstream_test

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/model-plane/pkg/jetstream"
)

const fixturesDir = "../../../rust/crates/mp-events/tests/fixtures"

func readFixture(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(fixturesDir, name))
	if err != nil {
		t.Fatalf("read fixture %s: %v", name, err)
	}
	return data
}

func TestStreamSpec_DecodeFixture(t *testing.T) {
	data := readFixture(t, "jetstream_stream_spec.json")
	s, err := jetstream.DecodeStream(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if s.Name == "" {
		t.Fatal("expected name")
	}
	if len(s.Subjects) == 0 {
		t.Fatal("expected subjects")
	}
	if s.Retention != jetstream.RetentionLimits {
		t.Fatalf("retention: got %q", s.Retention)
	}
	if s.Storage != jetstream.StorageFile {
		t.Fatalf("storage: got %q", s.Storage)
	}
	if s.Discard != jetstream.DiscardOld {
		t.Fatalf("discard: got %q", s.Discard)
	}
	if s.MaxAgeSecs == 0 || s.Replicas == 0 {
		t.Fatal("expected non-zero max_age_secs and replicas")
	}
	if err := s.Validate(); err != nil {
		t.Fatalf("validate: %v", err)
	}
}

func TestStreamSpec_Roundtrip(t *testing.T) {
	data := readFixture(t, "jetstream_stream_spec.json")
	s, err := jetstream.DecodeStream(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	encoded, err := s.Encode()
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	s2, err := jetstream.DecodeStream(encoded)
	if err != nil {
		t.Fatalf("re-decode: %v", err)
	}
	if s2.Name != s.Name || len(s2.Subjects) != len(s.Subjects) ||
		s2.Retention != s.Retention || s2.Storage != s.Storage ||
		s2.MaxAgeSecs != s.MaxAgeSecs || s2.Replicas != s.Replicas ||
		s2.Discard != s.Discard {
		t.Fatalf("roundtrip mismatch: %+v vs %+v", s, s2)
	}
}

func TestStreamSpec_MissingFieldsFixture(t *testing.T) {
	data := readFixture(t, "jetstream_stream_missing_fields.json")
	s, err := jetstream.DecodeStream(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if err := s.Validate(); err == nil {
		t.Fatal("expected validation error for missing-fields fixture")
	}
}

func TestStreamSpec_Validate_MissingName(t *testing.T) {
	s := &jetstream.StreamSpec{
		Subjects:   []string{"a.b"},
		Retention:  jetstream.RetentionLimits,
		Storage:    jetstream.StorageFile,
		MaxAgeSecs: 1,
		Replicas:   1,
		Discard:    jetstream.DiscardOld,
	}
	err := s.Validate()
	if err == nil || !strings.Contains(err.Error(), "name") {
		t.Fatalf("expected name error, got: %v", err)
	}
}

func TestStreamSpec_Validate_EmptySubjects(t *testing.T) {
	s := &jetstream.StreamSpec{
		Name:       "S",
		Subjects:   []string{},
		Retention:  jetstream.RetentionLimits,
		Storage:    jetstream.StorageFile,
		MaxAgeSecs: 1,
		Replicas:   1,
		Discard:    jetstream.DiscardOld,
	}
	err := s.Validate()
	if err == nil || !strings.Contains(err.Error(), "subjects") {
		t.Fatalf("expected subjects error, got: %v", err)
	}
}

func TestStreamSpec_Validate_EmptySubjectEntry(t *testing.T) {
	s := &jetstream.StreamSpec{
		Name:       "S",
		Subjects:   []string{""},
		Retention:  jetstream.RetentionLimits,
		Storage:    jetstream.StorageFile,
		MaxAgeSecs: 1,
		Replicas:   1,
		Discard:    jetstream.DiscardOld,
	}
	err := s.Validate()
	if err == nil || !strings.Contains(err.Error(), "subjects") {
		t.Fatalf("expected subjects error, got: %v", err)
	}
}

func TestStreamSpec_Validate_ZeroMaxAge(t *testing.T) {
	s := &jetstream.StreamSpec{
		Name:      "S",
		Subjects:  []string{"a"},
		Retention: jetstream.RetentionLimits,
		Storage:   jetstream.StorageFile,
		Replicas:  1,
		Discard:   jetstream.DiscardOld,
	}
	err := s.Validate()
	if err == nil || !strings.Contains(err.Error(), "max_age_secs") {
		t.Fatalf("expected max_age_secs error, got: %v", err)
	}
}

func TestStreamSpec_Validate_ZeroReplicas(t *testing.T) {
	s := &jetstream.StreamSpec{
		Name:       "S",
		Subjects:   []string{"a"},
		Retention:  jetstream.RetentionLimits,
		Storage:    jetstream.StorageFile,
		MaxAgeSecs: 1,
		Discard:    jetstream.DiscardOld,
	}
	err := s.Validate()
	if err == nil || !strings.Contains(err.Error(), "replicas") {
		t.Fatalf("expected replicas error, got: %v", err)
	}
}

func TestConsumerSpec_DecodeFixture(t *testing.T) {
	data := readFixture(t, "jetstream_consumer_spec.json")
	c, err := jetstream.DecodeConsumer(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if c.Durable == "" {
		t.Fatal("expected durable")
	}
	if c.AckPolicy != jetstream.AckExplicit {
		t.Fatalf("ack_policy: got %q", c.AckPolicy)
	}
	if c.AckWaitSecs == 0 || c.MaxDeliver == 0 || c.FilterSubject == "" {
		t.Fatal("expected non-zero consumer fields")
	}
	if err := c.Validate(); err != nil {
		t.Fatalf("validate: %v", err)
	}
}

func TestConsumerSpec_Roundtrip(t *testing.T) {
	data := readFixture(t, "jetstream_consumer_spec.json")
	c, err := jetstream.DecodeConsumer(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	encoded, err := c.Encode()
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	c2, err := jetstream.DecodeConsumer(encoded)
	if err != nil {
		t.Fatalf("re-decode: %v", err)
	}
	if *c != *c2 {
		t.Fatalf("roundtrip mismatch: %+v vs %+v", c, c2)
	}
}

func TestConsumerSpec_MissingFieldsFixture(t *testing.T) {
	data := readFixture(t, "jetstream_consumer_missing_fields.json")
	c, err := jetstream.DecodeConsumer(data)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	if err := c.Validate(); err == nil {
		t.Fatal("expected validation error for missing-fields fixture")
	}
}

func TestConsumerSpec_Validate_MissingDurable(t *testing.T) {
	c := &jetstream.ConsumerSpec{
		AckPolicy:     jetstream.AckExplicit,
		AckWaitSecs:   1,
		MaxDeliver:    1,
		FilterSubject: "a",
	}
	err := c.Validate()
	if err == nil || !strings.Contains(err.Error(), "durable") {
		t.Fatalf("expected durable error, got: %v", err)
	}
}

func TestConsumerSpec_Validate_ZeroAckWait(t *testing.T) {
	c := &jetstream.ConsumerSpec{
		Durable:       "d",
		AckPolicy:     jetstream.AckExplicit,
		MaxDeliver:    1,
		FilterSubject: "a",
	}
	err := c.Validate()
	if err == nil || !strings.Contains(err.Error(), "ack_wait_secs") {
		t.Fatalf("expected ack_wait_secs error, got: %v", err)
	}
}

func TestConsumerSpec_Validate_ZeroMaxDeliver(t *testing.T) {
	c := &jetstream.ConsumerSpec{
		Durable:       "d",
		AckPolicy:     jetstream.AckExplicit,
		AckWaitSecs:   1,
		FilterSubject: "a",
	}
	err := c.Validate()
	if err == nil || !strings.Contains(err.Error(), "max_deliver") {
		t.Fatalf("expected max_deliver error, got: %v", err)
	}
}

func TestConsumerSpec_Validate_MissingFilterSubject(t *testing.T) {
	c := &jetstream.ConsumerSpec{
		Durable:     "d",
		AckPolicy:   jetstream.AckExplicit,
		AckWaitSecs: 1,
		MaxDeliver:  1,
	}
	err := c.Validate()
	if err == nil || !strings.Contains(err.Error(), "filter_subject") {
		t.Fatalf("expected filter_subject error, got: %v", err)
	}
}
