CREATE INDEX idx_gscale_receipts_folded_barcode_updated
    ON public.mini_gscale_receipts (lower(barcode), updated_at DESC);
