// Package voice implements a three-stage voice pipeline: STT -> LLM -> TTS.
// Each stage is defined as an interface so concrete providers can be swapped in
// without changing pipeline logic.
package voice

import (
	"fmt"
)

// STTProvider converts audio bytes to transcript text.
type STTProvider interface {
	Transcribe(audioBytes []byte) (string, error)
}

// LLMProvider accepts a text prompt and returns a response.
type LLMProvider interface {
	Generate(text string) (string, error)
}

// TTSProvider converts text to audio bytes.
type TTSProvider interface {
	Synthesize(text string) ([]byte, error)
}

// VoicePipeline chains STT, LLM, and TTS stages into a single end-to-end
// audio-in / audio-out pipeline.
type VoicePipeline struct {
	stt STTProvider
	llm LLMProvider
	tts TTSProvider
}

// NewVoicePipeline constructs a pipeline from the given stage providers.
func NewVoicePipeline(stt STTProvider, llm LLMProvider, tts TTSProvider) *VoicePipeline {
	return &VoicePipeline{stt: stt, llm: llm, tts: tts}
}

// RunPipeline executes the full voice pipeline: audio input is transcribed via
// STT, the transcript is sent to the LLM, and the LLM response is synthesized
// back to audio via TTS.
func (p *VoicePipeline) RunPipeline(audioInput []byte) (audioOutput []byte, transcript string, response string, err error) {
	transcript, err = p.stt.Transcribe(audioInput)
	if err != nil {
		return nil, "", "", fmt.Errorf("stt stage failed: %w", err)
	}

	response, err = p.llm.Generate(transcript)
	if err != nil {
		return nil, transcript, "", fmt.Errorf("llm stage failed: %w", err)
	}

	audioOutput, err = p.tts.Synthesize(response)
	if err != nil {
		return nil, transcript, response, fmt.Errorf("tts stage failed: %w", err)
	}

	return audioOutput, transcript, response, nil
}

// ---------------------------------------------------------------------------
// Noop implementations — pass data through unchanged for testing / defaults.
// ---------------------------------------------------------------------------

// NoopSTT is a pass-through STT provider that treats the audio bytes as a
// UTF-8 string and returns it directly as the transcript.
type NoopSTT struct{}

// Transcribe interprets audioBytes as UTF-8 text and returns it.
func (NoopSTT) Transcribe(audioBytes []byte) (string, error) {
	return string(audioBytes), nil
}

// NoopLLM is a pass-through LLM provider that echoes the input text.
type NoopLLM struct{}

// Generate returns the input text unchanged.
func (NoopLLM) Generate(text string) (string, error) {
	return text, nil
}

// NoopTTS is a pass-through TTS provider that converts the input text to
// bytes and returns them as "audio".
type NoopTTS struct{}

// Synthesize returns the text encoded as UTF-8 bytes.
func (NoopTTS) Synthesize(text string) ([]byte, error) {
	return []byte(text), nil
}
