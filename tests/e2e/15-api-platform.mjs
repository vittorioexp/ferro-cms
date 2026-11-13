// Aggregations, HTTP cache (environment b runs with the cache and the rate limiter on).
import { rnd } from './lib.mjs';

export default async function ({ a, b, check }) {
  let r = await a.rest('POST', '/items/pages/aggregate', {
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

  r = await a.rest('GET', '/items/pages?filter[id][_in][99999999]=x');
  check('bracket filter with a huge list index is refused', r.status === 400, r.status);

  // a stale or foreign bearer token must not break endpoints that do not need one
  r = await a.rest('POST', '/auth/login', { email: 'admin@example.com', password: 'e2e-password-123' }, 'not-a-valid-token');
  check('login works with a leftover invalid bearer header', r.status === 200 && !!r.data?.access_token, r.status);
  const health = await fetch(`${a.url}/admin/`, { headers: { Authorization: 'Bearer expired.or.bogus' } });
  check('admin UI is served whatever the bearer header', health.status === 200, health.status);

  // ---- HTTP cache (environment b) ----
  const slug = `cache-${rnd()}`;
  const page = (await b.rest('POST', '/items/pages', { title: 'Before', slug, status: 'published' })).data;
  r = await b.rest('GET', `/items/pages/${page.id}?fields=title`);
  check('cached read', r.data?.title === 'Before' && /no-cache/.test(r.headers.get('cache-control') || '') && !!r.headers.get('etag'), [r.data, r.headers.get('cache-control')]);
  const etag = r.headers.get('etag');
  const nm = await fetch(`${b.url}/items/pages/${page.id}?fields=title`, { headers: { Authorization: `Bearer ${b.token}`, 'If-None-Match': etag } });
  check('If-None-Match answers 304', nm.status === 304 && nm.headers.get('etag') === etag, nm.status);
  const list1 = await b.rest('GET', `/items/pages?fields=title&filter[slug][_eq]=${slug}`);
  await b.rest('PATCH', `/items/pages/${page.id}`, { title: 'After' });
  r = await b.rest('GET', `/items/pages/${page.id}?fields=title`);
  check('a write invalidates cached items', r.data?.title === 'After', r.data);
  r = await b.rest('GET', `/items/pages?fields=title&filter[slug][_eq]=${slug}`);
  check('a write invalidates cached lists', list1.data?.[0]?.title === 'Before' && r.data?.[0]?.title === 'After', r.data);

  const coll = `cache_big_${rnd()}`;
  await b.rest('POST', '/collections', { name: coll, fields: [{ name: 'body', type: 'text' }] });
  await b.rest('POST', `/items/${coll}`, Array.from({ length: 8 }, (_, i) => ({ body: `${i}`.repeat(100_000) })));
  for (let i = 0; i < 2; i++) {
    r = await b.rest('GET', `/items/${coll}?limit=-1`);
    check(`responses above the cache size limit are returned whole (#${i + 1})`, r.status === 200 && r.data?.length === 8 && r.text.length > 800_000, [r.status, r.text.length]);
  }
}
