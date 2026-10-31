# Environments and promotion

An **environment** is one running Ferro API instance (database + storage + config) with a name such as `local`, `dev`, or `prod`. The CLI addresses them through **`ferro.toml`**; the admin shows the current name on **Environment** (`#/schema`).

## Project file (`ferro.toml`)

```toml
[project]
schema = "schema"          # directory of YAML snapshots (git)

[environments.local]
url = "http://localhost:8055"
token_env = "FERRO_LOCAL_TOKEN"

[environments.prod]
url = "https://cms.example.com"
token_env = "FERRO_PROD_TOKEN"
protected = true           # CLI asks for confirmation (or --yes in CI)
```

- **`token_env`**: name of an env var holding an administrator API token (`ferro user token --email …`).
- **`protected`**: `ferro schema apply` and `ferro data push` require explicit confirmation unless `--yes`.

Commands:

```bash
ferro env status              # ping each env; compare schema hashes
ferro schema pull --from dev  # snapshot YAML from dev into schema/
ferro schema diff  --to prod
ferro schema apply --to prod
```

## Two kinds of promotion

| Kind | What moves | Tooling |
|------|------------|---------|
| **Schema** | Collections, fields, roles, policies, flows (model + access) | `ferro schema *`, admin **Align this environment to another** |
| **Data** | Items, junction rows, folders, **file metadata + binaries** | `ferro data *`, admin **Promote content from another server**, `/sync/*` API |

Always align **schema** before **data** when the target is missing collections or fields. Data import checks compatibility and errors with “sync the schema first” when needed.

Details:

- Schema: [ARCHITECTURE.md](./ARCHITECTURE.md) §6  
- Data + assets: [DATA_SYNC.md](./DATA_SYNC.md)  

## Admin Environment page (`#/schema`)

1. **Info** — environment name, schema hash, download YAML snapshot, apply JSON snapshot file (with plan preview).
2. **Align this environment to another** — fetch remote `/schema/snapshot`, plan/apply on **this** server (needs CORS on source).
3. **Promote content from another server** — export selection from source, transfer missing **file binaries**, import on **this** server (dry run or apply). Uses the same URL/token fields as schema align.
4. **Schema history** — recent applies on this instance.

Content context: when you browse a collection in the admin, the last opened collection is remembered for the **Assistant** (`context.collection`).

## Schema hash

Identical hash ⇒ identical model and permissions snapshot. Commit `schema/` after `ferro schema pull` so git is the source of truth; CI can `ferro schema diff --to prod --exit-code`.

## When environments cannot talk to each other

Use file-based data bundles:

```bash
ferro data export --from dev --select pages --out ./bundle
ferro data import --to prod --bundle ./bundle --yes
```

The `bundle/files/` directory holds asset binaries; import uploads missing blobs on the target before applying rows.

See [ARCHITECTURE.md](./ARCHITECTURE.md) §7 and §12 for CI/CD patterns.
