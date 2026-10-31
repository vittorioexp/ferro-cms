// Draft / publish workflow: live vs draft reads, publish, version history, discard, schedule.
import { rnd } from './lib.mjs';

export default async function ({ a, check }) {
  const slug = `ver-${rnd()}`;
  const q = (params) => {
    const p = new URLSearchParams(params).toString();
    return p ? `?${p}` : '';
  };

  const created = await a.rest('POST', '/items/pages', { title: 'Live title', slug, status: 'published' });
  check('create live page', created.status === 200, created);
  const id = created.data.id;

  let r = await a.rest('PATCH', `/items/pages/${id}${q({ version: 'draft' })}`, { title: 'Draft title' });
  check('save draft', r.status === 200, r);
  r = await a.rest('PATCH', `/items/pages/${id}${q({ version: 'draft' })}`, { title: 'Draft title v2' });
  r = await a.rest('GET', `/items/pages/${id}${q({ fields: 'title,slug', version: 'draft' })}`);
  check('partial draft patch merges onto prior draft/live', r.data?.title === 'Draft title v2' && r.data?.slug === slug, r.data);
  r = await a.rest('GET', `/items/pages/${id}${q({ fields: 'title', version: 'published' })}`);
  check('live row unchanged', r.data?.title === 'Live title', r.data);
  r = await a.rest('GET', `/items/pages/${id}${q({ fields: 'title', version: 'draft' })}`);
  check('draft read', r.data?.title === 'Draft title v2', r.data);
  r = await a.rest('GET', `/items/pages/${id}${q({ meta: 'draft' })}`);
  check('draft meta', r.json?.meta?.draft?.has_draft === true, r.json?.meta);

  r = await a.rest('POST', `/items/pages/${id}/publish`, { force: true });
  check('publish now', r.status === 200 && r.data?.title === 'Draft title v2', r);
  r = await a.rest('GET', `/items/pages/${id}/versions`);
  check('published version archived', r.status === 200 && r.data?.length >= 1, r.data);

  r = await a.rest('PATCH', `/items/pages/${id}${q({ version: 'draft' })}`, { title: 'Restore candidate' });
  check('second draft', r.status === 200, r);
  const verId = (await a.rest('GET', `/items/pages/${id}/versions`)).data?.[0]?.id;
  r = await a.rest('POST', `/items/pages/${id}/versions/${verId}/restore`);
  check('restore version into draft', r.status === 200 && r.data?.title === 'Live title', r.data);

  r = await a.rest('DELETE', `/items/pages/${id}/draft`);
  check('discard draft', r.status === 204, r.status);
  r = await a.rest('GET', `/items/pages/${id}${q({ fields: 'title', version: 'draft' })}`);
  check('without draft, draft read falls back to live', r.data?.title === 'Draft title v2', r.data);

  const coll = `ver_plain_${rnd()}`;
  await a.rest('POST', '/collections', { name: coll, fields: [{ name: 'title', type: 'string' }] });
  const plain = (await a.rest('POST', `/items/${coll}`, { title: 'x' })).data;
  r = await a.rest('PATCH', `/items/${coll}/${plain.id}${q({ version: 'draft' })}`, { title: 'y' });
  check('version=draft rejected when versioning disabled', r.status === 400, r.error);

  const slug2 = `ver2-${rnd()}`;
  const p2 = (await a.rest('POST', '/items/pages', { title: 'Sched live', slug: slug2, status: 'published' })).data;
  const future = new Date(Date.now() + 3600_000).toISOString();
  await a.rest('PATCH', `/items/pages/${p2.id}${q({ version: 'draft' })}`, { title: 'After schedule', publish_at: future });
  r = await a.rest('POST', `/items/pages/${p2.id}/publish`, { publish_at: future });
  check('future publish_at schedules without promoting', r.status === 200 && r.json?.data?.scheduled === true, r.json);
  r = await a.rest('GET', `/items/pages/${p2.id}${q({ fields: 'title', version: 'published' })}`);
  check('live still old while scheduled', r.data?.title === 'Sched live', r.data);
  r = await a.rest('POST', `/items/pages/${p2.id}/publish`, { force: true });
  check('force publish applies draft', r.status === 200 && r.data?.title === 'After schedule', r.data);
}
