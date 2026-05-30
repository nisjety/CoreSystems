package scraper

import (
	"context"
	"encoding/json"
	"strings"

	"github.com/PuerkitoBio/goquery"

	"github.com/triodelab/quarry/internal/config"
)

type PageStructureAnalyzer struct {
	deployment string
}

func NewPageStructureAnalyzer(cfg *config.Config) *PageStructureAnalyzer {
	return &PageStructureAnalyzer{deployment: cfg.AzureOpenAIModel}
}

type AnalysisResult struct {
	PageType       string           `json:"pageType"`
	MainContent    string           `json:"mainContent"`
	Items          []ItemSelector   `json:"items"`
	Fields         []FieldExtractor `json:"fields"`
	SkipSelectors  []string         `json:"skipSelectors"`
	WaitCondition  string           `json:"waitCondition"`
	ScrollRequired bool             `json:"scrollRequired"`
	JavaScriptCode string           `json:"javaScriptCode"`
	Confidence     float64          `json:"confidence"`
}

type ItemSelector struct {
	Name     string `json:"name"`
	Selector string `json:"selector"`
}

type FieldExtractor struct {
	Name      string `json:"name"`
	Selector  string `json:"selector"`
	Attribute string `json:"attribute"`
	Type      string `json:"type"`
	Optional  bool   `json:"optional"`
}

func (psa *PageStructureAnalyzer) AnalyzePageStructure(ctx context.Context, htmlContent, purpose string) (*AnalysisResult, error) {
	_ = ctx
	_ = purpose

	doc, err := goquery.NewDocumentFromReader(strings.NewReader(htmlContent))
	if err != nil {
		return &AnalysisResult{PageType: "unknown", Confidence: 0.2}, nil
	}

	pageType := "article"
	if doc.Find(".product, [itemtype*='Product'], [class*='product']").Length() > 0 {
		pageType = "product_listing"
	}
	if doc.Find(".product-detail, [itemprop='name']").Length() > 0 {
		pageType = "single_product"
	}

	return &AnalysisResult{
		PageType:    pageType,
		MainContent: "main, .main, #main, .content",
		Items: []ItemSelector{{
			Name:     "primaryItems",
			Selector: ".product, article, li",
		}},
		Fields: []FieldExtractor{
			{Name: "title", Selector: "h1, h2, h3", Attribute: "text", Type: "string", Optional: false},
			{Name: "url", Selector: "a[href]", Attribute: "href", Type: "url", Optional: true},
		},
		SkipSelectors:  []string{"script", "style", "nav", "footer"},
		WaitCondition:  "body",
		ScrollRequired: false,
		Confidence:     0.6,
	}, nil
}

func (psa *PageStructureAnalyzer) GenerateExtractionScript(ctx context.Context, analysis *AnalysisResult) (string, error) {
	_ = ctx
	if analysis == nil {
		analysis = &AnalysisResult{}
	}
	payload, err := json.Marshal(analysis)
	if err != nil {
		return "", err
	}
	return "// generated phase-1 extraction script\nconst analysis = " + string(payload) + ";", nil
}
