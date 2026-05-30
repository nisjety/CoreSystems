package api

import (
	"context"
	"fmt"
	"strings"

	quarrycrawl "github.com/triodelab/quarry/internal/crawl"
)

func shouldSendPlatformWebhookEvent(events []string, eventType string) bool {
	if len(events) == 0 {
		return true
	}
	normalizedEvent := strings.ToLower(strings.TrimSpace(eventType))
	shortEvent := normalizedEvent
	if index := strings.LastIndex(normalizedEvent, "."); index >= 0 && index+1 < len(normalizedEvent) {
		shortEvent = normalizedEvent[index+1:]
	}
	for _, item := range events {
		normalized := strings.ToLower(strings.TrimSpace(item))
		if normalized == "" {
			continue
		}
		if normalized == normalizedEvent || normalized == shortEvent {
			return true
		}
	}
	return false
}

func (h *Handler) buildCrawlWebhookMetadata(ctx context.Context, run *quarrycrawl.Run) map[string]any {
	if h == nil || h.crawlStore == nil || run == nil {
		return nil
	}

	metadata := map[string]any{
		"preset":    run.Spec.Preset,
		"completed": run.Completed,
		"failed":    run.Failed,
		"blocked":   run.Blocked,
		"total":     run.Total,
	}
	if !run.Spec.ZDRMode {
		metadata["url"] = run.URL
	}

	docs, _, err := h.crawlStore.ListDocuments(ctx, run.ID, 0, max(run.Total, run.Completed))
	if err == nil && len(docs) > 0 {
		changeCounts := map[string]int{}
		pageStatusCounts := map[string]int{}
		httpStatusCounts := map[string]int{}
		alerts := make([]map[string]any, 0)

		for _, doc := range docs {
			docMeta := mapFromAny(doc.Metadata)
			pageStatus := mapFromAny(docMeta["pageStatus"])
			status := stringFromAny(pageStatus["status"], stringFromAny(docMeta["status"], ""))
			if status != "" {
				pageStatusCounts[status]++
			}

			httpStatus := intFromAny(pageStatus["httpStatus"])
			if httpStatus == 0 {
				httpStatus = intFromAny(docMeta["httpStatus"])
			}
			if httpStatus > 0 {
				httpStatusCounts[fmt.Sprintf("%d", httpStatus)]++
			}

			changeTracking := mapFromAny(docMeta["changeTracking"])
			changeStatus := strings.TrimSpace(stringFromAny(changeTracking["changeStatus"], ""))
			if changeStatus != "" {
				changeCounts[changeStatus]++
			}

			if alert := buildCrawlAlert(doc.URL, run.Spec.ZDRMode, status, httpStatus, changeStatus, pageStatus, changeTracking); len(alert) > 0 {
				alerts = append(alerts, alert)
			}
		}

		metadata["changes"] = changeCounts
		metadata["pageStatus"] = pageStatusCounts
		metadata["httpStatus"] = httpStatusCounts
		metadata["alerts"] = alerts
	}

	if pageErrors, listErr := h.crawlStore.ListErrors(ctx, run.ID); listErr == nil {
		metadata["errors"] = len(pageErrors)
	}
	if robotsBlocked, listErr := h.crawlStore.ListRobotsBlocked(ctx, run.ID); listErr == nil {
		metadata["robotsBlocked"] = len(robotsBlocked)
	}
	return metadata
}

func buildCrawlAlert(targetURL string, zdrMode bool, status string, httpStatus int, changeStatus string, pageStatus map[string]any, changeTracking map[string]any) map[string]any {
	if httpStatus < 400 && status == "completed" && changeStatus != "new" && changeStatus != "changed" && changeStatus != "removed" && changeStatus != "hidden" {
		return nil
	}

	alert := map[string]any{}
	if !zdrMode {
		alert["url"] = targetURL
	}
	if status != "" {
		alert["status"] = status
	}
	if httpStatus > 0 {
		alert["httpStatus"] = httpStatus
	}
	if changeStatus != "" {
		alert["changeStatus"] = changeStatus
	}
	if blockedReason := stringFromAny(pageStatus["blockedReason"], ""); blockedReason != "" {
		alert["blockedReason"] = blockedReason
	}
	if duplicateReason := stringFromAny(pageStatus["duplicateReason"], ""); duplicateReason != "" {
		alert["duplicateReason"] = duplicateReason
	}
	if summary := stringFromAny(changeTracking["summary"], ""); summary != "" {
		alert["changeSummary"] = summary
	}
	return alert
}
