-- A paddon is a physical package. Its identity is a five-digit, database
-- allocated sequence number; it does not have an independent lifecycle state.

CREATE TABLE IF NOT EXISTS mini_paddon_sequence (
    id SMALLINT PRIMARY KEY,
    next_number INTEGER NOT NULL,
    CONSTRAINT mini_paddon_sequence_single_row CHECK (id = 1),
    CONSTRAINT mini_paddon_sequence_range CHECK (next_number BETWEEN 1 AND 100000)
);

-- Normalize rows created by the first Paddon implementation before enforcing
-- the five-digit code shape. The temporary value avoids unique-key conflicts
-- while the final numbers are assigned.
UPDATE mini_paddons
SET code = 'paddon-legacy-' || id
WHERE code !~ '^[0-9]{5}$';

WITH numbered AS (
    SELECT
        id,
        LPAD(
            ROW_NUMBER() OVER (ORDER BY created_at, id)::text,
            5,
            '0'
        ) AS code
    FROM mini_paddons
)
UPDATE mini_paddons AS paddon
SET code = numbered.code
FROM numbered
WHERE paddon.id = numbered.id;

INSERT INTO mini_paddon_sequence (id, next_number)
SELECT
    1,
    LEAST(
        COALESCE(
            MAX(CASE WHEN code ~ '^[0-9]{5}$' THEN code::integer ELSE 0 END),
            0
        ) + 1,
        100000
    )
FROM mini_paddons
ON CONFLICT (id) DO UPDATE
SET next_number = GREATEST(
    mini_paddon_sequence.next_number,
    EXCLUDED.next_number
);

ALTER TABLE mini_paddons
    DROP CONSTRAINT IF EXISTS mini_paddons_status_allowed,
    DROP COLUMN IF EXISTS status,
    DROP COLUMN IF EXISTS closed_at,
    ADD CONSTRAINT mini_paddons_code_five_digits CHECK (code ~ '^[0-9]{5}$');
