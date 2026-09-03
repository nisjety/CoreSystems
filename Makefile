.PHONY: help warm-bases check-bases

# Root-level targets that span planes. Per-plane work lives in each plane's own
# Makefile (apps/Data Plane v2/Makefile, apps/Ingestion Plane/Makefile, ...).

help:
	@echo "CoreSystem - root make targets"
	@echo "=============================="
	@echo ""
	@echo "Docker base images:"
	@echo "  make check-bases        Report digest-pinned bases missing locally (read-only)"
	@echo "  make warm-bases         Pull the missing ones so no build stalls on a download"
	@echo ""
	@echo "Per-plane targets live in each plane's Makefile; see CLAUDE.md."

# ── Docker base images ────────────────────────────────────────────────────────

# Every plane pins its Docker base by digest (FROM ...@sha256:...). Docker
# re-pulls a missing digest on demand, so nothing BREAKS without these -- but
# the first build of a plane then stalls on the download, and one broad
# `docker image prune -a` strips all of them at once. On 2026-09-03 a prune did
# exactly that: 8 of the 10 pins vanished and no build would have started
# without ~1.1 GB of downloads first.
#
# The digest list is DERIVED from the Dockerfiles on every run, never hardcoded,
# so bumping a pin cannot leave a stale digest behind here.
#
# -print0/-0 is required: the plane directories contain spaces ("Data Plane v2"),
# which a plain `xargs grep` would split into nonexistent paths.
PIN_PRUNE := -type d \( -name node_modules -o -name .git -o -name target \
	-o -name .claude -o -name dist -o -name .venv -o -name vendor \) -prune -o

SCAN_PINS = find apps $(PIN_PRUNE) -name 'Dockerfile*' -print0 2>/dev/null \
	| xargs -0 grep -hE '^[[:space:]]*FROM[[:space:]]+[^[:space:]]+@sha256:[0-9a-f]{64}' 2>/dev/null \
	| sed -E 's/^[[:space:]]*FROM[[:space:]]+([^[:space:]]+)@(sha256:[0-9a-f]{64}).*/\1@\2/' \
	| sed -E 's/:[^:/@]*@/@/' \
	| sort -u

# Both targets check for Docker first. Without it every ref "fails" one by one,
# burying the real reason under one line of noise per pin.
REQUIRE_DOCKER = command -v docker >/dev/null 2>&1 || \
	{ echo "  docker not available -- skipping"; exit 1; }

check-bases:
	@$(REQUIRE_DOCKER); \
	refs=$$($(SCAN_PINS)); \
	if [ -z "$$refs" ]; then echo "No digest-pinned bases found -- check the scan."; exit 1; fi; \
	total=0; missing=0; \
	for ref in $$refs; do \
		total=$$((total+1)); \
		if docker image inspect "$$ref" >/dev/null 2>&1; then \
			printf "  present  %s\n" "$$ref"; \
		else \
			printf "  MISSING  %s\n" "$$ref"; missing=$$((missing+1)); \
		fi; \
	done; \
	echo "  $$missing of $$total pinned bases missing"; \
	[ "$$missing" -eq 0 ] || { echo "  run 'make warm-bases' to pull them"; exit 1; }

warm-bases:
	@$(REQUIRE_DOCKER); \
	refs=$$($(SCAN_PINS)); \
	if [ -z "$$refs" ]; then echo "No digest-pinned bases found -- check the scan."; exit 1; fi; \
	total=0; pulled=0; failed=0; \
	for ref in $$refs; do \
		total=$$((total+1)); \
		if docker image inspect "$$ref" >/dev/null 2>&1; then continue; fi; \
		printf "  pulling %s ... " "$$ref"; \
		if docker pull --quiet "$$ref" >/dev/null 2>&1; then \
			echo "ok"; pulled=$$((pulled+1)); \
		else \
			echo "FAILED"; failed=$$((failed+1)); \
		fi; \
	done; \
	echo "  $$total pinned bases, $$pulled pulled, $$failed failed"; \
	[ "$$failed" -eq 0 ]
