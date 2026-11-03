package authz

import (
	"context"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/pkg/zdrrefusal"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

// trailerStream is a grpc.ServerTransportStream that records the trailers the
// gate sets, which is where mp-contracts' reader looks for the refusal kind.
type trailerStream struct {
	method   string
	trailers metadata.MD
}

func (s *trailerStream) Method() string                  { return s.method }
func (s *trailerStream) SetHeader(metadata.MD) error     { return nil }
func (s *trailerStream) SendHeader(metadata.MD) error    { return nil }
func (s *trailerStream) SetTrailer(md metadata.MD) error { s.trailers = metadata.Join(s.trailers, md); return nil }

func gateContext(method string, principal *authctx.Principal) (context.Context, *trailerStream) {
	stream := &trailerStream{method: method}
	ctx := grpc.NewContextWithServerTransportStream(context.Background(), stream)
	if principal != nil {
		ctx = authctx.ContextWithPrincipal(ctx, *principal)
	}
	return ctx, stream
}

func runGate(t *testing.T, gate grpc.UnaryServerInterceptor, method string, principal *authctx.Principal) (called bool, err error, stream *trailerStream) {
	t.Helper()
	ctx, stream := gateContext(method, principal)
	_, err = gate(ctx, struct{}{}, &grpc.UnaryServerInfo{FullMethod: method}, func(context.Context, any) (any, error) {
		called = true
		return "ok", nil
	})
	return called, err, stream
}

func zdrOn() *authctx.Principal {
	return &authctx.Principal{ZeroDataRetention: true, RetentionPolicyPresent: true, OrganizationID: "org-1", ActorID: "u1"}
}

func zdrOff() *authctx.Principal {
	return &authctx.Principal{ZeroDataRetention: false, RetentionPolicyPresent: true, OrganizationID: "org-1", ActorID: "u1"}
}

func zdrAbsent() *authctx.Principal {
	return &authctx.Principal{OrganizationID: "org-1", ActorID: "u1"}
}

func TestZDRGateRefusesAZDRPrincipalOnEverySandboxManagerMethod(t *testing.T) {
	t.Parallel()
	gate := zdrGate(false)
	if len(mpv1.SandboxManager_ServiceDesc.Methods) == 0 {
		t.Fatal("service descriptor lists no methods")
	}
	for _, m := range mpv1.SandboxManager_ServiceDesc.Methods {
		full := "/" + mpv1.SandboxManager_ServiceDesc.ServiceName + "/" + m.MethodName
		called, err, stream := runGate(t, gate, full, zdrOn())
		if called {
			t.Errorf("%s: handler ran for a ZDR principal", full)
		}
		if status.Code(err) != codes.FailedPrecondition {
			t.Errorf("%s: code = %v, want FailedPrecondition (err=%v)", full, status.Code(err), err)
		}
		if got := stream.trailers.Get(zdrrefusal.KindTrailer); len(got) != 1 || got[0] != zdrrefusal.DurableMutationForbidden {
			t.Errorf("%s: %s trailer = %v, want [%s]", full, zdrrefusal.KindTrailer, got, zdrrefusal.DurableMutationForbidden)
		}
		if got := stream.trailers.Get(zdrrefusal.FeatureTrailer); len(got) != 0 {
			t.Errorf("%s: durable-mutation refusal must carry no feature trailer, got %v", full, got)
		}
	}
	for _, m := range mpv1.SandboxManager_ServiceDesc.Streams {
		t.Logf("stream method %s is not covered by a unary interceptor", m.StreamName)
	}
}

func TestZDRGateRefusalUsesTheCatalogueSentence(t *testing.T) {
	t.Parallel()
	_, err, _ := runGate(t, zdrGate(false), "/model_plane.v1.SandboxManager/AcquireLease", zdrOn())
	if got, want := status.Convert(err).Message(), zdrrefusal.Message(zdrrefusal.DurableMutationForbidden, ""); got != want || got == "" {
		t.Fatalf("message = %q, want catalogue sentence %q", got, want)
	}
}

func TestZDRGateSkipsHealthEvenForAZDRPrincipalAndForNoPrincipal(t *testing.T) {
	t.Parallel()
	for _, principal := range []*authctx.Principal{zdrOn(), nil} {
		called, err, stream := runGate(t, zdrGate(true), "/grpc.health.v1.Health/Check", principal)
		if err != nil || !called {
			t.Fatalf("health must pass untouched, called=%v err=%v", called, err)
		}
		if len(stream.trailers) != 0 {
			t.Fatalf("health must set no trailers, got %v", stream.trailers)
		}
	}
}

func TestZDRGatePassesAZDRFalsePrincipal(t *testing.T) {
	t.Parallel()
	for _, requireClaim := range []bool{false, true} {
		called, err, _ := runGate(t, zdrGate(requireClaim), "/model_plane.v1.SandboxManager/AcquireLease", zdrOff())
		if err != nil || !called {
			t.Fatalf("requireClaim=%v: zdr:false principal refused, called=%v err=%v", requireClaim, called, err)
		}
	}
}

func TestZDRGatePassesAnAbsentClaimUnlessRequireClaimIsOn(t *testing.T) {
	t.Parallel()
	called, err, _ := runGate(t, zdrGate(false), "/model_plane.v1.SandboxManager/AcquireLease", zdrAbsent())
	if err != nil || !called {
		t.Fatalf("absent claim with requireClaim off must pass, called=%v err=%v", called, err)
	}

	called, err, stream := runGate(t, zdrGate(true), "/model_plane.v1.SandboxManager/AcquireLease", zdrAbsent())
	if called {
		t.Fatal("absent claim with requireClaim on must not reach the handler")
	}
	if status.Code(err) != codes.FailedPrecondition {
		t.Fatalf("code = %v, want FailedPrecondition", status.Code(err))
	}
	if got := stream.trailers.Get(zdrrefusal.KindTrailer); len(got) != 1 || got[0] != zdrrefusal.DurableMutationForbidden {
		t.Fatalf("kind trailer = %v", got)
	}
}

func TestZDRGateRefusesAMissingPrincipalFailClosed(t *testing.T) {
	t.Parallel()
	for _, requireClaim := range []bool{false, true} {
		called, err, _ := runGate(t, zdrGate(requireClaim), "/model_plane.v1.SandboxManager/AcquireLease", nil)
		if called {
			t.Fatal("handler ran with no verified principal")
		}
		if status.Code(err) != codes.Unauthenticated {
			t.Fatalf("code = %v, want Unauthenticated (err=%v)", status.Code(err), err)
		}
	}
}

func TestRequireRetentionClaimFromEnvIsOffUnlessExactlyOn(t *testing.T) {
	t.Parallel()
	cases := map[string]bool{"on": true, " on ": true, "ON": false, "true": false, "1": false, "": false, "off": false}
	for value, want := range cases {
		got := RequireRetentionClaimFromEnv(func(name string) string {
			if name != RequireRetentionClaimEnv {
				t.Fatalf("read unexpected env %q", name)
			}
			return value
		})
		if got != want {
			t.Errorf("%q => %v, want %v", value, got, want)
		}
	}
	if !strings.HasPrefix(RequireRetentionClaimEnv, "SANDBOX_MANAGER_") {
		t.Fatal("env name drifted")
	}
	// A nil getenv falls back to the process environment, which must not panic.
	_ = RequireRetentionClaimFromEnv(nil)
}
