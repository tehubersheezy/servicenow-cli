---
paths:
  - "src/cli/catalog.rs"
  - "src/cli/import.rs"
  - "src/cli/identify.rs"
  - "src/cli/schema.rs"
  - "tests/schema.rs"
  - "tests/new_apis.rs"
---

# `sn catalog`, `sn import`, `sn identify`, `sn schema`

## Modules

- `src/cli/catalog.rs` → sn catalog list/get/categories/items/order/cart/checkout/wishlist
- `src/cli/import.rs` → sn import create/bulk/get (staging-table imports)
- `src/cli/identify.rs` → sn identify create-update/query + enhanced variants (CI reconciliation)
- `src/cli/schema.rs` → sn schema tables/columns/choices (undocumented SN endpoints)

**Endpoints** (`catalog`): `/api/sn_sc/servicecatalog`

full cart flow (browse → add → checkout) or direct `order` (bypasses cart); item-variables endpoint exposes required form fields

**Endpoints** (`import`): `/api/now/import/{stagingTableName}`

single create + bulk via `insertMultiple`; staging table is positional

**Endpoints** (`identify`): `/api/now/identifyreconcile`

POST-only; CI create/update + read-only query; enhanced variants accept `--options`; payload via `--data`

**Endpoints** (schema (undocumented)): `GET /api/now/doc/table/schema` (tables); `GET /api/now/ui/meta/{table}` (columns/choices/refs)

not in SN's OpenAPI specs; may 404 on very old instances
