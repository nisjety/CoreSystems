package extractor

import (
	"context"
	"strings"

	"github.com/PuerkitoBio/goquery"

	"github.com/triodelab/quarry/internal/config"
	"github.com/triodelab/quarry/internal/models"
)

type JSONExtractor struct {
	deployment string
}

func NewJSONExtractor(cfg *config.Config) (*JSONExtractor, error) {
	return &JSONExtractor{deployment: cfg.AzureOpenAIModel}, nil
}

func (e *JSONExtractor) Extract(ctx context.Context, content string, format models.JSONFormat) (map[string]interface{}, error) {
	result := map[string]interface{}{}

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(content))
	if err == nil {
		result["title"] = strings.TrimSpace(doc.Find("title").First().Text())
		result["h1"] = strings.TrimSpace(doc.Find("h1").First().Text())
	}

	if format.Prompt != "" {
		result["prompt"] = format.Prompt
	}
	if format.Schema != nil {
		result["schema"] = format.Schema
	}
	result["deployment"] = e.deployment
	return result, nil
}
