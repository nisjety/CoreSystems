package nats

import (
	"encoding/json"
	"fmt"
)

func marshalJSON(data any) ([]byte, error) {
	b, err := json.Marshal(data)
	if err != nil {
		return nil, fmt.Errorf("marshal JSON: %w", err)
	}
	return b, nil
}
