# CLI reference

The **`ferro`** binary is built from `crates/ferro-cli`. It targets project workflows: schema, data, migrations, and local serve helpers. The API server itself is **`ferro serve`** (PostgreSQL required).

Deep behaviour: [ARCHITECTURE.md](./ARCHITECTURE.md) §12–§13. Environment names and tokens: [ENVIRONMENT.md](./ENVIRONMENT.md).

## Project file

`ferro.toml` at the repo root (or `--project`):

```toml
[project]
schema = "schema"

[environments.dev]
url = "http://localhost:8055"
token_env = "FERRO_DEV_TOKEN"

[environments.prod]
url = "https://api.example.com"
token_env = "FERRO_PROD_TOKEN"
protected = true
```

Export `FERRO_DEV_TOKEN` / `FERRO_PROD_TOKEN` as admin static tokens or JWT from the target environment.

## Common commands

| Command | Purpose |
|---------|---------|
| `ferro serve` | Start API + admin (reads `.env`) |
| `ferro schema plan` | Dry-run schema diff vs database |
| `ferro schema apply` | Apply schema from `schema/` |
| `ferro schema diff --from dev --to prod` | Compare snapshots between environments |
| `ferro schema apply --to prod` | Push schema to remote |
| `ferro data diff --from dev --to prod --select …` | Preview content promotion |
| `ferro data push --from dev --to prod --select …` | Export + file transfer + import |
| `ferro data export --from dev --select … --out ./bundle` | Write bundle + `files/` to disk |
| `ferro data import --to prod --bundle ./bundle` | Import from disk |
| `ferro migrate` | Run system migrations (Postgres/SQLite/MySQL URL) |

Selection syntax for `data *`: see [DATA_SYNC.md](./DATA_SYNC.md).

## Flags shared by schema/data

- `--from` / `--to` — environment names from `ferro.toml`
- `--yes` — skip protected-environment confirmation
- `--json` — machine-readable output where supported
- `--report path.html` — HTML diff report (data commands)

## Help

```bash
ferro --help
ferro schema --help
ferro data --help
```
