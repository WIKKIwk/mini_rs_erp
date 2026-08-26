CREATE TABLE mini_apparatus_collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    revision BIGINT NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT mini_apparatus_collection_id_shape CHECK (
        id ~ '^apparatus-collection:[0-9a-f]{32}$'
    ),
    CONSTRAINT mini_apparatus_collection_name_valid CHECK (
        name = btrim(name)
        AND char_length(name) BETWEEN 1 AND 80
    ),
    CONSTRAINT mini_apparatus_collection_revision_positive CHECK (revision > 0)
);

CREATE UNIQUE INDEX mini_apparatus_collections_name_unique
    ON mini_apparatus_collections (lower(name));

CREATE TABLE mini_apparatus_collection_members (
    collection_id TEXT NOT NULL,
    apparatus_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY (collection_id, apparatus_id),
    CONSTRAINT mini_apparatus_collection_member_collection_fk
        FOREIGN KEY (collection_id)
        REFERENCES mini_apparatus_collections (id)
        ON UPDATE RESTRICT ON DELETE CASCADE,
    CONSTRAINT mini_apparatus_collection_member_apparatus_fk
        FOREIGN KEY (apparatus_id)
        REFERENCES mini_canonical_apparatus_identities (apparatus_id)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CONSTRAINT mini_apparatus_collection_member_position_nonnegative CHECK (position >= 0),
    CONSTRAINT mini_apparatus_collection_member_position_unique
        UNIQUE (collection_id, position)
);

CREATE INDEX mini_apparatus_collection_members_apparatus_idx
    ON mini_apparatus_collection_members (apparatus_id);
