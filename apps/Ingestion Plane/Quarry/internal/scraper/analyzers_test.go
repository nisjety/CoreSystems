package scraper

import "testing"

func TestAnalyzeSEO(t *testing.T) {
	t.Parallel()

	html := `<!doctype html>
<html lang="en">
<head>
  <title>Pricing | Quarry</title>
  <meta name="description" content="Compare Quarry plans.">
  <link rel="canonical" href="/pricing">
  <meta property="og:title" content="Pricing | Quarry">
</head>
<body>
  <h1>Pricing</h1>
</body>
</html>`

	result := analyzeSEO("https://example.com/pricing", html)
	if result["title"] != "Pricing | Quarry" {
		t.Fatalf("title = %#v, want Pricing | Quarry", result["title"])
	}
	if result["canonicalUrl"] != "https://example.com/pricing" {
		t.Fatalf("canonicalUrl = %#v, want resolved canonical", result["canonicalUrl"])
	}
	issues, _ := result["issues"].([]string)
	if len(issues) != 0 {
		t.Fatalf("issues = %v, want none", issues)
	}
}

func TestAnalyzeWCAG(t *testing.T) {
	t.Parallel()

	html := `<!doctype html>
<html>
<head></head>
<body>
  <img src="/hero.png">
  <button></button>
  <input id="email">
  <h1>Main</h1>
  <h3>Skipped</h3>
</body>
</html>`

	result := analyzeWCAG(html)
	if result["imagesMissingAlt"] != 1 {
		t.Fatalf("imagesMissingAlt = %#v, want 1", result["imagesMissingAlt"])
	}
	if result["buttonsMissingAccessibleName"] != 1 {
		t.Fatalf("buttonsMissingAccessibleName = %#v, want 1", result["buttonsMissingAccessibleName"])
	}
	if result["formInputsMissingLabel"] != 1 {
		t.Fatalf("formInputsMissingLabel = %#v, want 1", result["formInputsMissingLabel"])
	}
	if result["headingOrderIssues"] != 1 {
		t.Fatalf("headingOrderIssues = %#v, want 1", result["headingOrderIssues"])
	}
}

func TestBuildPageStatus(t *testing.T) {
	t.Parallel()

	okStatus := buildPageStatus("https://example.com", 200, "text/html")
	if okStatus["status"] != "completed" {
		t.Fatalf("status = %#v, want completed", okStatus["status"])
	}

	failStatus := buildPageStatus("https://example.com/missing", 404, "text/html")
	if failStatus["status"] != "failed_fetch" {
		t.Fatalf("status = %#v, want failed_fetch", failStatus["status"])
	}
}
