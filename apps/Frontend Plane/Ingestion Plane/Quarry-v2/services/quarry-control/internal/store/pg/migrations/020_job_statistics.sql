-- What a finished crawl or batch reports about itself: page counts, status
-- classes, serving drivers, error categories and duration. Written once, from
-- the terminal event's payload, by the control resources handler. Closed
-- vocabularies only: never a URL, a title or an error message.
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS statistics JSONB;
