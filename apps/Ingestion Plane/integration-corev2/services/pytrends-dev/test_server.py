import unittest
from server import Collector, result, validate

QUERY = {"terms": ["web design"], "geo": "NO", "timeframe": "today 12-m"}


class CollectorTests(unittest.TestCase):
    def test_request_bounds(self):
        for change in ({"terms": []}, {"terms": ["a"] * 6}, {"terms": ["a", "a"]},
                       {"terms": ["bad\nterm"]}, {"geo": "http://localhost"},
                       {"timeframe": "now 1-H"}, {"url": "https://example.com"}):
            with self.assertRaises(ValueError):
                validate(dict(QUERY, **change))

    def test_reuses_original_observation_and_does_not_refetch(self):
        calls = []
        worker = Collector(run=lambda q: calls.append(q) or result(q, "available", [{"date": "2026-01-01", "values": [0], "partial": False}]))
        first = worker.get(QUERY)
        second = worker.get(QUERY)
        self.assertEqual(len(calls), 1)
        self.assertFalse(first["cached"])
        self.assertTrue(second["cached"])
        self.assertEqual(first["observedAt"], second["observedAt"])
        self.assertEqual(second["rows"][0]["values"], [0])

    def test_google_refusal_pauses_all_new_queries_without_fabricating_data(self):
        calls = []
        now = [10000]
        worker = Collector(run=lambda q: calls.append(q) or result(q, "rate_limited"), now=lambda: now[0])
        first = worker.get(QUERY)
        blocked = worker.get(dict(QUERY, terms=["other"]))
        self.assertEqual(first["status"], "rate_limited")
        self.assertEqual(blocked["status"], "cooldown")
        self.assertEqual(blocked["rows"], [])
        self.assertEqual(len(calls), 1)
        now[0] += 3601
        worker.get(dict(QUERY, terms=["other"]))
        self.assertEqual(len(calls), 2)

    def test_busy_does_not_queue_unbounded_work(self):
        worker = Collector(run=lambda _: self.fail("unexpected upstream call"))
        worker.lock.acquire()
        try:
            self.assertEqual(worker.get(QUERY)["status"], "cooldown")
        finally:
            worker.lock.release()


if __name__ == "__main__":
    unittest.main()
