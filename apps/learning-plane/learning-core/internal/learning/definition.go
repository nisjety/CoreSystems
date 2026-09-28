package learning

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"regexp"
	"strings"
)

var courseStepID = regexp.MustCompile(`^[a-z0-9][a-z0-9._-]{0,79}$`)

type courseDefinition struct {
	Version    int      `json:"version"`
	Application string  `json:"application"`
	Objectives []string `json:"objectives"`
	Steps      []struct {
		ID             string `json:"id"`
		Instruction    string `json:"instruction"`
		ExpectedResult string `json:"expectedResult"`
	} `json:"steps"`
}

// ValidCourseDefinition pins the stored lesson shape to a small, reviewable
// v1 contract. It contains no executable actions or model instructions.
func ValidCourseDefinition(raw json.RawMessage) bool {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var definition courseDefinition
	if decoder.Decode(&definition) != nil || definition.Version != 1 ||
		strings.TrimSpace(definition.Application) == "" || len([]rune(definition.Application)) > 120 ||
		len(definition.Objectives) == 0 || len(definition.Objectives) > 20 ||
		len(definition.Steps) == 0 || len(definition.Steps) > 100 {
		return false
	}
	for _, objective := range definition.Objectives {
		if strings.TrimSpace(objective) == "" || len([]rune(objective)) > 300 {
			return false
		}
	}
	seen := make(map[string]struct{}, len(definition.Steps))
	for _, step := range definition.Steps {
		step.ID = strings.TrimSpace(step.ID)
		if !courseStepID.MatchString(step.ID) ||
			strings.TrimSpace(step.Instruction) == "" || len([]rune(step.Instruction)) > 2000 ||
			strings.TrimSpace(step.ExpectedResult) == "" || len([]rune(step.ExpectedResult)) > 2000 {
			return false
		}
		if _, exists := seen[step.ID]; exists {
			return false
		}
		seen[step.ID] = struct{}{}
	}
	var trailing any
	return errors.Is(decoder.Decode(&trailing), io.EOF)
}
