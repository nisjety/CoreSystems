# Reference MCP bridge. Operators run one per stack; the model-gateway
# proxies via ModelGateway.ProxyMcpTool.
FROM node:22-alpine
WORKDIR /app
COPY server.js bridge.example.json ./
# bridge.json is mounted at runtime (operator's choice of upstream
# servers); fall back to the example for the dev image.
RUN cp bridge.example.json bridge.json
EXPOSE 9201
CMD ["node", "server.js"]
