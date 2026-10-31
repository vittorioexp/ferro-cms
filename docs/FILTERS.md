# Query filters

List endpoints (`GET /items/:collection`, `/files`, `/users`, `/activity`, …) accept a **Directus-compatible** filter language. The same JSON shape is used in GraphQL variables, permission item filters, flow conditions, and sync selections.

## Two ways to pass a filter on GET

### 1. JSON in `filter` query parameter

```http
GET /items/pages?filter={"status":{"_eq":"published"}}
```

URL-encode the JSON in clients. The admin and CLI send the object directly in JSON bodies where applicable.

### 2. Bracket query parameters (Directus-style)

```http
GET /items/pages?filter[status][_eq]=published
GET /items/pages?filter[slug][_eq]=home&filter[status][_eq]=published
```

Rules:

- Keys must look like `filter[<field>][<operator>]=<value>` (nested fields use more brackets for relations when the engine supports relational filters in SQL).
- Scalar values in the query string stay **strings** except `true`, `false`, `null`, and inline JSON objects/arrays.
- Invalid `filter[]=` without a field path is ignored.
- If both `filter=` JSON and bracket params are present, they are merged with **`_and`**.

Implementation: `ferro_core::filter::merge_query_filters` in the server `read_args` helper.

## Filter object shape

Leaf condition on a field:

```json
{ "status": { "_eq": "published" } }
```

Logical groups:

```json
{
  "_and": [
    { "status": { "_eq": "published" } },
    { "title": { "_icontains": "news" } }
  ]
}
```

```json
{
  "_or": [
    { "status": { "_eq": "draft" } },
    { "status": { "_eq": "review" } }
  ]
}
```

Relational filters follow field permissions (filtering on a field you cannot read is rejected).

## Operators

| Operator | Meaning |
|----------|---------|
| `_eq`, `_neq` | Equal / not equal |
| `_lt`, `_lte`, `_gt`, `_gte` | Comparison |
| `_in`, `_nin` | Value in / not in array |
| `_null`, `_nnull` | Is null / is not null |
| `_empty`, `_nempty` | Empty string or null / not empty |
| `_contains`, `_ncontains` | Substring |
| `_icontains`, `_nicontains` | Case-insensitive contains |
| `_starts_with`, `_nstarts_with`, `_istarts_with` | Prefix |
| `_ends_with`, `_nends_with`, `_iends_with` | Suffix |
| `_between`, `_nbetween` | Range (operand is two-element array) |

Permission filters may use **`$CURRENT_USER`**, **`$CURRENT_ROLE`**, **`$NOW`** (substituted server-side).

## Related query parameters

| Parameter | Purpose |
|-----------|---------|
| `fields` | Field tree (`*`, `title,author.name`) |
| `sort` | Comma-separated fields; `-` prefix = descending |
| `limit`, `offset`, `page` | Pagination |
| `search` | Full-text search on allowed fields |
| `meta` | `total_count`, `filter_count`, or `*` |
| `filter_count` | Uses the same filter as the list |

See [ARCHITECTURE.md](./ARCHITECTURE.md) §8 for the full REST surface.
