-- Q3-E3.7.6-B-edge: a `seen` state for queue items, written by RequestQueue::mark_seen.
--
-- A seen row records a request id the crawl has already fetched (the final URL of a
-- redirect), so a later enqueue of the same id is refused. It is never popped. It carries no
-- URL: the url column is written empty. A seen row is not an enqueue or an ack, so the queue
-- stats do not count it.
--
-- The 0001 CHECK was declared inline, so Postgres named it quarry_queue_items_status_check;
-- it is dropped and re-added under the same name, which makes this file safe to apply twice.
-- Old binaries write only the four states they know, which the wider CHECK still accepts.
--
-- Number 0021: 0019 is the W23 queue step (see 0020); this takes the next free number.
ALTER TABLE quarry_queue_items
    DROP CONSTRAINT IF EXISTS quarry_queue_items_status_check;

ALTER TABLE quarry_queue_items
    ADD CONSTRAINT quarry_queue_items_status_check
    CHECK (status IN ('queued', 'in_flight', 'acked', 'failed', 'seen'));
