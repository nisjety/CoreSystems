#!/bin/bash

# Lightweight performance test for CI / local checks
set -e

BASE_URL="${QUARRY_BASE:-http://localhost:9090}"
API_KEY="${QUARRY_API_KEY:-dev-test-key-12345}"
DURATION="${DURATION:-2}"
CONCURRENCY="${CONCURRENCY:-20}"
LOAD_REQUESTS="${LOAD_REQUESTS:-20}"

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BOLD}=== QUARRY LIGHTWEIGHT PERFORMANCE (CI) ===${NC}\n"

# 1. Latency Test (20)
COUNT=20
SUM=0
MIN=999999
MAX=0
for i in $(seq 1 $COUNT); do
  START=$(date +%s%3N)
  curl -s -H "X-API-Key: $API_KEY" "$BASE_URL/health" > /dev/null || true
  END=$(date +%s%3N)
  LAT=$((END-START))
  SUM=$((SUM+LAT))
  if [ $LAT -lt $MIN ]; then MIN=$LAT; fi
  if [ $LAT -gt $MAX ]; then MAX=$LAT; fi
done
AVG=$((SUM / COUNT))

echo -e "Average Latency: ${AVG}ms"
echo -e "Min Latency: ${MIN}ms"
echo -e "Max Latency: ${MAX}ms"" 

# 2. Short Throughput
echo -e "\n${BOLD}Throughput Test (${DURATION}s, ${CONCURRENCY} parallel)${NC}"
END_TIME=$(( $(date +%s) + DURATION ))
COUNT=0
while [ $(date +%s) -lt $END_TIME ]; do
  for i in $(seq 1 $CONCURRENCY); do
    curl -s -H "X-API-Key: $API_KEY" "$BASE_URL/health" > /dev/null &
    COUNT=$((COUNT+1))
  done
  wait
done
THROUGHPUT=$(echo "scale=2; $COUNT / $DURATION" | bc)

echo -e "Throughput: ${THROUGHPUT} req/sec"

# 3. Small Load Test
echo -e "\n${BOLD}Small Load Test (${LOAD_REQUESTS} concurrent /v1/scrape)${NC}"
for i in $(seq 1 $LOAD_REQUESTS); do
  curl -s -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
    -d '{"url":"https://example.com","collection":"quick"}' \
    "$BASE_URL/v1/scrape" > /dev/null &
done
wait
sleep 1

echo -e "\n${GREEN}CI performance test completed${NC}"
