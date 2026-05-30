#!/usr/bin/env python3
"""
RAG Integration End-to-End Test Suite

This script demonstrates the complete RAG pipeline:
1. Ingest documents into Data Plane
2. Query retrieval service (vector search + reranking)
3. Verify multi-tenant isolation
4. Test integration with AI-Core (optional)

Run: python3 rag_integration_test.py
"""

import asyncio
import json
import sys
import time
from typing import Any, Dict, List

import httpx

# Configuration
DATA_PLANE_BASE = "http://localhost:8004"
DOCUMENTS_BASE = "http://localhost:8001"
AI_CORE_BASE = "http://localhost:8100"
TIMEOUT = httpx.Timeout(timeout=30.0, connect=5.0)


class RAGIntegrationTester:
    """End-to-end test suite for RAG system."""

    def __init__(self):
        self.client = httpx.AsyncClient(timeout=TIMEOUT)
        self.results = []

    async def cleanup(self):
        await self.client.aclose()

    # ── Retrieval Tests ────────────────────────────────────────────────────

    async def test_vector_search_plus_reranking(self, org_id: str = "integration-test"):
        """Test that vector search + Cohere reranking works end-to-end."""
        print("\n[TEST 1] Vector Search + Cohere Reranking")
        print("=" * 60)

        query = "How do embeddings improve semantic understanding?"
        
        try:
            response = await self.client.post(
                f"{DATA_PLANE_BASE}/v1/retrieve",
                json={
                    "org_id": org_id,
                    "query": query,
                    "top_k": 5,
                },
            )
            response.raise_for_status()
            result = response.json()
            
            facts = result.get("facts", [])
            sources = result.get("sources", [])
            
            print(f"✅ Retrieved {len(facts)} facts from {len(sources)} source(s)")
            print()
            
            for i, fact in enumerate(facts, 1):
                vs_score = fact.get("score", 0)
                rerank_score = fact.get("rerank_score")
                has_rerank = "✅" if rerank_score is not None else "⚠️"
                
                print(f"[{i}] {has_rerank} Vector: {vs_score:.4f} | Rerank: {rerank_score}")
                text_preview = fact.get("text", "")[:70]
                print(f"     {text_preview}...")
                print()
            
            # Assertion: all facts should have rerank scores
            facts_with_scores = [f for f in facts if f.get("rerank_score") is not None]
            assert len(facts_with_scores) == len(facts), \
                f"Expected all {len(facts)} facts to have rerank scores, got {len(facts_with_scores)}"
            
            print(f"✅ PASS: All facts have rerank scores")
            self.results.append(("Vector Search + Reranking", True, None))
            
        except Exception as e:
            print(f"❌ FAIL: {e}")
            self.results.append(("Vector Search + Reranking", False, str(e)))

    async def test_multi_tenant_isolation(self):
        """Test that org_id hard filter prevents cross-tenant retrieval."""
        print("\n[TEST 2] Multi-Tenant Isolation")
        print("=" * 60)

        try:
            # Query with non-existent org_id
            response = await self.client.post(
                f"{DATA_PLANE_BASE}/v1/retrieve",
                json={
                    "org_id": "non-existent-org-12345",
                    "query": "vector search",
                    "top_k": 10,
                },
            )
            response.raise_for_status()
            result = response.json()
            
            facts = result.get("facts", [])
            print(f"Facts for non-existent org: {len(facts)} (expected: 0)")
            
            assert len(facts) == 0, \
                f"Expected 0 facts for non-existent org, got {len(facts)}"
            
            print(f"✅ PASS: Multi-tenant isolation working - no cross-tenant leakage")
            self.results.append(("Multi-Tenant Isolation", True, None))
            
        except Exception as e:
            print(f"❌ FAIL: {e}")
            self.results.append(("Multi-Tenant Isolation", False, str(e)))

    async def test_filtered_retrieval(self):
        """Test that metadata filters work correctly."""
        print("\n[TEST 3] Filtered Retrieval (Type Filter)")
        print("=" * 60)

        try:
            # Query with document type filter
            response = await self.client.post(
                f"{DATA_PLANE_BASE}/v1/retrieve",
                json={
                    "org_id": "rag-demo",
                    "query": "retrieval augmented generation",
                    "top_k": 5,
                    "filters": {
                        "document_types": ["article"],
                    }
                },
            )
            response.raise_for_status()
            result = response.json()
            
            facts = result.get("facts", [])
            all_articles = all(
                f.get("metadata", {}).get("type") == "article"
                for f in facts
            )
            
            print(f"✅ Retrieved {len(facts)} articles (filtered)")
            print(f"   All results are type='article': {all_articles}")
            
            assert all_articles or len(facts) == 0, \
                "Filter not applied correctly"
            
            print(f"✅ PASS: Document type filtering working")
            self.results.append(("Filtered Retrieval", True, None))
            
        except Exception as e:
            print(f"❌ FAIL: {e}")
            self.results.append(("Filtered Retrieval", False, str(e)))

    async def test_response_format(self):
        """Test that response format matches spec."""
        print("\n[TEST 4] Response Format Validation")
        print("=" * 60)

        try:
            response = await self.client.post(
                f"{DATA_PLANE_BASE}/v1/retrieve",
                json={
                    "org_id": "rag-demo",
                    "query": "vector search",
                    "top_k": 3,
                },
            )
            response.raise_for_status()
            result = response.json()
            
            # Check required fields
            required_top_level = {"facts", "sources", "query", "org_id"}
            missing = required_top_level - set(result.keys())
            assert not missing, f"Missing top-level fields: {missing}"
            
            # Check fact structure
            for fact in result.get("facts", []):
                required_fact = {"knowledge_id", "document_id", "text", "score", "metadata"}
                missing = required_fact - set(fact.keys())
                assert not missing, f"Missing fact fields: {missing}"
                
                # rerank_score is optional but should be present
                assert "rerank_score" in fact, "rerank_score missing from fact"
            
            # Check source structure
            for source in result.get("sources", []):
                required_source = {"document_id", "title", "source", "type"}
                missing = required_source - set(source.keys())
                assert not missing, f"Missing source fields: {missing}"
            
            print(f"✅ Response format valid")
            print(f"   - Top-level: {list(result.keys())}")
            print(f"   - Facts: {len(result['facts'])} (structure ✓)")
            print(f"   - Sources: {len(result['sources'])} (structure ✓)")
            
            print(f"✅ PASS: Response format matches spec")
            self.results.append(("Response Format", True, None))
            
        except Exception as e:
            print(f"❌ FAIL: {e}")
            self.results.append(("Response Format", False, str(e)))

    async def test_service_health(self):
        """Test that all required services are healthy."""
        print("\n[TEST 5] Service Health")
        print("=" * 60)

        services = {
            "Data Plane (Retrieval)": f"{DATA_PLANE_BASE}/health",
            "Data Plane (Documents)": f"{DOCUMENTS_BASE}/health",
            "AI-Core": f"{AI_CORE_BASE}/health",
        }
        
        all_healthy = True
        for name, url in services.items():
            try:
                response = await self.client.get(url, timeout=5.0)
                if response.status_code == 200:
                    print(f"✅ {name}: OK")
                else:
                    print(f"⚠️  {name}: HTTP {response.status_code}")
                    all_healthy = False
            except Exception as e:
                print(f"❌ {name}: {e}")
                all_healthy = False
        
        if all_healthy:
            print(f"\n✅ PASS: All services healthy")
            self.results.append(("Service Health", True, None))
        else:
            print(f"\n⚠️  WARN: Some services unavailable")
            self.results.append(("Service Health", False, "Some services unavailable"))

    # ── Reporting ──────────────────────────────────────────────────────────

    def print_summary(self):
        """Print test summary."""
        print("\n" + "=" * 60)
        print("TEST SUMMARY")
        print("=" * 60)
        
        passed = sum(1 for _, success, _ in self.results if success)
        failed = sum(1 for _, success, _ in self.results if not success)
        total = len(self.results)
        
        for test_name, success, error in self.results:
            status = "✅ PASS" if success else "❌ FAIL"
            print(f"{status:10} {test_name}")
            if error:
                print(f"           Error: {error}")
        
        print()
        print(f"Results: {passed}/{total} passed")
        
        if failed == 0:
            print("\n🎉 All tests passed! RAG integration is working.")
            return True
        else:
            print(f"\n⚠️  {failed} test(s) failed. Check configuration.")
            return False


async def main():
    """Run all integration tests."""
    print("\n" + "🚀 RAG Integration Test Suite".center(60, "="))
    print("Data Plane Retrieval + Cohere Reranking + AI-Core".center(60))
    print("=" * 60)
    
    tester = RAGIntegrationTester()
    
    try:
        # Run tests in order
        await tester.test_service_health()
        await asyncio.sleep(0.5)
        
        await tester.test_vector_search_plus_reranking()
        await asyncio.sleep(0.5)
        
        await tester.test_multi_tenant_isolation()
        await asyncio.sleep(0.5)
        
        await tester.test_filtered_retrieval()
        await asyncio.sleep(0.5)
        
        await tester.test_response_format()
        
        # Print summary and exit
        success = tester.print_summary()
        sys.exit(0 if success else 1)
        
    except KeyboardInterrupt:
        print("\n\n⚠️  Test interrupted by user")
        sys.exit(1)
    finally:
        await tester.cleanup()


if __name__ == "__main__":
    asyncio.run(main())
