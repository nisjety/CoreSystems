package store

// WithPruneBatchSizeForTest shrinks the purge batch so a test can cross batch
// boundaries without inserting thousands of rows.
func WithPruneBatchSizeForTest(n int) Option {
	return func(s *Store) { s.pruneBatchOverride = n }
}
