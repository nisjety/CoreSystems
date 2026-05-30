package main

import (
	"context"
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/finspo/internal/dataplane"
	"github.com/triodelab/finspo/internal/db"
	"github.com/triodelab/finspo/internal/store"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = zerolog.New(os.Stdout).With().Timestamp().Str("service", "finspo-source-object-backfill").Logger()

	ctx, cancel := context.WithTimeout(context.Background(), envDuration("FINSPO_BACKFILL_TIMEOUT", 30*time.Minute))
	defer cancel()

	dsn := strings.TrimSpace(os.Getenv("FINSPO_DSN"))
	if dsn == "" {
		log.Fatal().Msg("FINSPO_DSN is required")
	}
	dataPlaneURL := strings.TrimSpace(os.Getenv("DATA_PLANE_DOCUMENTS_BASE_URL"))
	if dataPlaneURL == "" {
		log.Fatal().Msg("DATA_PLANE_DOCUMENTS_BASE_URL is required")
	}

	pool, err := db.Open(ctx, dsn)
	if err != nil {
		log.Fatal().Err(err).Msg("open finspo database")
	}
	defer pool.Close()

	st := store.New(pool)
	sources, err := loadSources(ctx, st, strings.TrimSpace(os.Getenv("FINSPO_BACKFILL_ORG_ID")))
	if err != nil {
		log.Fatal().Err(err).Msg("load sources")
	}

	client := dataplane.NewSourceObjectClient(dataPlaneURL, strings.TrimSpace(os.Getenv("DATA_PLANE_INTERNAL_API_KEY")))
	batchSize := envInt("FINSPO_BACKFILL_BATCH_SIZE", 500)

	var total int
	for _, source := range sources {
		count, err := backfillSource(ctx, st, client, source, batchSize)
		if err != nil {
			log.Fatal().Err(err).
				Str("source_id", source.ID.String()).
				Str("organization_id", source.OrganizationID).
				Msg("backfill source failed")
		}
		total += count
		log.Info().
			Str("source_id", source.ID.String()).
			Str("organization_id", source.OrganizationID).
			Int("items", count).
			Msg("backfilled source objects")
	}

	fmt.Printf("backfilled_source_objects=%d sources=%d\n", total, len(sources))
}

func loadSources(ctx context.Context, st *store.Store, orgID string) ([]store.Source, error) {
	if orgID != "" {
		return st.Sources().ListByOrganization(ctx, orgID)
	}
	return st.Sources().ListEnabled(ctx)
}

func backfillSource(ctx context.Context, st *store.Store, client *dataplane.SourceObjectClient, source store.Source, batchSize int) (int, error) {
	if batchSize <= 0 {
		batchSize = 500
	}

	var total int
	for offset := 0; ; offset += batchSize {
		items, err := st.Items().ListLiveBySource(ctx, source.ID, batchSize, offset)
		if err != nil {
			return total, err
		}
		if len(items) == 0 {
			return total, nil
		}
		for _, item := range items {
			permissions, err := st.Permissions().ListForItem(ctx, item.ID)
			if err != nil {
				return total, fmt.Errorf("list permissions for %s: %w", item.ID, err)
			}
			if err := client.UpsertSourceObjectWithPermissions(ctx, source, item, permissions); err != nil {
				return total, fmt.Errorf("upsert source object %s: %w", item.ItemID, err)
			}
			total++
		}
	}
}

func envInt(key string, fallback int) int {
	raw := strings.TrimSpace(os.Getenv(key))
	if raw == "" {
		return fallback
	}
	value, err := strconv.Atoi(raw)
	if err != nil || value <= 0 {
		return fallback
	}
	return value
}

func envDuration(key string, fallback time.Duration) time.Duration {
	raw := strings.TrimSpace(os.Getenv(key))
	if raw == "" {
		return fallback
	}
	value, err := time.ParseDuration(raw)
	if err != nil || value <= 0 {
		return fallback
	}
	return value
}
