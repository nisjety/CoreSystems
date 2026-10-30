# Module-root test targets. Run them from this directory.

GOTOOLCHAIN ?= local
export GOTOOLCHAIN

# Database packages for test-db. Override with: make test-db PKGS="./internal/db/"
PKGS ?=

.PHONY: test test-db

# Pure tests. The database and Doffin variables are unset, so no test reaches a
# database or Doffin, and -skip '^TestLive' keeps every live test out. The pattern is anchored: a
# plain Live also skips every test whose name merely contains the word ("...ALiveBody"), silently.
test:
	env -u ANBUD_TEST_DSN -u DOFFIN_API_KEY -u ANBUD_REQUIRE_DB_TESTS go test -race -count=1 -skip '^TestLive' ./...

# Database tests against a throwaway container. See scripts/test-db.sh.
test-db:
	./scripts/test-db.sh $(PKGS)
