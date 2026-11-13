// Draft / publish workflow: live vs draft reads, publish, version history, discard, schedule.
import { rnd } from './lib.mjs';

export default async function ({ a, check }) {
  const slug = `ver-${rnd()}`;
  const q = (params) => {
    const p = new URLSearchParams(params).toString();
    return p ? `?${p}` : '';
  };

  const coll = `ver_pages_${rnd()}`;
  const made = await a.rest('POST', '/collections', {
    name: coll,
    meta: { versioning: { enabled: true, schedule_field: 'publish_at' } },
    fields: [
      { name: 'title', type: 'string' },
      { name: 'slug', type: 'string' },
      { name: 'status', type: 'string' },
      { name: 'seo', type: 'json' },
      { name: 'publish_at', type: 'datetime' },
    ],
  });
  check('create a versioned collection', made.status === 200, made.error);

  const created = await a.rest('POST', `/items/${coll}`, { title: 'Live title', slug, status: 'published' });
  check('create live page', created.status === 200, created);
  const id = created.data.id;

  let r = await a.rest('PATCH', `/items/${coll}/${id}${q({ version: 'draft' })}`, { title: 'Draft title' });
  check('save draft', r.status === 200, r);
  r = await a.rest('PATCH', `/items/${coll}/${id}${q({ version: 'draft' })}`, { title: 'Draft title v2' });
  r = await a.rest('GET', `/items/${coll}/${id}${q({ fields: 'title,slug', version: 'draft' })}`);
  check('partial draft patch merges onto prior draft/live', r.data?.title === 'Draft title v2' && r.data?.slug === slug, r.data);
  r = await a.rest('GET', `/items/${coll}/${id}${q({ fields: 'title', version: 'published' })}`);
  check('live row unchanged', r.data?.title === 'Live title', r.data);
  r = await a.rest('GET', `/items/${coll}/${id}${q({ fields: 'title', version: 'draft' })}`);
  check('draft read', r.data?.title === 'Draft title v2', r.data);
  r = await a.rest('GET', `/items/${coll}/${id}${q({ meta: 'draft' })}`);
  check('draft meta', r.json?.meta?.draft?.has_draft === true, r.json?.meta);

  r = await a.rest('POST', `/items/${coll}/${id}/publish`, { force: true });
  check('publish now', r.status === 200 && r.data?.title === 'Draft title v2', r);
  r = await a.rest('GET', `/items/${coll}/${id}/versions`);
  check('published version archived', r.status === 200 && r.data?.length >= 1, r.data);

  r = await a.rest('PATCH', `/items/${coll}/${id}${q({ version: 'draft' })}`, { title: 'Restore candidate' });
  check('second draft', r.status === 200, r);
  const verId = (await a.rest('GET', `/items/${coll}/${id}/versions`)).data?.[0]?.id;
  r = await a.rest('POST', `/items/${coll}/${id}/versions/${verId}/restore`);
  check('restore version into draft', r.status === 200 && r.data?.title === 'Live title', r.data);

  r = await a.rest('DELETE', `/items/${coll}/${id}/draft`);
  check('discard draft', r.status === 204, r.status);
  r = await a.rest('GET', `/items/${coll}/${id}${q({ fields: 'title', version: 'draft' })}`);
  check('without draft, draft read falls back to live', r.data?.title === 'Draft title v2', r.data);

  const plainColl = `ver_plain_${rnd()}`;
  await a.rest('POST', '/collections', { name: plainColl, fields: [{ name: 'title', type: 'string' }] });
  const plain = (await a.rest('POST', `/items/${plainColl}`, { title: 'x' })).data;
  r = await a.rest('PATCH', `/items/${plainColl}/${plain.id}${q({ version: 'draft' })}`, { title: 'y' });
  check('version=draft rejected when versioning disabled', r.status === 400, r.error);

  const slug2 = `ver2-${rnd()}`;
  const p2 = (await a.rest('POST', `/items/${coll}`, { title: 'Sched live', slug: slug2, status: 'published' })).data;
  const future = new Date(Date.now() + 3600_000).toISOString();
  await a.rest('PATCH', `/items/${coll}/${p2.id}${q({ version: 'draft' })}`, { title: 'After schedule', publish_at: future });
  r = await a.rest('POST', `/items/${coll}/${p2.id}/publish`, { publish_at: future });
  check('future publish_at schedules without promoting', r.status === 200 && r.json?.data?.scheduled === true, r.json);
  r = await a.rest('GET', `/items/${coll}/${p2.id}${q({ fields: 'title', version: 'published' })}`);
  check('live still old while scheduled', r.data?.title === 'Sched live', r.data);
  r = await a.rest('POST', `/items/${coll}/${p2.id}/publish`, { force: true });
  check('force publish applies draft', r.status === 200 && r.data?.title === 'After schedule', r.data);

  // a draft holds only its own changes: live edits made after it survive publishing
  const slug3 = `ver3-${rnd()}`;
  const p3 = (await a.rest('POST', `/items/${coll}`, { title: 'Live 3', slug: slug3, status: 'draft', seo: { a: 1, b: 2 } })).data;
  await a.rest('PATCH', `/items/${coll}/${p3.id}${q({ version: 'draft' })}`, { title: 'Draft 3', seo: { a: 9 } });
  r = await a.rest('GET', `/items/${coll}/${p3.id}${q({ fields: 'title', version: 'draft' })}`);
  check('draft read honours fields', r.data?.title === 'Draft 3' && !('slug' in (r.data || {})) && !('seo' in (r.data || {})), r.data);
  await a.rest('PATCH', `/items/${coll}/${p3.id}`, { status: 'published' });
  r = await a.rest('POST', `/items/${coll}/${p3.id}/publish`, { force: true });
  check(
    'publishing keeps live edits made after the draft',
    r.status === 200 && r.data?.title === 'Draft 3' && r.data?.status === 'published' && r.data?.slug === slug3,
    r.data,
  );
  check('JSON fields in a draft replace the whole value, like PATCH', JSON.stringify(r.data?.seo) === JSON.stringify({ a: 9 }), r.data?.seo);

  // deleting an item drops its draft (otherwise the scheduler would try to publish a ghost)
  const p4 = (await a.rest('POST', `/items/${coll}`, { title: 'Gone', slug: `ver4-${rnd()}`, status: 'draft' })).data;
  await a.rest('PATCH', `/items/${coll}/${p4.id}${q({ version: 'draft' })}`, { title: 'Gone draft', publish_at: new Date(Date.now() + 3600_000).toISOString() });
  r = await a.rest('DELETE', `/items/${coll}/${p4.id}`);
  check('delete item with a draft', r.status === 204, r.status);
  await a.rest('POST', `/items/${coll}`, { id: p4.id, title: 'Reborn', slug: `ver4b-${rnd()}`, status: 'draft' });
  r = await a.rest('GET', `/items/${coll}/${p4.id}${q({ meta: 'draft', fields: 'title' })}`);
  check('the old draft did not survive the delete', r.json?.meta?.draft?.has_draft === false && r.data?.title === 'Reborn', r.json);
}
