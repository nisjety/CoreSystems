#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import json
import math
import sys
import time
from dataclasses import asdict, dataclass
from typing import Any

import httpx


TIMEOUT = httpx.Timeout(timeout=30.0, connect=5.0)

TOPIC_FIXTURES = [
    (
        "vacation-policy",
        "How many vacation days can employees carry over?",
        "Employees receive 25 vacation days per year. They may carry over up to 5 unused days. Vacation requests must be submitted two weeks in advance.",
    ),
    (
        "incident-response",
        "What is the target time to acknowledge a severity one incident?",
        "Severity one incidents must be acknowledged within 15 minutes. The incident commander owns communication updates every 30 minutes until mitigation is complete.",
    ),
    (
        "security-access",
        "How often must privileged access be reviewed?",
        "Privileged access rights are reviewed every 90 days. Break-glass access requires explicit approval and must be logged for audit.",
    ),
    (
        "expense-policy",
        "What is the hotel limit for travel reimbursement?",
        "Hotel reimbursement is capped at 220 dollars per night unless pre-approved by finance. Receipts are required for all lodging expenses.",
    ),
    (
        "customer-support",
        "When should premium support tickets receive a first response?",
        "Premium support tickets require a first human response within one hour. Standard tickets should receive a response within one business day.",
    ),
]


@dataclass
class BenchmarkConfig:
    documents_base: str
    retrieval_base: str
    org_id: str
    documents: int
    queries: int
    ingest_concurrency: int
    query_concurrency: int
    poll_interval: float
    max_wait_seconds: float
    top_k: int
    top_n: int
    cleanup: bool
    output_json: str | None


@dataclass
class IngestResult:
    document_id: str
    topic: str
    query: str
    ingest_seconds: float
    indexed_seconds: float | None
    final_status: str
    error: str | None


@dataclass
class RetrievalResult:
    query: str
    latency_seconds: float
    fact_count: int
    error: str | None


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    if len(values) == 1:
        return values[0]
    ordered = sorted(values)
    index = (len(ordered) - 1) * pct
    lower = math.floor(index)
    upper = math.ceil(index)
    if lower == upper:
        return ordered[int(index)]
    lower_value = ordered[lower]
    upper_value = ordered[upper]
    return lower_value + (upper_value - lower_value) * (index - lower)


class DataPlaneBenchmark:
    def __init__(self, config: BenchmarkConfig):
        self.config = config
        self.client = httpx.AsyncClient(timeout=TIMEOUT)

    async def close(self) -> None:
        await self.client.aclose()

    async def ensure_ready(self) -> None:
        checks = {
            "documents": f"{self.config.documents_base}/readyz",
            "retrieval": f"{self.config.retrieval_base}/readyz",
        }
        for name, url in checks.items():
            response = await self.client.get(url)
            response.raise_for_status()
            payload = response.json()
            if payload.get("status") != "ready":
                raise RuntimeError(f"{name} is not ready: {payload}")

    def build_document_payload(self, index: int) -> dict[str, Any]:
        topic, _, body = TOPIC_FIXTURES[index % len(TOPIC_FIXTURES)]
        repeated_body = "\n\n".join([body] * 8)
        return {
            "org_id": self.config.org_id,
            "source": "benchmark",
            "type": "benchmark",
            "title": f"Benchmark Doc {index:03d} - {topic}",
            "content": repeated_body,
            "metadata": {
                "topic": topic,
                "suite": "data-plane-benchmark",
                "document_index": index,
            },
        }

    async def ingest_one(self, index: int, semaphore: asyncio.Semaphore) -> IngestResult:
        payload = self.build_document_payload(index)
        topic, query, _ = TOPIC_FIXTURES[index % len(TOPIC_FIXTURES)]
        async with semaphore:
            started_at = time.perf_counter()
            response = await self.client.post(
                f"{self.config.documents_base}/v1/documents",
                json=payload,
            )
            response.raise_for_status()
            document = response.json()
            ingest_seconds = time.perf_counter() - started_at

        document_id = document["document_id"]
        indexed_seconds, final_status, error = await self.wait_for_document(document_id)
        return IngestResult(
            document_id=document_id,
            topic=topic,
            query=query,
            ingest_seconds=ingest_seconds,
            indexed_seconds=indexed_seconds,
            final_status=final_status,
            error=error,
        )

    async def wait_for_document(self, document_id: str) -> tuple[float | None, str, str | None]:
        started_at = time.perf_counter()
        deadline = started_at + self.config.max_wait_seconds
        detail_url = f"{self.config.documents_base}/v1/documents/{document_id}"

        while time.perf_counter() < deadline:
            response = await self.client.get(
                detail_url,
                params={"org_id": self.config.org_id},
            )
            response.raise_for_status()
            payload = response.json()
            status_value = payload.get("status", "unknown")
            if status_value == "indexed":
                return time.perf_counter() - started_at, status_value, None
            if status_value == "failed":
                return time.perf_counter() - started_at, status_value, payload.get("error_message")
            await asyncio.sleep(self.config.poll_interval)

        return None, "timeout", "document did not reach indexed status before timeout"

    async def run_ingest_phase(self) -> list[IngestResult]:
        semaphore = asyncio.Semaphore(self.config.ingest_concurrency)
        tasks = [self.ingest_one(index, semaphore) for index in range(self.config.documents)]
        return await asyncio.gather(*tasks)

    async def retrieve_one(self, query: str, semaphore: asyncio.Semaphore) -> RetrievalResult:
        async with semaphore:
            started_at = time.perf_counter()
            try:
                response = await self.client.post(
                    f"{self.config.retrieval_base}/v1/retrieve",
                    json={
                        "org_id": self.config.org_id,
                        "query": query,
                        "top_k": self.config.top_k,
                        "top_n": self.config.top_n,
                    },
                )
                response.raise_for_status()
                payload = response.json()
                return RetrievalResult(
                    query=query,
                    latency_seconds=time.perf_counter() - started_at,
                    fact_count=len(payload.get("facts", [])),
                    error=None,
                )
            except Exception as exc:
                return RetrievalResult(
                    query=query,
                    latency_seconds=time.perf_counter() - started_at,
                    fact_count=0,
                    error=str(exc),
                )

    async def run_retrieval_phase(self, ingest_results: list[IngestResult]) -> list[RetrievalResult]:
        indexed_results = [result for result in ingest_results if result.final_status == "indexed"]
        if not indexed_results:
            raise RuntimeError("no indexed documents available for retrieval benchmark")

        semaphore = asyncio.Semaphore(self.config.query_concurrency)
        query_pool = [result.query for result in indexed_results]
        tasks = [
            self.retrieve_one(query_pool[index % len(query_pool)], semaphore)
            for index in range(self.config.queries)
        ]
        return await asyncio.gather(*tasks)

    async def cleanup_documents(self, ingest_results: list[IngestResult]) -> None:
        if not self.config.cleanup:
            return

        async def _delete(document_id: str) -> None:
            response = await self.client.delete(
                f"{self.config.documents_base}/v1/documents/{document_id}",
                params={"org_id": self.config.org_id},
            )
            if response.status_code not in {200, 404}:
                response.raise_for_status()

        await asyncio.gather(*[_delete(result.document_id) for result in ingest_results])

    def build_summary(
        self,
        ingest_results: list[IngestResult],
        retrieval_results: list[RetrievalResult],
        total_seconds: float,
    ) -> dict[str, Any]:
        ingest_latencies = [result.ingest_seconds for result in ingest_results]
        indexed_latencies = [
            result.indexed_seconds for result in ingest_results if result.indexed_seconds is not None
        ]
        retrieval_latencies = [result.latency_seconds for result in retrieval_results]
        retrieval_failures = [result for result in retrieval_results if result.error]
        ingest_failures = [
            result for result in ingest_results if result.final_status not in {"indexed", "timeout"}
        ]

        return {
            "config": asdict(self.config),
            "totals": {
                "documents": len(ingest_results),
                "documents_indexed": sum(1 for result in ingest_results if result.final_status == "indexed"),
                "queries": len(retrieval_results),
                "query_failures": len(retrieval_failures),
                "wall_clock_seconds": round(total_seconds, 3),
            },
            "ingest": {
                "p50_seconds": round(percentile(ingest_latencies, 0.50), 4),
                "p95_seconds": round(percentile(ingest_latencies, 0.95), 4),
                "p99_seconds": round(percentile(ingest_latencies, 0.99), 4),
                "failures": [asdict(result) for result in ingest_failures],
            },
            "indexing": {
                "p50_seconds": round(percentile(indexed_latencies, 0.50), 4),
                "p95_seconds": round(percentile(indexed_latencies, 0.95), 4),
                "p99_seconds": round(percentile(indexed_latencies, 0.99), 4),
                "timeouts": [
                    asdict(result) for result in ingest_results if result.final_status == "timeout"
                ],
            },
            "retrieval": {
                "p50_seconds": round(percentile(retrieval_latencies, 0.50), 4),
                "p95_seconds": round(percentile(retrieval_latencies, 0.95), 4),
                "p99_seconds": round(percentile(retrieval_latencies, 0.99), 4),
                "avg_fact_count": round(
                    sum(result.fact_count for result in retrieval_results) / max(len(retrieval_results), 1),
                    2,
                ),
                "failures": [asdict(result) for result in retrieval_failures],
            },
        }

    def print_summary(self, summary: dict[str, Any]) -> None:
        print("\nData Plane Benchmark")
        print("=" * 60)
        print(json.dumps(summary["totals"], indent=2))
        print("\nIngest Latency")
        print(json.dumps(summary["ingest"], indent=2))
        print("\nIndexing Latency")
        print(json.dumps(summary["indexing"], indent=2))
        print("\nRetrieval Latency")
        print(json.dumps(summary["retrieval"], indent=2))


def parse_args() -> BenchmarkConfig:
    parser = argparse.ArgumentParser(description="Benchmark the Data Plane ingest/index/retrieval path.")
    parser.add_argument("--documents-base", default="http://localhost:8001")
    parser.add_argument("--retrieval-base", default="http://localhost:8004")
    parser.add_argument("--org-id", default="benchmark-org")
    parser.add_argument("--documents", type=int, default=20)
    parser.add_argument("--queries", type=int, default=40)
    parser.add_argument("--ingest-concurrency", type=int, default=5)
    parser.add_argument("--query-concurrency", type=int, default=10)
    parser.add_argument("--poll-interval", type=float, default=1.0)
    parser.add_argument("--max-wait-seconds", type=float, default=180.0)
    parser.add_argument("--top-k", type=int, default=10)
    parser.add_argument("--top-n", type=int, default=5)
    parser.add_argument("--no-cleanup", action="store_true")
    parser.add_argument("--output-json")
    args = parser.parse_args()
    return BenchmarkConfig(
        documents_base=args.documents_base.rstrip("/"),
        retrieval_base=args.retrieval_base.rstrip("/"),
        org_id=args.org_id,
        documents=args.documents,
        queries=args.queries,
        ingest_concurrency=args.ingest_concurrency,
        query_concurrency=args.query_concurrency,
        poll_interval=args.poll_interval,
        max_wait_seconds=args.max_wait_seconds,
        top_k=args.top_k,
        top_n=args.top_n,
        cleanup=not args.no_cleanup,
        output_json=args.output_json,
    )


async def main() -> int:
    config = parse_args()
    benchmark = DataPlaneBenchmark(config)
    started_at = time.perf_counter()

    try:
        await benchmark.ensure_ready()
        ingest_results = await benchmark.run_ingest_phase()
        retrieval_results = await benchmark.run_retrieval_phase(ingest_results)
        total_seconds = time.perf_counter() - started_at
        summary = benchmark.build_summary(ingest_results, retrieval_results, total_seconds)
        benchmark.print_summary(summary)
        if config.output_json:
            with open(config.output_json, "w", encoding="utf-8") as output_file:
                json.dump(summary, output_file, indent=2)
        return 0
    except Exception as exc:
        print(f"Benchmark failed: {exc}", file=sys.stderr)
        return 1
    finally:
        try:
            if "ingest_results" in locals():
                await benchmark.cleanup_documents(ingest_results)
        finally:
            await benchmark.close()


if __name__ == "__main__":
    raise SystemExit(asyncio.run(main()))