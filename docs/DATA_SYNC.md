# Data promotion between environments

Move **content and file binaries** from one Ferro environment to another with stable UUIDs, dependency closure, natural keys, conflict strategies, and optional prune.

Related: [ENVIRONMENT.md](./ENVIRONMENT.md) (schema vs data), [ARCHITECTURE.md](./ARCHITECTURE.md) §7, [API.md](./API.md) (`/sync/*`).

## Prerequisites

- **Administrator** access on source (export, read files) and target (import, write files).
- Target **schema** compatible with exported collections (run `ferro schema apply` first if needed).
- For **`safe`** strategy: prior successful imports record content hashes in `ferro_sync_state`.

## What gets exported

- Selected items plus **dependency closure** (M2O, O2M, M2M, M2A blocks, related rows).
- **`ferro_files`** metadata and **folder** chain for referenced assets.
- **Not exported**: users (system user fields stripped; foreign user refs nulled on import), collections with `sync.exclude: true`.

Configure per field/collection in YAML:

```yaml
sync:
  natural_key: [slug]   # remap ids when the same business key exists on target
  exclude: false        # true = never in bundles
```

Relation modes: default shallow/deep, or `sync: deep | reference | skip` on fields ([ARCHITECTURE.md](./ARCHITECTURE.md) §7.3).

## Bundle format (`ferro-bundle/1`)

```
bundle/
  bundle.json     # items, collection defs, folders, file rows, "complete" parents
  files/<uuid>    # one binary per referenced file id
```

`bundle.json` is returned by `POST /sync/export` inside `{ "data": … }`.

## CLI

Configure [ferro.toml](./ENVIRONMENT.md), set `FERRO_*_TOKEN` env vars.

### Selection syntax

```bash
--select pages                                    # whole collection (with deps)
--select 'pages:id=0199…,0199…'                   # by id
--select 'pages:{"slug":{"_eq":"home"}}'          # by filter
--shallow                                         # no dependency closure
```

### Commands

```bash
# Preview (dry run on target; file binaries not required)
ferro data diff --from dev --to prod --select 'pages:{"slug":{"_eq":"home"}}' --report diff.html

# One-step: export + transfer missing files + import
ferro data push --from dev --to prod --select pages --strategy safe --prune --yes

# File-based (air-gapped or manual approval)
ferro data export --from dev --select pages --out ./bundle
ferro data import --to prod --bundle ./bundle --yes
```

### Import options

| Flag | Effect |
|------|--------|
| `--strategy overwrite` | Default; update differing items |
| `--strategy safe` | Skip/update unless target changed since last sync; else **conflict** |
| `--strategy insert-only` | Never update existing rows |
| `--prune` | Delete/detach owned children removed at source (complete parents only) |
| `--dry-run` | Transaction rolled back; report = exact diff |
| `--ignore-missing-fields` | Drop fields missing on target schema |
| `--force` | Apply despite conflicts (overwrites conflicting items) |
| `--yes` | Skip protected-environment prompt |

Protected targets (`protected = true` in `ferro.toml`) show a dry-run summary before apply unless `--yes`.

## Asset (binary) transfer

Metadata travels in the bundle; **bytes** are copied separately so large media is not embedded in JSON.

**Push / admin flow:**

1. `POST /sync/export` on **source** → bundle includes `files[]` with id + checksum.
2. `POST /sync/missing-files` on **target** with `[{ id, checksum? }, …]` → list of ids still needed.
3. For each id: `GET /sync/files/:id` on source → `PUT /sync/files/:id` on target (raw body).
4. `POST /sync/import` on target — verifies each file blob exists in default storage before committing rows.

Checksum match + blob present ⇒ skip re-upload. Export to disk writes `files/<uuid>`; import reads that directory before calling the API.

E2e coverage: `tests/e2e/02-sync.mjs` (“transferred 1 file”, asset served on target).

## HTTP API summary

See [API.md](./API.md). All routes require **admin** except none are public.

Reset sync history (for `safe` conflicts): `DELETE /sync/state` or with `{ "collection": "pages" }`.

## Admin UI

**Environment** (`#/schema`) → **Promote content from another server**:

- Reuses **Source URL** and **Source token** from schema alignment.
- Pick collection, optional filter JSON, strategy, dependency closure, prune.
- **Preview (dry run)** then **Promote content + files** (confirmation dialog).

## Reports

CLI: `--report file.html` on diff/import/push (field-level HTML). JSON via `--json`.

## Troubleshooting

| Symptom | Check |
|---------|--------|
| `missing in target (sync the schema)` | `ferro schema apply --to prod` |
| `binary of file … missing` | Run file transfer before import (push does this automatically) |
| `conflict` with `safe` | Resolve on target, use `--force`, or `DELETE /sync/state` |
| `natural key` remap in report | Expected when same slug/name exists with different uuid |
| CORS errors in admin | Source must allow admin origin for export/file GET |

## Strategies reference

| Strategy | When target row exists and content differs |
|----------|---------------------------------------------|
| `overwrite` | Update from bundle |
| `safe` | Update only if hash matches last sync; else conflict |
| `insert-only` | Leave target unchanged (`skip`) |

Draft rows are environment-local; live content promotion follows published item rules in the engine ([ARCHITECTURE.md](./ARCHITECTURE.md) §8.6).
