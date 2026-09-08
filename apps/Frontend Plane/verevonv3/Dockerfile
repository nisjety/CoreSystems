# syntax=docker/dockerfile:1.7
# Development image with hot reload. Keep pnpm pinned so the lockfile is
# interpreted consistently across developer machines and CI builders.
FROM node:22-alpine@sha256:16e22a550f3863206a3f701448c45f7912c6896a62de43add43bb9c86130c3e2 AS dev
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"
WORKDIR /app

RUN apk add --no-cache curl \
    && corepack enable \
    && corepack prepare pnpm@11.3.0 --activate \
    && chown node:node /app

USER node
# pnpm-workspace.yaml carries the `overrides` (security pins) that pnpm v11 reads
# instead of the package.json "pnpm" field; without it `--frozen-lockfile` fails
# with ERR_PNPM_LOCKFILE_CONFIG_MISMATCH against the lockfile's recorded overrides.
COPY --chown=node:node package.json pnpm-lock.yaml pnpm-workspace.yaml ./
# The browser uses the generated Quarry client for typed source/browser
# contracts.  It is intentionally kept in the Ingestion Plane and is supplied
# as a BuildKit named context by compose/CI; copying it to the path encoded by
# the local `file:` dependency keeps the frontend context small without
# publishing a second, drifting SDK package.
# `--chown` matters here, and its absence is not cosmetic: `@quarry/client` is a
# `file:` dependency, and pnpm imports those by HARDLINKING the package's files
# into the virtual store. This stage runs as `node`, so without the chown the
# SDK lands root-owned and every hardlink fails with `Operation not permitted`
# (os error 1) once the kernel has `fs.protected_hardlinks=1` — the default in
# Docker Desktop's VM, which is why this reproduces there and not in every CI.
# The error names a different file on each run, so it reads like a flake rather
# than a permission problem. Prefer this over `package-import-method=copy`,
# which would hide the cause and copy the whole SDK on every install.
COPY --from=quarry-client --chown=node:node [".", "/Ingestion Plane/Quarry-v2/sdks/typescript/"]
RUN --mount=type=cache,target=/home/node/.local/share/pnpm/store,uid=1000,gid=1000 \
    pnpm install --frozen-lockfile

# Copy source code (will be overridden by volume mount in dev)
COPY --chown=node:node . .

# Expose Vite dev server port
EXPOSE 5173

# Run Vite dev server with HMR enabled
CMD ["pnpm", "dev", "--host", "0.0.0.0"]

FROM dev AS build
# `COPY --chown=node:node . .` above is what first reveals packages/* and
# apps/remote-dev's own package.json files -- `pnpm install --frozen-lockfile`
# earlier in the `dev` stage only had the root manifest on disk. pnpm's
# dependency-status check runs before any `pnpm run <script>` and, seeing the
# now-complete workspace, decides node_modules needs to be purged and
# reinstalled to reconcile. That decision needs a confirmation prompt, and a
# Docker RUN step never has a TTY to answer it. CI=true is pnpm's own
# documented way to say this is a non-interactive environment (which a Docker
# build genuinely is), so it proceeds instead of aborting.
ENV CI=true
# @verevon/remote-core (packages/remote-core) is a `workspace:*` dep that
# RemoteSupportPage.tsx imports. `pnpm install` only links the workspace
# package -- it does not run remote-core's own `build` script -- and its
# package.json `main`/`types`/`exports` all point at dist/, which is
# gitignored. Without this step `pnpm build`'s typecheck fails with "Cannot
# find module '@verevon/remote-core'" instead of a legible build error.
# Mirrors the same fix in the `spa-verify` CI job's workflow step.
RUN pnpm --filter @verevon/remote-core run build
RUN pnpm build

# Static production image. nginx-unprivileged runs as UID 101 and listens on a
# non-privileged port; the digest makes weekly upstream rebuilds explicit.
FROM nginxinc/nginx-unprivileged:1.30.0-alpine3.23@sha256:808f7846d21a9c94cf53833e8807a00a33fd0b65cc47fb05b79efe366c2d201f AS production
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build --chown=101:101 /app/dist/ /usr/share/nginx/html/

EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD wget -qO- http://127.0.0.1:8080/health >/dev/null || exit 1
