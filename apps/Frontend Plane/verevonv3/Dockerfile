# syntax=docker/dockerfile:1.7
# Development image with hot reload. Keep pnpm pinned so the lockfile is
# interpreted consistently across developer machines and CI builders.
FROM node:22-alpine@sha256:16e22a550f3863206a3f701448c45f7912c6896a62de43add43bb9c86130c3e2 AS dev
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"
WORKDIR /app
ENV COREPACK_ENABLE_PROJECT_SPEC=0 \
    COREPACK_DEFAULT_TO_LATEST=0 \
    COREPACK_HOME=/usr/local/share/corepack

RUN apk add --no-cache curl \
    && corepack enable \
    && corepack prepare pnpm@11.3.0 --activate \
    && chown node:node /app

USER node
# The SDK is a local file dependency outside the SPA build context. Copy only
# its source/manifests through the named context, never its host node_modules.
COPY --from=quarry-sdk --chown=node:node ["package.json", "tsconfig.json", "tsconfig.esm.json", "/Ingestion Plane/Quarry-v2/sdks/typescript/"]
COPY --from=quarry-sdk --chown=node:node ["src", "/Ingestion Plane/Quarry-v2/sdks/typescript/src/"]
# pnpm-workspace.yaml carries the `overrides` (security pins) that pnpm v11 reads
# instead of the package.json "pnpm" field; without it `--frozen-lockfile` fails
# with ERR_PNPM_LOCKFILE_CONFIG_MISMATCH against the lockfile's recorded overrides.
COPY --chown=node:node package.json pnpm-lock.yaml pnpm-workspace.yaml ./
RUN --mount=type=cache,target=/home/node/.local/share/pnpm/store,uid=1000,gid=1000 \
    pnpm install --frozen-lockfile \
    && pnpm exec tsc --ignoreDeprecations 6.0 --rootDir '/Ingestion Plane/Quarry-v2/sdks/typescript/src' -p '/Ingestion Plane/Quarry-v2/sdks/typescript/tsconfig.json' \
    && pnpm exec tsc --ignoreDeprecations 6.0 --rootDir '/Ingestion Plane/Quarry-v2/sdks/typescript/src' -p '/Ingestion Plane/Quarry-v2/sdks/typescript/tsconfig.esm.json' \
    && cp -R '/Ingestion Plane/Quarry-v2/sdks/typescript/dist' node_modules/@quarry/client/dist

# Copy source code (will be overridden by volume mount in dev)
COPY --chown=node:node . .

# Expose Vite dev server port
EXPOSE 5173

# Run Vite dev server with HMR enabled
CMD ["pnpm", "dev", "--host", "0.0.0.0"]

FROM dev AS build
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
