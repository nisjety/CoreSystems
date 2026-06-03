package commands

import (
	"context"
	"strings"
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"google.golang.org/grpc"
)

// fakeModels / fakeCompactor are the tiny fakes the narrow client interfaces
// allow — no running inference-core/session-core needed.
type fakeModels struct {
	resp *mpv1.ListModelsResponse
	err  error
	got  *mpv1.ListModelsRequest
}

func (f *fakeModels) ListModels(
	_ context.Context, in *mpv1.ListModelsRequest, _ ...grpc.CallOption,
) (*mpv1.ListModelsResponse, error) {
	f.got = in
	return f.resp, f.err
}

type fakeCompactor struct {
	resp *mpv1.CompactNowResponse
	err  error
	got  *mpv1.CompactNowRequest
}

func (f *fakeCompactor) CompactNow(
	_ context.Context, in *mpv1.CompactNowRequest, _ ...grpc.CallOption,
) (*mpv1.CompactNowResponse, error) {
	f.got = in
	return f.resp, f.err
}

func TestDispatchModels_DelegatesToInferenceListModels(t *testing.T) {
	fm := &fakeModels{resp: &mpv1.ListModelsResponse{Models: []*mpv1.ModelInfo{
		{Id: "gpt-4o", Provider: "openai", Modality: "chat", Streaming: true},
		{Id: "whisper-1", Provider: "openai", Modality: "speech"},
	}}}
	h := NewHandler().WithModels(fm)

	res := h.dispatch(context.Background(), "/models",
		CommandExecRequest{Args: map[string]string{"modality": "chat", "provider": "openai"}})

	if !res.Success {
		t.Fatalf("expected success, got %+v", res)
	}
	// Filter args forwarded to the canonical owner (no duplicate filtering here).
	if fm.got.GetModality() != "chat" || fm.got.GetProvider() != "openai" {
		t.Fatalf("filter args not forwarded: %+v", fm.got)
	}
	if !strings.Contains(res.Output, "gpt-4o") || !strings.Contains(res.Output, "(streaming)") {
		t.Fatalf("output missing real model detail: %q", res.Output)
	}
}

func TestDispatchModels_NilClientReportsUnavailableNotFakeSuccess(t *testing.T) {
	res := NewHandler().dispatch(context.Background(), "/models", CommandExecRequest{})
	if res.Success || !strings.Contains(res.Output, "unavailable") {
		t.Fatalf("nil models client must report unavailable (not fake success): %+v", res)
	}
}

func TestDispatchCompact_DelegatesToSessionCompactNow(t *testing.T) {
	fc := &fakeCompactor{resp: &mpv1.CompactNowResponse{Summary: "compacted 3 checkpoint(s)"}}
	h := NewHandler().WithCompactor(fc)

	res := h.dispatch(context.Background(), "/compact",
		CommandExecRequest{Args: map[string]string{"toon": "true"}})

	if !res.Success || res.Output != "compacted 3 checkpoint(s)" {
		t.Fatalf("compact should return the session-core summary, got %+v", res)
	}
	if !fc.got.GetToon() {
		t.Fatal("toon arg must be forwarded to CompactNow")
	}
}

func TestDispatchCompact_NilClientReportsUnavailable(t *testing.T) {
	res := NewHandler().dispatch(context.Background(), "/compact", CommandExecRequest{})
	if res.Success || !strings.Contains(res.Output, "unavailable") {
		t.Fatalf("nil compactor must report unavailable: %+v", res)
	}
}

func TestDispatchHelp_ListsEnabledCommands(t *testing.T) {
	res := NewHandler().dispatch(context.Background(), "/help", CommandExecRequest{})
	if !res.Success || !strings.Contains(res.Output, "Available commands") {
		t.Fatalf("help should list the catalog, got %+v", res)
	}
}

func TestDispatchUnknown_IsHonestAboutNoServerAction(t *testing.T) {
	// A command with no server-side handler must NOT fabricate a success result.
	res := NewHandler().dispatch(context.Background(), "/clear", CommandExecRequest{})
	if !strings.Contains(res.Output, "client-handled") {
		t.Fatalf("default must honestly report no server-side action, got %+v", res)
	}
}
