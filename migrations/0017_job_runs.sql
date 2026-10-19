-- When a periodic job last started, cluster-wide.
--
-- The ticker that drives these jobs runs inside each replica, so with two
-- replicas every job fired twice per interval. The advisory lock around it
-- only prevents two replicas running *at the same moment*: because the pods
-- start at different times their ticks are offset, and by the time the second
-- replica ticks the first has long finished and released the lock, so it ran a
-- full second cycle. Measured on production 2026-10-19: ~290 ticks a day
-- against 43 skips, i.e. refresh_all was sweeping every calendar roughly every
-- five minutes instead of the intended ten, at twice the Notion API cost.
--
-- Recording the start time in one shared row turns "every replica runs every
-- interval" into "the cluster runs once per interval", whatever the replica
-- count.
CREATE TABLE job_runs (
    job TEXT PRIMARY KEY,
    last_started_at TIMESTAMPTZ NOT NULL
);
