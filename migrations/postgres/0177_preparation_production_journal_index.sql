SET LOCAL lock_timeout = '5s';
SET LOCAL statement_timeout = '60s';

-- The preparation journal refreshes own production, without scanning every
-- factory receipt on each read. Draft receipts never form production output.
CREATE INDEX idx_mini_gscale_receipts_preparation_owner
    ON public.mini_gscale_receipts ((payload_json->>'actor_ref'), barcode)
    WHERE status='submitted' AND payload_json->>'actor_role'='tayyorlov_masteri';
