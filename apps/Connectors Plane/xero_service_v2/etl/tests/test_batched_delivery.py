from concurrent.futures import ThreadPoolExecutor
import threading
import time

from etl.common.batched_delivery import BoundedBatcher


def test_bounded_batcher_coalesces_concurrent_items_and_waits_for_commit():
    batcher = BoundedBatcher(max_items=8, max_wait_seconds=0.03)
    delivered = []
    delivered_lock = threading.Lock()

    def publish(items):
        with delivered_lock:
            delivered.append(tuple(items))

    with ThreadPoolExecutor(max_workers=4) as pool:
        futures = [pool.submit(batcher.submit, "raw", item, publish) for item in range(4)]
        for future in futures:
            assert future.result(timeout=2) is None

    assert delivered == [(0, 1, 2, 3)]


def test_bounded_batcher_keeps_distinct_keys_independent():
    batcher = BoundedBatcher(max_items=8, max_wait_seconds=0)
    observed = []
    lock = threading.Lock()

    def publish(items):
        with lock:
            observed.append(tuple(items))

    with ThreadPoolExecutor(max_workers=2) as pool:
        first = pool.submit(batcher.submit, "raw", "raw-item", publish)
        second = pool.submit(batcher.submit, "typed", "typed-item", publish)
        assert first.result(timeout=2) is None
        assert second.result(timeout=2) is None

    assert set(observed) == {("raw-item",), ("typed-item",)}


def test_bounded_batcher_propagates_publish_failures_to_all_queued_callers():
    batcher = BoundedBatcher(max_items=8, max_wait_seconds=0.03)
    barrier = threading.Barrier(2)

    def publish(_items):
        raise RuntimeError("publish failed")

    def submit(item):
        barrier.wait()
        batcher.submit("raw", item, publish)

    with ThreadPoolExecutor(max_workers=2) as pool:
        futures = [pool.submit(submit, item) for item in (1, 2)]
        for future in futures:
            try:
                future.result(timeout=2)
            except RuntimeError as exc:
                assert str(exc) == "publish failed"
            else:
                raise AssertionError("publish failure must be returned to every caller")

    time.sleep(0.01)
