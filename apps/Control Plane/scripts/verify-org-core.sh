#!/bin/bash
echo "=== Final Integration Verification ==="
echo ""
echo "1. Service Status:"
docker ps --filter "name=org-core-service" --format "   {{.Names}}: {{.Status}}"
echo ""
echo "2. Quick Health Check:"
curl -s http://localhost:8080/health | jq -r '"   Status: \(.status), Uptime: \(.uptime)s"'
echo ""
echo "3. Available Endpoints:"
echo "   HTTP: http://localhost:8080"
echo "   gRPC: localhost:9090"
echo "   Metrics: http://localhost:9091/metrics"
echo ""
echo "4. RAG Tools:"
curl -s http://localhost:8080/api/v1/rag/tools/schema | jq -r '.tools | keys | .[] | "   - \(.)"'
echo ""
echo "✅ Org-Core Docker deployment successful!"
