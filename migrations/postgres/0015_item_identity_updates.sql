-- Item codes are admin-editable master data. Direct operational relationships
-- must follow a code rename, while immutable event ledgers intentionally remain text
-- snapshots of the code used when each event occurred.
ALTER TABLE mini_customer_items
    DROP CONSTRAINT IF EXISTS mini_customer_items_item_code_fkey;

ALTER TABLE mini_customer_items
    ADD CONSTRAINT mini_customer_items_item_code_fkey
        FOREIGN KEY (item_code)
        REFERENCES mini_items(code)
        ON UPDATE CASCADE
        ON DELETE CASCADE;
