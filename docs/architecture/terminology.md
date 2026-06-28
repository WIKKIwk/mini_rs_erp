# Terminology Notes

This file records unclear domain names before they are renamed. The goal is to
avoid guessing and preserve API compatibility while refactoring structure.

## Werka

Current observed meaning:

- Backend role/capability area exposed through `/v1/mobile/werka/...`.
- Mobile role and screens under `features/werka`.
- Handles supplier-facing and internal operational flows around:
  - purchase receipts,
  - delivery notes,
  - supplier dispatch,
  - unannounced supplier receipts,
  - customer issue creation,
  - archive/history/pending/status views,
  - AI search suggestions.

Current decision:

- Do not rename modules, routes, DB fields, or mobile contracts yet.
- Treat `werka` as an unresolved business term until the product meaning is
  confirmed.
- Refactor inside the existing boundary first, preserving behavior and public
  route paths.
