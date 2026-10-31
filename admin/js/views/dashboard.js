// Dashboard home: configurable panels, insights, notifications preview.

import { api } from '../api.js';
import { add, button, fail, h, replace } from '../dom.js';
import * as S from '../schema.js';
import { isAdmin, session } from '../session.js';
import { shortId, truncate } from '../util.js';

const go = (hash) => {
  location.hash = hash;
};

async function panelInsights(el) {
  const r = await api.get('/insights');
  const d = r.data || {};
  const cols = (d.collections || []).slice(0, 12);
  replace(
    el,
    h('h3', {}, 'Insights'),
    h('p', { class: 'muted' }, `${d.unread_notifications ?? 0} unread notifications`),
    h(
      'div',
      { class: 'tiles tiles-sm' },
      ...cols.map((c) =>
        h(
          'a',
          { class: 'tile', href: `#/content/${encodeURIComponent(c.name)}` },
          h('strong', {}, S.collectionLabel(S.getCollection(c.name) || { name: c.name })),
          h('span', { class: 'muted' }, c.count === null || c.count === undefined ? '— items' : `${c.count} items`),
        ),
      ),
    ),
    d.activity_by_day?.length
      ? h(
          'table',
          { class: 'compact' },
          h('thead', {}, h('tr', {}, ['Day', 'Creates', 'Updates', 'Deletes'].map((t) => h('th', {}, t)))),
          h(
            'tbody',
            {},
            ...d.activity_by_day.slice(0, 7).map((row) =>
              h('tr', {}, h('td', {}, row.day), h('td', {}, String(row.creates)), h('td', {}, String(row.updates)), h('td', {}, String(row.deletes))),
            ),
          ),
        )
      : h('p', { class: 'muted' }, isAdmin() ? 'No activity in the last week yet.' : 'Activity trends are visible to administrators.'),
  );
}

async function panelRecentActivity(el, limit = 8) {
  if (!isAdmin()) {
    replace(el, h('h3', {}, 'Recent activity'), h('p', { class: 'muted' }, 'Administrators can see the global activity log here.'));
    return;
  }
  const r = await api.get('/insights/recent-activity', { limit });
  replace(
    el,
    h('h3', {}, 'Recent activity'),
    h(
      'table',
      { class: 'compact' },
      h('thead', {}, h('tr', {}, ['When', 'User', 'Action', 'Collection', 'Item'].map((t) => h('th', {}, t)))),
      h(
        'tbody',
        {},
        ...(r.data || []).map((a) =>
          h(
            'tr',
            {},
            h('td', {}, new Date(a.timestamp).toLocaleString()),
            h('td', {}, a.user ? shortId(a.user) : '—'),
            h('td', {}, a.action),
            h('td', {}, a.collection),
            h('td', { class: 'mono small' }, truncate(a.item, 28)),
          ),
        ),
      ),
    ),
    button('Open full activity log', () => go('#/activity'), 'sm'),
  );
}

async function panelCollections(el) {
  const cols = S.userCollections().filter((c) => !c.meta?.hidden);
  replace(
    el,
    h('h3', {}, 'Collections'),
    cols.length
      ? h(
          'div',
          { class: 'tiles' },
          cols.map((c) =>
            h('a', { class: 'tile', href: `#/content/${encodeURIComponent(c.name)}` }, h('strong', {}, S.collectionLabel(c)), h('span', { class: 'muted' }, `${(c.fields || []).length} fields`)),
          ),
        )
      : h('p', { class: 'muted' }, 'No collections yet.'),
  );
}

async function panelNotifications(el, limit = 6) {
  const r = await api.get('/notifications', { limit });
  const items = r.data || [];
  replace(
    el,
    h('h3', {}, 'Notifications'),
    items.length
      ? h(
          'ul',
          { class: 'notif-list' },
          ...items.map((n) =>
            h(
              'li',
              { class: n.read_at ? 'read' : 'unread' },
              h('strong', {}, n.subject),
              h('span', { class: 'muted small' }, new Date(n.date_created).toLocaleString()),
              n.message ? h('p', {}, truncate(n.message, 120)) : null,
              n.collection && n.item
                ? button('Open', () => go(`#/content/${encodeURIComponent(n.collection)}/${encodeURIComponent(n.item)}`), 'sm')
                : null,
            ),
          ),
        )
      : h('p', { class: 'muted' }, 'You are all caught up.'),
    button('Mark all read', () => api.post('/notifications/read-all').then(() => panelNotifications(el, limit).catch(fail)), 'sm'),
  );
}

async function renderPanel(panel, root) {
  const box = h('div', { class: 'card dashboard-panel' });
  root.append(box);
  const type = panel.type || 'collections';
  try {
    if (type === 'insights') await panelInsights(box);
    else if (type === 'recent_activity') await panelRecentActivity(box, panel.limit || 8);
    else if (type === 'notifications') await panelNotifications(box, panel.limit || 6);
    else await panelCollections(box);
  } catch (err) {
    replace(box, h('p', { class: 'error-box' }, err.message || String(err)));
  }
}

export async function dashboardHome(root) {
  const layout = (await api.get('/dashboard')).data?.layout || { panels: [{ type: 'insights' }, { type: 'collections' }] };
  const grid = h('div', { class: 'dashboard-grid' });
  add(root, h('div', { class: 'page-head' }, h('h1', {}, 'Dashboard'), h('span', { class: 'muted' }, session.me?.email || '')));
  add(root, grid);
  for (const panel of layout.panels || []) {
    await renderPanel(panel, grid);
  }
}

/** Bell + dropdown in the top bar. */
export function mountNotificationBell(topbar) {
  const badge = h('span', { class: 'notif-badge', hidden: true }, '0');
  const menu = h('div', { class: 'notif-menu', hidden: true, onClick: (e) => e.stopPropagation() });
  const btn = h(
    'button',
    {
      class: 'notif-bell',
      type: 'button',
      title: 'Notifications',
      onClick: (e) => {
        e.stopPropagation();
        if (menu.hidden) {
          menu.hidden = false;
          refresh().catch(fail);
        } else {
          menu.hidden = true;
        }
      },
    },
    '🔔',
    badge,
  );
  const wrap = h('div', { class: 'notif-wrap' }, btn, menu);
  topbar.insertBefore(wrap, topbar.querySelector('.spacer'));

  async function refresh() {
    const r = await api.get('/notifications', { limit: 12 });
    const unread = r.meta?.unread_count ?? 0;
    badge.textContent = String(unread);
    badge.hidden = unread === 0;
    replace(
      menu,
      ...(r.data || []).map((n) =>
        h(
          'button',
          {
            type: 'button',
            class: `notif-item${n.read_at ? '' : ' unread'}`,
            onClick: async () => {
              if (!n.read_at) await api.post(`/notifications/${n.id}/read`);
              menu.hidden = true;
              if (n.collection && n.item) go(`#/content/${encodeURIComponent(n.collection)}/${encodeURIComponent(n.item)}`);
              refresh().catch(fail);
            },
          },
          h('strong', {}, n.subject),
          h('span', { class: 'muted' }, truncate(n.message || '', 80)),
        ),
      ),
      button('Mark all read', () => api.post('/notifications/read-all').then(refresh).catch(fail), 'sm'),
    );
  }

  refresh().catch(() => {});
  setInterval(() => refresh().catch(() => {}), 60_000);
  document.addEventListener('click', () => {
    menu.hidden = true;
  });
}
