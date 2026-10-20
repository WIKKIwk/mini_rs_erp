# Qolip set identifiers (0154)

The actual `mini_qolip_sets.set_id` is now `9501` followed by eight digits.
PostgreSQL allocates it from a bounded, noncycling sequence. The identity remains
unchanged when members are added, removed or edited. The separate `set_code`
and historical `display_code` columns are removed. The JSON `display_code`
field remains equal to `set_id` solely for older mobile clients.

Migration 0154 changes the registry, physical mold payloads, product links and
nested JSON references (including session/progress snapshots and saved replies)
in one transaction. It checks that all effective product/mold bindings and
explicit product/set links are preserved. The maintenance-only
`mini_qolip_set_id_migrations` table records the previous IDs and codes for
recovery; it is not used for application lookups.

## Rollout

1. Back up the database using the existing database backup procedure.
2. Stop the old backend before migrating: it writes old-format IDs and queries
   the columns removed by 0154.
3. Build the backend and `mini_rs_migrate` from the same revision. Apply the
   migration with the configured maintenance credentials. On the local Mac,
   follow `docs/macos-postgres-access.md`; normal HTTP startup uses the restricted
   runtime account and cannot apply this schema change.
4. Start the updated backend and reload the mobile application. Cached set
   selections from before the migration must be reloaded and reconfirmed.

Do not rewrite 0153 or its checksum on installations where it is already applied.
Do not run an old backend against the migrated schema. A rollback requires the
matching database backup and previous application revision.

## Standalone sets (0155)

`POST /v1/mobile/qolip/sets/create` accepts `warehouse` and a nonempty `members`
list of `qolip_code`, `size` and `color`. The authenticated caller must have
access to the receiving warehouse. The server allocates the numeric set ID and
commits all members and the creation audit event in one transaction. Conflicting
mold codes reject the complete operation.

Migration 0155 permits a NULL source product in the set registry. These physical
molds have empty product metadata and no initial product link; no placeholder
catalog item is created. Choosing the set for a product creates the existing
explicit association. Product-facing reads and stock labels use that linked
product while set membership and source identity stay independent.

Apply 0155 with the same maintenance rollout above before enabling standalone
creation. Existing product-bound sets and links require no data rewrite.
