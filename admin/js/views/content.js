// Content: item listing (search, filters, sort, pagination, bulk delete) and item editor.

import { api } from '../api.js';
import { add, busy, button, confirmDialog, fail, h, modal, replace, selectEl, toast } from '../dom.js';
import { clearOptionCache, FormError, renderForm } from '../form.js';
import { onLeave, runLeave } from '../lifecycle.js';
import { realtime } from '../realtime.js';
import { itemActionsFor } from '../registry.js';
import * as S from '../schema.js';
import { buildFilter, FILTER_OPS, shortId, truncate } from '../util.js';
import { changeBanner } from './banner.js';

const PAGE_SIZE = 25;

function go(hash) {
  location.hash = hash;
}

const listHash = (name) => `#/content/${encodeURIComponent(name)}`;
const itemHash = (name, id) => `#/content/${encodeURIComponent(name)}/${encodeURIComponent(id)}`;

function rememberCollection(name) {
  try {
    sessionStorage.setItem('ferro_content_collection', name);
  } catch {
    /* private mode */
  }
}

function pickColumns(c) {
  const eligible = S.scalarFields(c).filter((f) => !['json', 'text', 'm2a_item'].includes(f.type) && !f.meta?.hidden);
  const cols = eligible.slice(0, 5);
  if (c.system_fields !== false) cols.push({ name: 'date_updated', type: 'datetime' });
  return cols;
}

export function renderCell(field, value) {
  if (value === null || value === undefined || value === '') return h('span', { class: 'muted' }, '—');
  switch (field.type) {
    case 'boolean':
      return value ? '✓' : '✗';
    case 'datetime':
      return new Date(value).toLocaleString();
    case 'm2o':
    case 'file':
      return typeof value === 'object' ? S.labelOf(S.relatedCollection(field), value) : `${shortId(value)}…`;
    case 'uuid':
      return shortId(value);
    case 'json':
      return truncate(JSON.stringify(value), 40);
    default:
      return truncate(typeof value === 'object' ? JSON.stringify(value) : value, 80);
  }
}

export async function contentList(root, { collection }) {
  rememberCollection(collection);
  const c = S.getCollection(collection);
  if (!c) throw new Error(`Collection "${collection}" not found`);
  if (c.meta?.singleton) return singletonRedirect(collection);

  const columns = pickColumns(c);
  const state = {
    page: 1,
    sort: c.system_fields !== false ? '-date_created' : '',
    search: '',
    conds: [],
    selected: new Set(),
  };

  const tbody = h('tbody');
  const thead = h('thead');
  const pager = h('div', { class: 'pager' });
  const conditions = h('div', { class: 'conditions' });
  const bulk = h('span');

  const fieldOptions = [{ value: 'id', label: 'id' }, ...S.scalarFields(c).map((f) => ({ value: f.name, label: f.name }))];

  function drawConditions() {
    replace(conditions, 
      ...state.conds.map((cond, i) =>
        h(
          'div',
          { class: 'cond' },
          selectEl(fieldOptions, cond.field, (v) => (cond.field = v)),
          selectEl(
            FILTER_OPS.map(([value, label]) => ({ value, label })),
            cond.op,
            (v) => {
              cond.op = v;
              drawConditions();
            },
          ),
          cond.op === '_null' || cond.op === '_nnull'
            ? null
            : h('input', { type: 'text', value: cond.value, onInput: (e) => (cond.value = e.target.value), onKeydown: (e) => e.key === 'Enter' && reload(1) }),
          button(
            '✕',
            () => {
              state.conds.splice(i, 1);
              drawConditions();
              reload(1);
            },
            'icon',
          ),
        ),
      ),
    );
  }

  function drawHead() {
    replace(thead, 
      h(
        'tr',
        {},
        h('th', { class: 'check-col' }),
        ...columns.map((f) => {
          const asc = state.sort === f.name;
          const desc = state.sort === `-${f.name}`;
          return h(
            'th',
            {
              class: 'sortable',
              onClick: () => {
                state.sort = asc ? `-${f.name}` : f.name;
                reload(1);
              },
            },
            S.fieldLabel(f),
            asc ? ' ▲' : desc ? ' ▼' : '',
          );
        }),
      ),
    );
  }

  async function reload(page = state.page) {
    state.page = page;
    state.selected.clear();
    drawHead();
    const expand = columns.filter((f) => f.type === 'm2o' || f.type === 'file').map((f) => `${f.name}.*`);
    const r = await api.get(`/items/${collection}`, {
      fields: [...S.listFieldsParam(c), ...expand].join(','),
      filter: buildFilter(state.conds),
      sort: state.sort,
      limit: PAGE_SIZE,
      page: state.page,
      search: state.search,
      meta: 'filter_count',
    });
    const total = r.meta?.filter_count ?? r.data.length;
    replace(tbody, 
      ...(r.data.length
        ? r.data.map((item) =>
            h(
              'tr',
              { class: 'clickable', onClick: () => go(itemHash(collection, item.id)) },
              h(
                'td',
                { class: 'check-col', onClick: (e) => e.stopPropagation() },
                h('input', {
                  type: 'checkbox',
                  onChange: (e) => {
                    if (e.target.checked) state.selected.add(item.id);
                    else state.selected.delete(item.id);
                    drawBulk();
                  },
                }),
              ),
              ...columns.map((f) => h('td', {}, renderCell(f, item[f.name]))),
            ),
          )
        : [h('tr', {}, h('td', { colspan: columns.length + 1, class: 'empty' }, 'No items'))]),
    );
    const pages = Math.max(1, Math.ceil(total / PAGE_SIZE));
    replace(pager, 
      button('‹', () => state.page > 1 && reload(state.page - 1).catch(fail), 'sm'),
      h('span', {}, ` ${state.page} / ${pages} · ${total} items `),
      button('›', () => state.page < pages && reload(state.page + 1).catch(fail), 'sm'),
    );
    drawBulk();
  }

  function drawBulk() {
    replace(bulk, 
      state.selected.size
        ? button(
            `Delete ${state.selected.size} selected`,
            async () => {
              if (!(await confirmDialog(`Delete ${state.selected.size} item(s)? This cannot be undone.`, { label: 'Delete', danger: true }))) return;
              await api.del(`/items/${collection}`, null, [...state.selected]);
              clearOptionCache();
              toast('Deleted');
              await reload(1);
            },
            'danger sm',
          )
        : null,
    );
  }

  const searchInput = h('input', {
    type: 'search',
    placeholder: 'Search…',
    onKeydown: (e) => {
      if (e.key === 'Enter') {
        state.search = e.target.value;
        reload(1).catch(fail);
      }
    },
  });

  let presets = [];
  const presetHost = h('span', { class: 'preset-host' });

  function applyPreset(p) {
    if (!p?.layout) return;
    state.sort = p.layout.sort || state.sort;
    state.search = p.layout.search || '';
    searchInput.value = state.search;
    state.conds = (p.layout.conditions || []).map((c) => ({ ...c }));
    drawConditions();
    reload(1).catch(fail);
  }

  function drawPresetSelect() {
    replace(
      presetHost,
      selectEl(
        [{ value: '', label: 'Saved views…' }, ...presets.map((p) => ({ value: String(p.id), label: p.name + (p.shared ? ' (shared)' : '') }))],
        '',
        (id) => {
          const p = presets.find((x) => String(x.id) === String(id));
          if (p) applyPreset(p);
        },
      ),
    );
  }

  async function loadPresets() {
    const r = await api.get('/presets', { collection });
    presets = r.data || [];
    drawPresetSelect();
  }

  async function savePreset() {
    const name = window.prompt('Name for this saved view (filters, sort, search)?');
    if (!name?.trim()) return;
    await api.post('/presets', {
      collection,
      name: name.trim(),
      layout: { sort: state.sort, search: state.search, conditions: state.conds },
      shared: false,
    });
    toast('View saved', 'ok');
    await loadPresets();
  }

  const banner = changeBanner('Refresh', () => reload(state.page));
  onLeave(
    realtime.subscribe(
      collection,
      (ev) => {
        if (ev.event === 'error') return;
        banner.show(ev.event === 'resync' ? 'The list may be out of date.' : `Someone ${ev.event === 'create' ? 'added' : ev.event === 'delete' ? 'deleted' : 'changed'} items in this collection.`);
      },
      { fields: 'id' },
    ),
  );

  add(root,
    banner.el,
    h(
      'div',
      { class: 'page-head' },
      h('h1', {}, S.collectionLabel(c)),
      h('span', { class: 'spacer' }),
      searchInput,
      presetHost,
      button('Save view', () => savePreset().catch(fail), 'sm'),
      button(
        '+ Filter',
        () => {
          state.conds.push({ field: fieldOptions[1]?.value || 'id', op: '_eq', value: '' });
          drawConditions();
        },
        'sm',
      ),
      button('Apply filters', () => reload(1).catch(fail), 'sm'),
      bulk,
      button('+ New item', () => go(itemHash(collection, '+')), 'primary'),
    ),
    conditions,
    h('div', { class: 'table-wrap' }, h('table', {}, thead, tbody)),
    pager,
  );
  await loadPresets();
  await reload(1);
}

async function singletonRedirect(collection) {
  const r = await api.get(`/items/${collection}`, { fields: 'id' });
  go(itemHash(collection, r.data?.id || '+'));
}

// ---------------------------------------------------------------------------
// item editor
// ---------------------------------------------------------------------------

export async function contentItem(root, { collection, id }) {
  rememberCollection(collection);
  const c = S.getCollection(collection);
  if (!c) throw new Error(`Collection "${collection}" not found`);
  const versioning = !!c.meta?.versioning?.enabled;
  const isNew = id === '+';
  let item = null;
  let draftMeta = null;
  if (!isNew) {
    const params = { fields: S.editFieldsParam(collection), meta: versioning ? 'draft' : undefined, version: versioning ? 'draft' : undefined };
    const r = await api.get(`/items/${collection}/${id}`, params);
    item = r.data;
    draftMeta = r.meta?.draft;
  }
  const form = renderForm(collection, item);
  const singleton = !!c.meta?.singleton;
  let flowActions = [];
  if (!isNew) {
    try {
      flowActions = (await api.get('/flow-actions', { collection })).data || [];
    } catch {
      /* automations are optional */
    }
  }

  let savedAt = 0; // our own writes echo back through the socket: ignore them
  const banner = changeBanner('Reload', () => rerender(id));
  if (!isNew) {
    onLeave(
      realtime.subscribe(
        collection,
        (ev) => {
          if (ev.event === 'error' || Date.now() - savedAt < 3000) return;
          if (ev.event === 'delete') {
            if (Array.isArray(ev.data) && ev.data.includes(id)) banner.show('This item was deleted.');
          } else if (ev.event === 'resync') banner.show('This item may have changed.');
          else banner.show('This item was changed by someone else. Reload to see the new version (your edits will be lost).');
        },
        { fields: 'id', filter: { id: { _eq: id } } },
      ),
    );
  }

  async function save(thenClose) {
    let payload;
    try {
      payload = form.collect();
    } catch (e) {
      if (e instanceof FormError) return toast(e.message, 'error');
      throw e;
    }
    savedAt = Date.now();
    const q = versioning ? { version: 'draft' } : {};
    const r = isNew ? await api.post(`/items/${collection}`, payload, q) : await api.patch(`/items/${collection}/${id}`, payload, q);
    savedAt = Date.now();
    clearOptionCache();
    toast('Saved', 'ok');
    if (thenClose && !singleton) return go(listHash(collection));
    const savedId = r.data?.id || id;
    if (isNew) go(itemHash(collection, savedId));
    else await rerender(savedId);
  }

  async function rerender(savedId) {
    runLeave();
    root.replaceChildren();
    await contentItem(root, { collection, id: savedId });
  }

  const saveBtn = button(versioning ? 'Save draft' : 'Save', () => busy(saveBtn, () => save(false)).catch(fail), 'primary');
  const saveCloseBtn = singleton ? null : button(versioning ? 'Save draft & close' : 'Save & close', () => busy(saveCloseBtn, () => save(true)).catch(fail));
  const publishBtn =
    !isNew && versioning
      ? button('Publish now', () =>
          busy(publishBtn, async () => {
            if (!(await confirmDialog('Publish this draft to the live site?', { label: 'Publish' }))) return;
            await api.post(`/items/${collection}/${id}/publish`, { force: true });
            toast('Published', 'ok');
            await rerender(id);
          }).catch(fail),
        )
      : null;
  const discardBtn =
    !isNew && versioning && draftMeta?.has_draft
      ? button('Discard draft', () =>
          busy(discardBtn, async () => {
            if (!(await confirmDialog('Discard unsaved draft changes?', { label: 'Discard', danger: true }))) return;
            await api.del(`/items/${collection}/${id}/draft`);
            toast('Draft discarded');
            await rerender(id);
          }).catch(fail),
        )
      : null;
  const versionsBtn =
    !isNew && versioning
      ? button('Published versions', () => showContentVersions(collection, id).catch(fail))
      : null;
  const actions = [
    saveBtn,
    saveCloseBtn,
    publishBtn,
    discardBtn,
    versionsBtn,
    ...flowActions.map((fl) =>
      button(
        `▶ ${fl.name}`,
        async () => {
          const r = await api.post(`/flows/${fl.id}/run`, { collection, keys: [id] });
          toast(r.meta.ok ? `${fl.name}: done` : `${fl.name} failed: ${r.meta.error}`, r.meta.ok ? 'ok' : 'error');
          if (r.meta.ok) await rerender(id);
        },
        'sm',
      ),
    ),
    ...(isNew ? [] : itemActionsFor(collection)).map((a) => button(a.label, () => a.run({ collection, id, item, form }, { api, schema: S, reload: () => rerender(id) }), 'sm')),
    !isNew && c.meta?.revisions !== false ? button('History', () => showRevisions(collection, id, c).catch(fail)) : null,
    !isNew && !singleton
      ? button(
          'Delete',
          async () => {
            if (!(await confirmDialog('Delete this item? This cannot be undone.', { label: 'Delete', danger: true }))) return;
            await api.del(`/items/${collection}/${id}`);
            clearOptionCache();
            toast('Deleted');
            go(listHash(collection));
          },
          'danger',
        )
      : null,
  ];

  let commentsEl = null;
  add(root, 
    h(
      'div',
      { class: 'page-head' },
      singleton ? null : h('a', { href: listHash(collection), class: 'back' }, `‹ ${S.collectionLabel(c)}`),
      h('h1', {}, isNew ? `New ${S.collectionLabel(c)}` : singleton ? S.collectionLabel(c) : S.labelOf(collection, item)),
      h('span', { class: 'spacer' }),
      actions,
    ),
    banner.el,
    versioning && !isNew
      ? h(
          'p',
          { class: 'muted' },
          draftMeta?.publish_at
            ? `Draft · scheduled publish ${new Date(draftMeta.publish_at).toLocaleString()}`
            : draftMeta?.has_draft
              ? 'Draft saved · live content unchanged until you publish'
              : 'Editing draft · live content unchanged until you publish',
        )
      : null,
    h('div', { class: 'card' }, form.el),
    !isNew ? (commentsEl = h('div', { class: 'card' })) : null,
    item?.date_updated || item?.date_created
      ? h('p', { class: 'muted' }, `Last change: ${new Date(item.date_updated || item.date_created).toLocaleString()}`)
      : null,
  );

  if (commentsEl) {
    mountCommentsPanel(commentsEl, collection, id).catch(fail);
  }
}

async function mountCommentsPanel(el, collection, id) {
  if (!el) return;
  const list = h('div', { class: 'comment-list' });
  const input = h('textarea', { rows: 3, placeholder: 'Add a comment… Use @user-id to mention someone.' });
  const post = button('Post comment', async () => {
    const text = input.value.trim();
    if (!text) return;
    await api.post(`/items/${collection}/${id}/comments`, { text });
    input.value = '';
    toast('Comment posted', 'ok');
    await refresh();
  }, 'primary sm');

  async function refresh() {
    const r = await api.get(`/items/${collection}/${id}/comments`);
    replace(
      list,
      ...(r.data || []).map((c) =>
        h('div', { class: 'comment' }, h('div', { class: 'muted small' }, new Date(c.date_created).toLocaleString()), h('p', {}, c.text)),
      ),
    );
  }

  replace(el, h('h3', {}, 'Comments'), list, input, h('div', { class: 'row-actions' }, post));
  await refresh();
}

async function showRevisions(collection, id, c) {
  const r = await api.get('/revisions', {
    fields: 'id,data,delta,activity.*',
    filter: { _and: [{ collection: { _eq: collection } }, { item: { _eq: id } }] },
    sort: '-id',
    limit: 50,
  });
  const restorable = (rev) => {
    if (!rev.data) return null;
    const out = {};
    for (const f of S.scalarFields(c)) if (f.name in rev.data) out[f.name] = rev.data[f.name];
    return out;
  };
  const rows = r.data.map((rev) => {
    const a = rev.activity || {};
    const data = restorable(rev);
    return h(
      'tr',
      {},
      h('td', {}, a.timestamp ? new Date(a.timestamp).toLocaleString() : ''),
      h('td', {}, a.action || ''),
      h('td', {}, a.origin || ''),
      h('td', { class: 'mono small' }, truncate(JSON.stringify(rev.delta ?? {}), 120)),
      h(
        'td',
        {},
        data
          ? button(
              'Restore fields',
              async () => {
                if (!(await confirmDialog('Restore the plain fields of this version? Relations are not touched.', { label: 'Restore' }))) return;
                await api.patch(`/items/${collection}/${id}`, data);
                toast('Restored', 'ok');
                dlg.close();
                location.reload();
              },
              'sm',
            )
          : null,
      ),
    );
  });
  const dlg = modal({
    title: 'History',
    wide: true,
    body: rows.length
      ? h('table', {}, h('thead', {}, h('tr', {}, ['When', 'Action', 'Origin', 'Changes', ''].map((t) => h('th', {}, t)))), h('tbody', {}, rows))
      : h('p', { class: 'muted' }, 'No revisions recorded.'),
    buttons: [{ label: 'Close' }],
  });
}

async function showContentVersions(collection, id) {
  const r = await api.get(`/items/${collection}/${id}/versions`);
  const rows = (r.data || []).map((v) =>
    h(
      'tr',
      {},
      h('td', {}, `#${v.version_number}`),
      h('td', {}, v.published_at ? new Date(v.published_at).toLocaleString() : ''),
      h(
        'td',
        {},
        button(
          'Load into draft',
          async () => {
            if (!(await confirmDialog('Replace the current draft with this published version?', { label: 'Load' }))) return;
            await api.post(`/items/${collection}/${id}/versions/${v.id}/restore`);
            toast('Loaded into draft', 'ok');
            dlg.close();
            location.reload();
          },
          'sm',
        ),
      ),
    ),
  );
  const dlg = modal({
    title: 'Published versions',
    wide: true,
    body: rows.length
      ? h('table', {}, h('thead', {}, h('tr', {}, ['Version', 'Published', ''].map((t) => h('th', {}, t)))), h('tbody', {}, rows))
      : h('p', { class: 'muted' }, 'No published versions yet (versions are created when you publish a draft).'),
    buttons: [{ label: 'Close' }],
  });
}
