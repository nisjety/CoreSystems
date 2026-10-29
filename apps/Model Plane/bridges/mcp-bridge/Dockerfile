# syntax=docker/dockerfile:1.7
# Reference MCP bridge. Operators run one per stack; the model-gateway
# proxies via ModelGateway.ProxyMcpTool.
FROM node:22.22-alpine3.24
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
WORKDIR /app
COPY --chown=node:node server.js bridge.example.json ./
# bridge.json is mounted at runtime (operator's choice of upstream
# servers); fall back to the example for the dev image.
RUN cp bridge.example.json bridge.json && chown node:node bridge.json
USER node
EXPOSE 9201
HEALTHCHECK --interval=15s --timeout=5s --start-period=5s --retries=5 \
    CMD wget -q -O /dev/null http://127.0.0.1:9201/health || exit 1
CMD ["node", "server.js"]
