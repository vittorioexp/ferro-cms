// Comments, notifications, presets, dashboard, insights.
import { rnd, uuid } from './lib.mjs';

async function mkUser(a, role) {
  const email = `ws${rnd()}@example.com`;
  const user = (await a.rest('POST', '/users', { email, password: 'workspace-pass-1', role })).data;
  const token = (await a.rest('POST', '/auth/login', { email, password: 'workspace-pass-1' }, null)).data?.access_token;
  return { id: user?.id, token };
}

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

  // mentions: only users who can open the item are notified; unknown ids and odd text are harmless
  const editor = await mkUser(a, '0199a000-0000-7000-8000-00000000e001');
  const noRole = (await a.rest('POST', '/roles', { name: `nobody-${rnd()}` })).data?.id;
  const nobody = await mkUser(a, noRole);
  r = await a.rest('POST', `/items/pages/${page.id}/comments`, { text: `ping @${editor.id} @${nobody.id} @${uuid()} @${'é'.repeat(40)}` });
  check('comment mentioning unknown users and multibyte text is accepted', r.status === 200, r);
  const editorNotes = (await a.rest('GET', '/notifications', undefined, editor.token)).data || [];
  check('a mentioned editor is notified', editorNotes.some((n) => n.item === page.id), editorNotes);
  const nobodyNotes = await a.rest('GET', '/notifications', undefined, nobody.token);
  check('a mentioned user without access gets nothing', nobodyNotes.status === 403 || (nobodyNotes.data || []).length === 0, nobodyNotes.json);

  r = await a.rest('POST', '/presets', {
    collection: 'pages',
    name: `e2e-${rnd()}`,
    layout: { sort: '-date_created', search: '', conditions: [] },
  });
  check('create preset', r.status === 200 && r.data?.name, r.data);
  const presetId = r.data.id;
  r = await a.rest('GET', '/presets?collection=pages');
  check('list presets', r.data?.some((p) => p.id === presetId || String(p.id) === String(presetId)), r.data);

  r = await a.rest('GET', '/dashboard');
  check('dashboard layout', r.status === 200 && r.data?.layout?.panels?.length, r.data);
  r = await a.rest('GET', '/insights');
  check('insights', r.status === 200 && Array.isArray(r.data?.collections), r.data);

  r = await a.rest('DELETE', `/presets/${presetId}`);
  check('delete preset', r.status === 204, r.status);
}
