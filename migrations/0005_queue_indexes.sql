CREATE INDEX IF NOT EXISTS deliveries_pending_next_attempt_idx
    ON deliveries (next_attempt_at, created_at)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS deliveries_working_lease_idx
    ON deliveries (lease_until, created_at)
    WHERE status = 'working';
