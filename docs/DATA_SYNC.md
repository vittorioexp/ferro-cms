# Promoting content between environments

How to copy content and its files from one Ferro environment to another. The design
(bundle format, dependency closure, import rules) is in [ARCHITECTURE.md](./ARCHITECTURE.md) §7.

## Before you start

- An administrator token for both environments, declared in `ferro.toml` (§11) and
  exported as `FERRO_<ENV>_TOKEN`.
- The target schema must already have the collections and fields you export: run
  `ferro schema apply --to <env>` first.
- Collections with `sync.exclude: true` and users are never exported. `sync.natural_key`
  (for example `[slug]`) lets the import reuse the target's item when ids differ.

## Selecting content

```bash
--select pages                                # the whole collection
--select 'pages:id=0199…,0199…'               # some items
--select 'pages:{"slug":{"_eq":"home"}}'      # a filter
--shallow                                     # without the related items
```

`--select` can be repeated.

## Commands

```bash
# what would change (a dry run on the target)
ferro data diff --from dev --to prod --select 'pages:{"slug":{"_eq":"home"}}' --report diff.html

# export, copy the missing files, import
ferro data push --from dev --to prod --select pages --strategy safe --prune

# through a directory, when the environments cannot reach each other
ferro data export --from dev --select pages --out ./bundle
ferro data import --to prod --bundle ./bundle --dry-run --report import.html
ferro data import --to prod --bundle ./bundle --yes
```

Import options: `--strategy overwrite|safe|insert-only`, `--prune`, `--dry-run`,
`--force`, `--ignore-missing-fields`, `--yes` (see §7.4). A protected environment asks
for confirmation unless `--yes` is given. `--report file.html` and `--json` work on
diff, import and push.

## Files

The bundle carries file metadata; the binaries travel apart. `push` asks the target
which files it is missing (`POST /sync/missing-files`), copies only those
(`GET /sync/files/:id` on the source, `PUT /sync/files/:id` on the target), then
imports. `export` writes them to `files/<id>` and `import` uploads them first. The
import fails if a referenced binary is not in the target's storage.

## Admin UI

The Environment page has a "Promote content" card that uses the source URL and token of
the schema alignment card: pick a collection, an optional filter and the options, run
the preview, then promote. The source server must allow the admin origin in
`FERRO_CORS_ORIGINS`.

## When something goes wrong

- `missing in target (sync the schema)`: apply the schema to the target.
- `binary of file … missing`: copy the files before importing (`push` does it).
- `conflict` with `--strategy safe`: the item changed on the target since the last
  sync. Fix it there, use `--force`, or clear the sync history with `DELETE /sync/state`
  (optionally `{ "collection": "pages" }`).

Drafts are not exported; only live content is promoted.
