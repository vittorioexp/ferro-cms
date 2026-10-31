// Comments, notifications, presets, dashboard, insights.
import { rnd } from './lib.mjs';

export default async function ({ a, check }) {
  const slug = `ws-${rnd()}`;
  const page = (await a.rest('POST', '/items/pages', { title: 'WS', slug, status: 'published' })).data;
  check('create page', !!page?.id, page);

  let r = await a.rest('POST', `/items/pages/${page.id}/comments`, { text: 'Hello team' });
  check('post comment', r.status === 200 && r.data?.text === 'Hello team', r);
  r = await a.rest('GET', `/items/pages/${page.id}/comments`);
  check('list comments', r.status === 200 && r.data?.length === 1, r.data);
  const commentId = r.data[0].id;
  r = await a.rest('PATCH', `/comments/${commentId}`, { text: 'Updated comment' });
  check('patch comment', r.status === 200 && r.data?.text === 'Updated comment', r);

  r = await a.rest('GET', '/notifications');
  check('list notifications', r.status === 200, r);

  r = await a.rest('POST', '/presets', {
    collection: 'pages',
    name: `e2e-${rnd()}`,
    layout: { sort: '-date_created', search: '', conditions: [] },
  });
  check('create preset', r.status === 200 && r.data?.name, r.data);
  const presetId = r.data.id;
  r = await a.rest('GET', '/presets', { collection: 'pages' });
  check('list presets', r.data?.some((p) => p.id === presetId || String(p.id) === String(presetId)), r.data);

  r = await a.rest('GET', '/dashboard');
  check('dashboard layout', r.status === 200 && r.data?.layout?.panels?.length, r.data);
  r = await a.rest('GET', '/insights');
  check('insights', r.status === 200 && Array.isArray(r.data?.collections), r.data);

  r = await a.rest('DELETE', `/presets/${presetId}`);
  check('delete preset', r.status === 204, r.status);
}
