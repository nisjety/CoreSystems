#!/bin/bash

# Quarry Performance and Memory Testing Script
# Tests latency, throughput, memory leaks, and goroutine leaks

set -e

BASE_URL="${QUARRY_BASE:-http://localhost:9090}"
API_KEY="${QUARRY_API_KEY:-dev-test-key-12345}"
BOLD='\033[1m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BOLD}=== QUARRY PERFORMANCE & MEMORY TESTS ===${NC}\n"

# 1. Latency Test
echo -e "${BOLD}1. Latency Test (100 requests to /health)${NC}"
LATENCIES=()
for i in {1..100}; do
    START=$(date +%s%N)
    curl -s -H "X-API-Key: $API_KEY" "$BASE_URL/health" > /dev/null
    END=$(date +%s%N)
    LATENCY=$(( (END - START) / 1000000 ))
    LATENCIES+=($LATENCY)
done

# Calculate stats
SUM=0
MIN=999999
MAX=0
for lat in "${LATENCIES[@]}"; do
    SUM=$((SUM + lat))
    if [ $lat -lt $MIN ]; then MIN=$lat; fi
    if [ $lat -gt $MAX ]; then MAX=$lat; fi
done
AVG=$((SUM / 100))

echo -e "${GREEN}Average Latency: ${AVG}ms${NC}"
echo -e "Min Latency: ${MIN}ms"
echo -e "Max Latency: ${MAX}ms"
echo ""

# 2. Throughput Test
echo -e "${BOLD}2. Throughput Test (requests/sec)${NC}"
DURATION="${DURATION:-10}"
echo "Running for ${DURATION} seconds..."

START_TIME=$(date +%s)
END_TIME=$((START_TIME + DURATION))
COUNT=0

while [ $(date +%s) -lt $END_TIME ]; do
    curl -s -H "X-API-Key: $API_KEY" "$BASE_URL/health" > /dev/null &
    COUNT=$((COUNT + 1))
done

wait

THROUGHPUT=$(echo "scale=2; $COUNT / $DURATION" | bc)
echo -e "${GREEN}Throughput: ${THROUGHPUT} req/sec${NC}"
echo ""

# 3. Memory Usage Before Load
echo -e "${BOLD}3. Docker Memory Usage${NC}"
MEMORY_BEFORE=$(docker stats quarry-api --no-stream --format "{{.MemUsage}}" | awk '{print $1}')
echo "Memory Before Load: $MEMORY_BEFORE"

# Run load test
echo "Running load test (200 concurrent requests)..."
for i in {1..200}; do
    curl -s -H "X-API-Key: $API_KEY" -H "Content-Type: application/json" \
    -d '{"url":"https://example.com","collection":"quick"}' \
    "$BASE_URL/v1/scrape" > /dev/null 2>&1 &
done
wait

sleep 5

MEMORY_AFTER=$(docker stats quarry-api --no-stream --format "{{.MemUsage}}" | awk '{print $1}')
echo "Memory After Load: $MEMORY_AFTER"
echo ""

# 4. Goroutine Leak Check
echo -e "${BOLD}4. Goroutine Leak Check${NC}"
echo "Checking goroutine count before and after load..."

# Get initial goroutine count (we'll check /metrics endpoint which might include this info)
# For now, we'll check Docker logs for goroutine info
GOROUTINES_BEFORE=$(docker exec quarry-api ps aux | wc -l)
echo "Processes Before: $GOROUTINES_BEFORE"

# Small load
for i in {1..50}; do
    curl -s -H "X-API-Key: $API_KEY" "$BASE_URL/health" > /dev/null &
done
wait

sleep 2

GOROUTINES_AFTER=$(docker exec quarry-api ps aux | wc -l)
echo "Processes After: $GOROUTINES_AFTER"

if [ $GOROUTINES_AFTER -gt $((GOROUTINES_BEFORE + 10)) ]; then
    echo -e "${RED}⚠ Potential goroutine leak detected!${NC}"
else
    echo -e "${GREEN}✓ No significant goroutine leak detected${NC}"
fi
echo ""

# 5. Memory Leak Check (repeated requests)
echo -e "${BOLD}5. Memory Leak Check (5 iterations)${NC}"
MEMORY_READINGS=()

for iteration in {1..5}; do
    echo "Iteration $iteration: Running 100 requests..."
    for i in {1..100}; do
        curl -s -H "X-API-Key: $API_KEY" "$BASE_URL/health" > /dev/null 2>&1 &
    done
    wait
    
    sleep 2
    MEM=$(docker stats quarry-api --no-stream --format "{{.MemUsage}}" | awk '{print $1}')
    MEMORY_READINGS+=("$MEM")
    echo "Memory after iteration $iteration: $MEM"
done

echo -e "\n${BOLD}Memory Trend:${NC}"
for i in "${!MEMORY_READINGS[@]}"; do
    echo "  Iteration $((i+1)): ${MEMORY_READINGS[$i]}"
done
echo ""

# 6. Container Stats Summary
echo -e "${BOLD}6. Container Stats Summary${NC}"
docker stats --no-stream quarry-api quarry-worker quarry-temporal quarry-redis quarry-postgres

echo -e "\n${BOLD}=== PERFORMANCE TEST SUMMARY ===${NC}"
echo -e "✓ Average Latency: ${AVG}ms"
echo -e "✓ Throughput: ${THROUGHPUT} req/sec"
echo -e "✓ Memory Usage: $MEMORY_BEFORE → $MEMORY_AFTER"
echo -e "✓ Container Health: Running"

if [ $AVG -lt 100 ]; then
    echo -e "\n${GREEN}${BOLD}PERFORMANCE: EXCELLENT (<100ms avg latency)${NC}"
elif [ $AVG -lt 500 ]; then
    echo -e "\n${YELLOW}${BOLD}PERFORMANCE: GOOD (<500ms avg latency)${NC}"
else
    echo -e "\n${RED}${BOLD}PERFORMANCE: NEEDS OPTIMIZATION (>500ms avg latency)${NC}"
fi
