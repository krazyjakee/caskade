-- older Caskade: compatible
-- Installs the user paused, kept across a restart so they resume where
-- they stopped (#131): the job as it was, and what its run needs again.
-- Its download stays in the storage root under the job's id.
CREATE TABLE paused_jobs (
    id TEXT PRIMARY KEY NOT NULL,
    record_json TEXT NOT NULL CHECK(json_valid(record_json)),
    paused_at_ms INTEGER NOT NULL
);
