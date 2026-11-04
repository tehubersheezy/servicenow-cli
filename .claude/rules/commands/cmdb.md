---
paths:
  - "src/cli/cmdb.rs"
  - "tests/cmdb_ire.rs"
---

# `sn cmdb`

## Modules

- `src/cli/cmdb.rs` → sn cmdb list/get/create/update/meta + relation add/delete

**Endpoints** (`cmdb`): `/api/now/cmdb/instance/{className}` (CRUD + relations); `/api/now/cmdb/meta/{className}` (schema)

className is a positional arg

- **`sn cmdb create/update` writes go out as the IRE envelope** `{"attributes": {...}, "source": "..."}` (`cmdb.rs::ire_envelope`) — the CMDB Instance API accepts nothing else (PATCH of a flat body is a raw Java NPE, HTTP 500 `"attributes" is null`; POST is a 400 naming a null data source), so `--field` could never work on `cmdb` at all. A body whose `attributes` is a JSON *object* is passed through as a caller-written envelope (the only way to send `inbound_relations`/`outbound_relations` on create); anything else is wrapped. Keying off the key's *presence* would be wrong: 718 CMDB classes on a stock Zurich instance carry a real column named `attributes`, all String or Field List, so a real field value is never an object on the wire.
  - **Attribute values must be strings.** The API casts each one to `java.lang.String` before it reaches the record, so `--field cpu_count=8` answers HTTP 500 `class java.lang.Integer cannot be cast to class java.lang.String` and writes nothing (likewise `Boolean`, `LinkedHashMap`, `ArrayList`). `ire_envelope` stringifies numbers and booleans, refuses objects and arrays with a usage error naming the attribute, and leaves `null` alone (Java casts it fine, so it stays usable for clearing a field). **This is a CMDB-only constraint and must not be pushed into `body.rs`** — `coerce_field_value` is shared with every other command, and the Table API takes JSON numbers happily. It applies inside a hand-written envelope too: passthrough is about the body's *shape*, not an opt-out of a cast the server performs regardless.
  - `--source` defaults to `"Manual Entry"` (a CLI write literally is a manual entry). `source` given both in the body and as the flag is a usage error, and a **flat** body's top-level `source` is refused as ambiguous rather than guessed: demoting it into `attributes` had the API drop it and stamp the record with the default provenance (exit 0, wrong provenance, nothing said), while lifting it would swallow a legitimate field write on `cmdb_ci_service_discovered`/`cmdb_ci_service_calculated`, which have a real `source` column. Both readings lose something silently, so the caller is asked which they meant. Provenance is what the IRE reconciles on, and a borrowed discovery-source name lets that tool's next run overwrite a hand edit.
