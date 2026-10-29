"""Bounded in-process batches for immutable GCS delivery events."""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
import threading
import time
from typing import Callable, Generic, Hashable, TypeVar


Item = TypeVar("Item")
Key = TypeVar("Key", bound=Hashable)


@dataclass
class _Pending(Generic[Item]):
    item: Item
    complete: threading.Event = field(default_factory=threading.Event)
    error: BaseException | None = None


class BoundedBatcher(Generic[Key, Item]):
    """Coalesce concurrent compatible deliveries without acknowledging early."""

    def __init__(self, *, max_items: int, max_wait_seconds: float) -> None:
        if not isinstance(max_items, int) or max_items < 1:
            raise ValueError("max_items must be a positive integer")
        if not isinstance(max_wait_seconds, (int, float)) or max_wait_seconds < 0:
            raise ValueError("max_wait_seconds must be non-negative")
        self._max_items = max_items
        self._max_wait_seconds = float(max_wait_seconds)
        self._lock = threading.Lock()
        self._pending: dict[Key, list[_Pending[Item]]] = defaultdict(list)
        self._leaders: set[Key] = set()

    def submit(
        self,
        key: Key,
        item: Item,
        publish: Callable[[tuple[Item, ...]], None],
    ) -> None:
        pending = _Pending(item)
        with self._lock:
            queue = self._pending[key]
            queue.append(pending)
            leader = key not in self._leaders
            if leader:
                self._leaders.add(key)

        if leader:
            self._publish_queued(key, publish)
        else:
            pending.complete.wait()

        if pending.error is not None:
            raise pending.error

    def _publish_queued(self, key: Key, publish: Callable[[tuple[Item, ...]], None]) -> None:
        if self._max_wait_seconds:
            time.sleep(self._max_wait_seconds)

        while True:
            with self._lock:
                queue = self._pending[key]
                batch = queue[:self._max_items]
                del queue[:self._max_items]

            error: BaseException | None = None
            try:
                publish(tuple(entry.item for entry in batch))
            except Exception as exc:  # all deliveries must retry together
                error = exc
            finally:
                for entry in batch:
                    entry.error = error
                    entry.complete.set()

            with self._lock:
                if self._pending[key]:
                    continue
                self._pending.pop(key, None)
                self._leaders.remove(key)
                return
