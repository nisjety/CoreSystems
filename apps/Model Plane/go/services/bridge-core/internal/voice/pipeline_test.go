package voice

import (
	"errors"
	"testing"
)

func TestRunPipeline_NoopProviders(t *testing.T) {
	pipeline := NewVoicePipeline(NoopSTT{}, NoopLLM{}, NoopTTS{})

	input := []byte("hello world")
	audioOut, transcript, response, err := pipeline.RunPipeline(input)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if transcript != "hello world" {
		t.Errorf("transcript = %q, want %q", transcript, "hello world")
	}
	if response != "hello world" {
		t.Errorf("response = %q, want %q", response, "hello world")
	}
	if string(audioOut) != "hello world" {
		t.Errorf("audioOut = %q, want %q", string(audioOut), "hello world")
	}
}

func TestRunPipeline_EmptyInput(t *testing.T) {
	pipeline := NewVoicePipeline(NoopSTT{}, NoopLLM{}, NoopTTS{})

	audioOut, transcript, response, err := pipeline.RunPipeline([]byte{})
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}

	if transcript != "" {
		t.Errorf("transcript = %q, want empty", transcript)
	}
	if response != "" {
		t.Errorf("response = %q, want empty", response)
	}
	if len(audioOut) != 0 {
		t.Errorf("audioOut length = %d, want 0", len(audioOut))
	}
}

// failingSTT always returns an error.
type failingSTT struct{}

func (failingSTT) Transcribe([]byte) (string, error) {
	return "", errors.New("stt unavailable")
}

func TestRunPipeline_STTFailure(t *testing.T) {
	pipeline := NewVoicePipeline(failingSTT{}, NoopLLM{}, NoopTTS{})

	_, _, _, err := pipeline.RunPipeline([]byte("audio"))
	if err == nil {
		t.Fatal("expected error from STT stage, got nil")
	}
	if !errors.Is(err, errors.Unwrap(err)) {
		// Just verify the error wraps correctly
		unwrapped := errors.Unwrap(err)
		if unwrapped == nil {
			t.Error("expected wrapped error")
		}
	}
}

// failingLLM always returns an error.
type failingLLM struct{}

func (failingLLM) Generate(string) (string, error) {
	return "", errors.New("llm unavailable")
}

func TestRunPipeline_LLMFailure(t *testing.T) {
	pipeline := NewVoicePipeline(NoopSTT{}, failingLLM{}, NoopTTS{})

	_, transcript, _, err := pipeline.RunPipeline([]byte("audio"))
	if err == nil {
		t.Fatal("expected error from LLM stage, got nil")
	}
	// STT should have succeeded, so transcript is available.
	if transcript != "audio" {
		t.Errorf("transcript = %q, want %q", transcript, "audio")
	}
}

// failingTTS always returns an error.
type failingTTS struct{}

func (failingTTS) Synthesize(string) ([]byte, error) {
	return nil, errors.New("tts unavailable")
}

func TestRunPipeline_TTSFailure(t *testing.T) {
	pipeline := NewVoicePipeline(NoopSTT{}, NoopLLM{}, failingTTS{})

	_, transcript, response, err := pipeline.RunPipeline([]byte("audio"))
	if err == nil {
		t.Fatal("expected error from TTS stage, got nil")
	}
	// STT and LLM should have succeeded.
	if transcript != "audio" {
		t.Errorf("transcript = %q, want %q", transcript, "audio")
	}
	if response != "audio" {
		t.Errorf("response = %q, want %q", response, "audio")
	}
}
