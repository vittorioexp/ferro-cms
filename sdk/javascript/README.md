# Ferro JavaScript SDK

Thin wrapper around the Ferro REST API.

```js
import { FerroClient } from '@ferro/sdk';

const client = new FerroClient('http://localhost:8055');
await client.login('admin@example.com', 'secret');

const rows = await client.items('pages', {
  filter: JSON.stringify({ status: { _eq: 'published' } }),
  limit: 20,
});
// Or bracket query params: `?filter[status][_eq]=published` (same filter language)

const stats = await client.aggregate('pages', {
  group_by: ['status'],
  ops: [{ alias: 'cnt', function: 'count', field: '*' }],
});

const spec = await client.openapi();
```

See [docs/ARCHITECTURE.md](../../docs/ARCHITECTURE.md) §8.9 for server-side behaviour (rate limits, HTTP cache, OpenAPI).
