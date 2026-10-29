-- Keep pallet events and old membership history after an empty pallet is deleted.
ALTER TABLE public.mini_qolip_paddons ADD COLUMN deleted_at TIMESTAMPTZ;
GRANT UPDATE(deleted_at) ON public.mini_qolip_paddons TO mini_rs_erp;
