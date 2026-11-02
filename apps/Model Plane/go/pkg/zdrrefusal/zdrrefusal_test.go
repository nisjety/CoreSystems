package zdrrefusal

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"testing"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

type trailerStream struct{ trailers metadata.MD }

func (s *trailerStream) Method() string                  { return "/test/Method" }
func (s *trailerStream) SetHeader(metadata.MD) error     { return nil }
func (s *trailerStream) SendHeader(metadata.MD) error    { return nil }
func (s *trailerStream) SetTrailer(md metadata.MD) error { s.trailers = metadata.Join(s.trailers, md); return nil }

func streamContext() (context.Context, *trailerStream) {
	stream := &trailerStream{}
	return grpc.NewContextWithServerTransportStream(context.Background(), stream), stream
}

// The embedded catalogue is a vendored copy; the owner's file is
// api/contracts/zdr-refusals.json (three levels above this package).
func TestEmbeddedCatalogueIsByteIdenticalToTheOwnersContract(t *testing.T) {
	owner := filepath.Join("..", "..", "..", "api", "contracts", "zdr-refusals.json")
	want, err := os.ReadFile(owner)
	if err != nil {
		t.Skipf("owner contract not reachable from this checkout: %v", err)
	}
	if !bytes.Equal(catalogueJSON, want) {
		t.Fatal("embedded zdr-refusals.json drifted from api/contracts/zdr-refusals.json; run scripts/tests/zdr-contract-sync.test.sh")
	}
	// The local copy on disk must also be what is embedded.
	local, err := os.ReadFile("zdr-refusals.json")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(local, catalogueJSON) {
		t.Fatal("embedded bytes differ from the file on disk")
	}
}

func TestCatalogueLoadsAndKnowsEveryCodeConstant(t *testing.T) {
	if _, err := load(); err != nil {
		t.Fatalf("catalogue does not parse: %v", err)
	}
	for _, code := range []string{DurableMutationForbidden, FeatureNotSupported, PostureMismatch, DecisionUnavailable} {
		if !Known(code) {
			t.Errorf("constant %q is not a catalogue code", code)
		}
		if Message(code, "attachments") == "" {
			t.Errorf("code %q has no sentence", code)
		}
	}
	if Known("not_a_code") || Message("not_a_code", "") != "" {
		t.Fatal("unknown code must be unknown with an empty sentence")
	}
}

func TestStatusIsFailedPreconditionWithTheCatalogueSentence(t *testing.T) {
	err := Status(DurableMutationForbidden, "")
	st := status.Convert(err)
	if st.Code() != codes.FailedPrecondition {
		t.Fatalf("code = %v", st.Code())
	}
	if st.Message() != Message(DurableMutationForbidden, "") || st.Message() == "" {
		t.Fatalf("message = %q", st.Message())
	}
}

func TestRefuseEmitsTheKindTrailer(t *testing.T) {
	ctx, stream := streamContext()
	err := Refuse(ctx, DurableMutationForbidden, "")
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v", status.Code(err))
	}
	if got := stream.trailers.Get(KindTrailer); len(got) != 1 || got[0] != DurableMutationForbidden {
		t.Fatalf("kind trailer = %v", got)
	}
	if got := stream.trailers.Get(FeatureTrailer); len(got) != 0 {
		t.Fatalf("a code that takes no feature must send no feature trailer, got %v", got)
	}
}

func TestRefuseEmitsKindAndFeatureTrailersForAFeatureCode(t *testing.T) {
	ctx, stream := streamContext()
	err := Refuse(ctx, FeatureNotSupported, "attachments")
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v", status.Code(err))
	}
	if got := stream.trailers.Get(KindTrailer); len(got) != 1 || got[0] != FeatureNotSupported {
		t.Fatalf("kind trailer = %v", got)
	}
	if got := stream.trailers.Get(FeatureTrailer); len(got) != 1 || got[0] != "attachments" {
		t.Fatalf("feature trailer = %v", got)
	}
	if msg := status.Convert(err).Message(); msg != "This is not supported with zero data retention yet: file and image attachments. The request was refused before any model call." {
		t.Fatalf("message did not substitute the feature label: %q", msg)
	}
}

func TestUnknownCodeDegradesToAnUntypedRefusal(t *testing.T) {
	ctx, stream := streamContext()
	err := Refuse(ctx, "made_up_code", "")
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v", status.Code(err))
	}
	if len(stream.trailers) != 0 {
		t.Fatalf("an unknown code must not write a trailer, got %v", stream.trailers)
	}
}

func TestUnknownFeatureDegradesToAnUntypedRefusal(t *testing.T) {
	ctx, stream := streamContext()
	err := Refuse(ctx, FeatureNotSupported, "not_a_feature")
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v", status.Code(err))
	}
	if status.Convert(err).Message() != "zero data retention refusal" {
		t.Fatalf("message = %q", status.Convert(err).Message())
	}
	// Status degrades to untyped, and Refuse (known code) still writes its kind
	// trailer; the feature trailer carries the caller's key, which no client
	// can resolve. Pin the shape so a change is deliberate.
	if got := stream.trailers.Get(KindTrailer); len(got) != 1 {
		t.Fatalf("kind trailer = %v", got)
	}
}

func TestRefuseWithNoServerStreamJustReturnsTheStatus(t *testing.T) {
	err := Refuse(context.Background(), DurableMutationForbidden, "")
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v", status.Code(err))
	}
}
