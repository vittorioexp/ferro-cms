// Aggregations, OpenAPI, HTTP cache smoke (rate limit optional via env).
import { rnd } from './lib.mjs';

export default async function ({ a, check }) {
  let r = await a.rest('GET', '/server/openapi.json');
  check('openapi.json', r.status === 200 && r.json?.openapi?.startsWith('3.'), r.json?.info?.title);

  r = await a.rest('POST', '/items/pages/aggregate', {
    group_by: ['status'],
    ops: [{ alias: 'cnt', function: 'count', field: '*' }],
  });
  check('aggregate group_by status', r.status === 200 && Array.isArray(r.data) && r.data.length >= 1, r.data);

  r = await a.rest('POST', '/items/pages/aggregate', {
    ops: [{ alias: 'total', function: 'count', field: '*' }],
  });
  check('aggregate global count', r.status === 200 && r.data?.[0]?.total >= 1, r.data);

  r = await a.rest('POST', '/items/pages/aggregate', {
    ops: [
      { alias: 'a', function: 'count' },
      { alias: 'a', function: 'count' },
    ],
  });
  check('duplicate aggregate alias rejected', r.status === 400, r.error);

  const slug = `agg-${rnd()}`;
  await a.rest('POST', '/items/pages', { title: 'Agg', slug, status: 'published' });
  r = await a.rest('GET', '/items/pages', { fields: 'id', limit: 1 });
  check('list still works with platform middleware', r.status === 200, r.status);
}
