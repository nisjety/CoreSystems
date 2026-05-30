package channel

import (
	"encoding/json"
	"fmt"
	"time"
)

// MessageFrame is the wire-level envelope for all messages flowing through a
// channel adapter. Every payload is wrapped in a frame with metadata.
type MessageFrame struct {
	// Type is one of "text", "binary", or "control".
	Type string `json:"type"`
	// Payload is the raw message content.
	Payload []byte `json:"payload"`
	// SessionID identifies the session this frame belongs to.
	SessionID string `json:"session_id"`
	// Timestamp records when the frame was created.
	Timestamp time.Time `json:"timestamp"`
}

// FrameCodec encodes and decodes MessageFrame values to and from bytes.
type FrameCodec interface {
	Encode(frame MessageFrame) ([]byte, error)
	Decode(data []byte) (MessageFrame, error)
}

// JSONFrameCodec implements FrameCodec using standard JSON encoding.
type JSONFrameCodec struct{}

// Encode serializes a MessageFrame to JSON bytes.
func (JSONFrameCodec) Encode(frame MessageFrame) ([]byte, error) {
	data, err := json.Marshal(frame)
	if err != nil {
		return nil, fmt.Errorf("failed to encode frame: %w", err)
	}
	return data, nil
}

// Decode deserializes JSON bytes into a MessageFrame.
func (JSONFrameCodec) Decode(data []byte) (MessageFrame, error) {
	var frame MessageFrame
	if err := json.Unmarshal(data, &frame); err != nil {
		return MessageFrame{}, fmt.Errorf("failed to decode frame: %w", err)
	}
	return frame, nil
}
