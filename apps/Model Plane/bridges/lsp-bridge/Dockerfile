# syntax=docker/dockerfile:1.7
# Reference LSP bridge.
#
# Operators run one per workspace; the model-gateway proxies via
# ModelGateway.LspQuery (set LSP_BRIDGE_URL=http://lsp-bridge:9202).
#
# Default image ships TypeScript + Python language servers — the two
# most useful for agent code-editing workflows. Go and Rust servers
# are intentionally NOT bundled: alpine's package set is incomplete
# for both (gopls needs GOPATH plumbing that fights /usr/local/bin
# symlinks, rust-analyzer is missing on aarch64 alpine community),
# and a 700 MB image just to handle every conceivable language is
# the wrong default.
#
# To add Go support:
#   RUN apk add --no-cache go && \
#       GOPATH=/usr/local go install golang.org/x/tools/gopls@latest
#
# To add Rust support (use the debian-slim base instead — alpine
# doesn't ship rust-analyzer on every arch):
#   FROM python:3.12-slim
#   RUN apt-get update && apt-get install -y --no-install-recommends \
#       rust-analyzer && rm -rf /var/lib/apt/lists/*
FROM python:3.12.13-alpine3.24
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
WORKDIR /app

# Each step in its own RUN so a failure surfaces the actual culprit.
# pip-installed deps cache poorly when chained behind apk in alpine.
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add nodejs npm
RUN --mount=type=cache,target=/root/.cache/pip \
    pip install --no-cache-dir 'python-lsp-server[all]'
RUN --mount=type=cache,target=/root/.npm \
    npm install -g typescript typescript-language-server

RUN addgroup -S -g 10001 modelplane \
    && adduser -S -D -h /home/modelplane -u 10001 -G modelplane modelplane
COPY --chown=modelplane:modelplane server.py ./
USER modelplane
EXPOSE 9202
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
    CMD python3 -c "import urllib.request; urllib.request.urlopen('http://127.0.0.1:9202/health', timeout=3).read()" || exit 1
CMD ["python3", "server.py"]
