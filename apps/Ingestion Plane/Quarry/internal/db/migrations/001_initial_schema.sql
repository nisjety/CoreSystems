-- +goose Up
CREATE TABLE IF NOT EXISTS quarry_jobs (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_quarry_jobs_status ON quarry_jobs(status);
CREATE INDEX IF NOT EXISTS idx_quarry_jobs_expires_at ON quarry_jobs(expires_at);

-- +goose Down
DROP TABLE IF EXISTS quarry_jobs;
