package tracker

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"regexp"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/redis/go-redis/v9"
	"github.com/sergi/go-diff/diffmatchpatch"

	"github.com/triodelab/quarry/internal/models"
)

type ChangeTracker struct {
	storage     sync.Map
	redisClient *redis.Client
	ttl         time.Duration
	done        chan struct{}
	closeOnce   sync.Once
}

func NewChangeTracker() *ChangeTracker {
	tracker := &ChangeTracker{ttl: resolveTTL(), done: make(chan struct{})}
	if client := initRedisClient(); client != nil {
		tracker.redisClient = client
	}
	go tracker.cleanupLoop()
	return tracker
}

func resolveTTL() time.Duration {
	ttlHours := 24 * 30
	if raw := strings.TrimSpace(os.Getenv("CHANGE_TRACKING_TTL_HOURS")); raw != "" {
		if parsed, err := strconv.Atoi(raw); err == nil && parsed > 0 {
			ttlHours = parsed
		}
	}
	return time.Duration(ttlHours) * time.Hour
}

func initRedisClient() *redis.Client {
	backend := strings.ToLower(strings.TrimSpace(os.Getenv("CHANGE_TRACKING_BACKEND")))
	if backend == "" {
		if strings.EqualFold(strings.TrimSpace(os.Getenv("CACHE_BACKEND")), "redis") {
			backend = "redis"
		}
	}
	if backend != "redis" {
		return nil
	}

	redisURL := strings.TrimSpace(os.Getenv("REDIS_URL"))
	if redisURL == "" {
		return nil
	}

	opts, err := redis.ParseURL(redisURL)
	if err != nil {
		return nil
	}

	client := redis.NewClient(opts)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if err := client.Ping(ctx).Err(); err != nil {
		_ = client.Close()
		return nil
	}

	return client
}

func (ct *ChangeTracker) Track(ctx context.Context, key string, currentContent string, req *models.ChangeTrackingRequest) (*models.ChangeTrackingResult, error) {
	return ct.check(ctx, key, currentContent, req, true)
}

func (ct *ChangeTracker) Compare(ctx context.Context, key string, currentContent string, req *models.ChangeTrackingRequest) (*models.ChangeTrackingResult, error) {
	return ct.check(ctx, key, currentContent, req, false)
}

func (ct *ChangeTracker) check(ctx context.Context, key string, currentContent string, req *models.ChangeTrackingRequest, persist bool) (*models.ChangeTrackingResult, error) {
	_ = ctx
	if req == nil || !req.Enabled || key == "" {
		return nil, nil
	}

	normalizedCurrent := ct.normalizeContent(currentContent)
	currentHash := ct.hashContent(normalizedCurrent)

	storageKey := ct.buildKey(key, req.Tag)
	previous := ct.getPreviousScrape(ctx, storageKey)

	result := &models.ChangeTrackingResult{Visibility: "visible"}

	if previous == nil {
		result.ChangeStatus = "new"
		result.PreviousScrapeAt = nil
	} else {
		result.PreviousScrapeAt = &previous.Timestamp
		if previous.Hash == currentHash {
			result.ChangeStatus = "same"
		} else {
			result.ChangeStatus = "changed"

			if ct.hasMode(req.Modes, "git-diff") {
				diff, err := ct.generateDiff(previous.Content, normalizedCurrent)
				if err == nil {
					result.Diff = diff
				}
			}

			if ct.hasMode(req.Modes, "json") {
				jsonComparison, err := ct.generateJSONComparison(previous.Content, normalizedCurrent)
				if err == nil {
					result.JSON = jsonComparison
				}
			}
		}
	}

	if persist {
		ct.storeScrape(storageKey, &models.StoredScrape{
			URL:       key,
			Content:   normalizedCurrent,
			Timestamp: time.Now(),
			Tag:       req.Tag,
			Hash:      currentHash,
		})
	}

	return result, nil
}

func (ct *ChangeTracker) GetLatest(ctx context.Context, key string, tag string) (*models.StoredScrape, bool) {
	if strings.TrimSpace(key) == "" {
		return nil, false
	}
	storageKey := ct.buildKey(key, tag)
	if stored := ct.getPreviousScrape(ctx, storageKey); stored != nil {
		return stored, true
	}
	return nil, false
}

func (ct *ChangeTracker) normalizeContent(content string) string {
	iframeRegex := regexp.MustCompile(`<iframe[^>]*src="[^"]*"[^>]*>`)
	normalized := iframeRegex.ReplaceAllString(content, "<iframe>")
	whitespaceRegex := regexp.MustCompile(`\s+`)
	normalized = whitespaceRegex.ReplaceAllString(normalized, " ")
	return strings.TrimSpace(normalized)
}

func (ct *ChangeTracker) hashContent(content string) string {
	hash := sha256.Sum256([]byte(content))
	return hex.EncodeToString(hash[:])
}

func (ct *ChangeTracker) generateDiff(oldContent, newContent string) (*models.DiffResult, error) {
	dmp := diffmatchpatch.New()
	diffs := dmp.DiffMain(oldContent, newContent, false)
	diffs = dmp.DiffCleanupSemantic(diffs)
	patches := dmp.PatchMake(oldContent, diffs)
	diffText := dmp.PatchToText(patches)
	structuredDiff := ct.buildStructuredDiff(diffs)

	return &models.DiffResult{Text: diffText, JSON: structuredDiff}, nil
}

func (ct *ChangeTracker) buildStructuredDiff(diffs []diffmatchpatch.Diff) *models.StructuredDiff {
	oldFile := "previous"
	newFile := "current"

	file := models.DiffFile{From: &oldFile, To: &newFile, Chunks: []models.DiffChunk{}}
	chunk := models.DiffChunk{Content: "@@ Changes @@", Changes: []models.DiffChange{}}

	lineNum1 := 1
	lineNum2 := 1

	for _, diff := range diffs {
		lines := strings.Split(diff.Text, "\n")
		for _, line := range lines {
			if line == "" {
				continue
			}

			change := models.DiffChange{Content: line}
			switch diff.Type {
			case diffmatchpatch.DiffDelete:
				change.Type = "delete"
				change.Ln1 = &lineNum1
				lineNum1++
			case diffmatchpatch.DiffInsert:
				change.Type = "add"
				change.Ln2 = &lineNum2
				lineNum2++
			case diffmatchpatch.DiffEqual:
				change.Type = "normal"
				change.Normal = true
				ln := lineNum1
				change.Ln = &ln
				lineNum1++
				lineNum2++
			}
			chunk.Changes = append(chunk.Changes, change)
		}
	}

	file.Chunks = append(file.Chunks, chunk)
	return &models.StructuredDiff{Files: []models.DiffFile{file}}
}

func (ct *ChangeTracker) generateJSONComparison(oldContent, newContent string) (map[string]any, error) {
	var oldData map[string]any
	var newData map[string]any

	if err := json.Unmarshal([]byte(oldContent), &oldData); err != nil {
		return nil, fmt.Errorf("unmarshal old content: %w", err)
	}
	if err := json.Unmarshal([]byte(newContent), &newData); err != nil {
		return nil, fmt.Errorf("unmarshal new content: %w", err)
	}

	comparison := make(map[string]any)
	keys := make(map[string]bool)
	for k := range oldData {
		keys[k] = true
	}
	for k := range newData {
		keys[k] = true
	}

	for key := range keys {
		oldVal, oldExists := oldData[key]
		newVal, newExists := newData[key]
		if !oldExists || !newExists || fmt.Sprint(oldVal) != fmt.Sprint(newVal) {
			comparison[key] = models.FieldComparison{Previous: oldVal, Current: newVal}
		}
	}

	return comparison, nil
}

func (ct *ChangeTracker) buildKey(url, tag string) string {
	if tag != "" {
		return fmt.Sprintf("%s::%s", url, tag)
	}
	return url
}

func (ct *ChangeTracker) getPreviousScrape(ctx context.Context, key string) *models.StoredScrape {
	val, ok := ct.storage.Load(key)
	if !ok {
		if ct.redisClient == nil {
			return nil
		}
		payload, err := ct.redisClient.Get(ctx, ct.redisKey(key)).Bytes()
		if err != nil {
			return nil
		}
		var stored models.StoredScrape
		if err := json.Unmarshal(payload, &stored); err != nil {
			return nil
		}
		ct.storage.Store(key, &stored)
		return &stored
	}
	stored, ok := val.(*models.StoredScrape)
	if !ok {
		return nil
	}
	return stored
}

func (ct *ChangeTracker) storeScrape(key string, scrape *models.StoredScrape) {
	ct.storage.Store(key, scrape)
	if ct.redisClient != nil && scrape != nil {
		if payload, err := json.Marshal(scrape); err == nil {
			_ = ct.redisClient.Set(context.Background(), ct.redisKey(key), payload, ct.ttl).Err()
		}
	}
}

func (ct *ChangeTracker) redisKey(key string) string {
	return "change_tracking:" + key
}

func (ct *ChangeTracker) hasMode(modes []string, target string) bool {
	if len(modes) == 0 {
		return target == "git-diff"
	}
	for _, mode := range modes {
		if mode == target {
			return true
		}
	}
	return false
}

func (ct *ChangeTracker) cleanupLoop() {
	ticker := time.NewTicker(1 * time.Hour)
	defer ticker.Stop()
	for {
		select {
		case <-ticker.C:
			ct.cleanup()
		case <-ct.done:
			return
		}
	}
}

func (ct *ChangeTracker) cleanup() {
	cutoff := time.Now().Add(-30 * 24 * time.Hour)
	ct.storage.Range(func(key, value interface{}) bool {
		scrape, ok := value.(*models.StoredScrape)
		if ok && scrape.Timestamp.Before(cutoff) {
			ct.storage.Delete(key)
		}
		return true
	})
}

// Close stops background cleanup and releases the optional Redis client.
func (ct *ChangeTracker) Close() error {
	var closeErr error
	ct.closeOnce.Do(func() {
		close(ct.done)
		if ct.redisClient != nil {
			closeErr = ct.redisClient.Close()
			ct.redisClient = nil
		}
	})
	return closeErr
}
