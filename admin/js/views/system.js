// Environment (schema hash, history, compare / promote) and activity log.

import { api, download } from '../api.js';
import { add, applyWithPreview, button, confirmDialog, fail, h, inputEl, labeled, replace, selectEl, toast } from '../dom.js';
import * as S from '../schema.js';
import { reloadCollections, session } from '../session.js';
import { buildSelection, exportFromRemote, formatImportReport, importBundle, transferBundleFiles } from '../sync.js';
import { shortId, truncate } from '../util.js';

/** Plan + apply an arbitrary snapshot to this environment, with preview. */
async function promote(title, snapshot) {
  const res = await applyWithPreview(title, (q) => api.post('/schema/apply', { snapshot, ...q }));
  if (res) {
    await reloadCollections();
    toast('Schema updated', 'ok');
    location.reload();
  }
}

export async function schemaView(root) {
  const info = session.info || {};
  const history = await api.get('/schema/history', { limit: 30 });

  const remote = { url: '', token: '' };
  const data = { collection: '', filter: '', strategy: 'overwrite', withDeps: true, prune: false };
  const dataResult = h('pre', { class: 'result mono small' }, '');

  async function runDataPromotion(dryRun) {
    if (!remote.url || !remote.token) throw new Error('Source URL and administrator token are required');
    if (!data.collection) throw new Error('Choose a collection to export');
    let selection;
    try {
      selection = buildSelection(data.collection, data.filter, data.withDeps);
    } catch (e) {
      throw new Error(`Invalid filter JSON: ${e.message}`);
    }
    dataResult.textContent = dryRun ? 'Exporting from source (preview)…' : 'Exporting and promoting…';
    const bundle = await exportFromRemote(remote.url, remote.token, [selection]);
    const itemCount = Object.values(bundle.items || {}).reduce((n, arr) => n + (Array.isArray(arr) ? arr.length : 0), 0);
    let transferred = 0;
    if (!dryRun) transferred = await transferBundleFiles(remote.url, remote.token, bundle);
    const report = await importBundle(bundle, {
      strategy: data.strategy,
      prune: data.prune,
      dry_run: dryRun,
      ignore_missing_fields: false,
      force: false,
    });
    const head = dryRun
      ? `Preview: ${itemCount} items, ${bundle.files?.length ?? 0} files in bundle.\n`
      : `Transferred ${transferred} file(s).\n`;
    dataResult.textContent = head + formatImportReport(report);
    if (!dryRun && report.committed) toast('Content promotion applied', 'ok');
    else if (report.ok && dryRun) toast('Dry run complete', 'ok');
    else if (!report.ok) toast('Promotion finished with errors', 'warn');
  }

  const compare = h(
    'div',
    { class: 'card' },
    h('h3', {}, 'Align this environment to another'),
    h('p', { class: 'muted' }, 'Reads the snapshot (model, roles, policies) of another environment, shows the plan to make this one identical, and applies it on confirmation. The other server must allow this origin (CORS).'),
    labeled('Source URL', inputEl('', (v) => (remote.url = v.trim().replace(/\/$/, '')), { placeholder: 'https://cms-dev.example.com' })),
    labeled('Source token (administrator)', inputEl('', (v) => (remote.token = v.trim()), { type: 'password', autocomplete: 'off' })),
    button(
      'Compare',
      async () => {
        if (!remote.url || !remote.token) throw new Error('URL and token are required');
        const res = await fetch(`${remote.url}/schema/snapshot`, { headers: { Authorization: `Bearer ${remote.token}` } });
        if (!res.ok) throw new Error(`Source answered HTTP ${res.status}`);
        const json = await res.json();
        await promote(`Align to ${remote.url}`, json.data);
      },
      'primary',
    ),
  );

  const userCols = S.userCollections().filter((c) => !c.meta?.hidden && !c.sync?.exclude);
  const withDepsBox = h('input', { type: 'checkbox', id: 'sync-deps' });
  withDepsBox.checked = true;
  withDepsBox.addEventListener('change', () => {
    data.withDeps = withDepsBox.checked;
  });
  const pruneBox = h('input', { type: 'checkbox', id: 'sync-prune' });
  pruneBox.addEventListener('change', () => {
    data.prune = pruneBox.checked;
  });
  const dataPromo = h(
    'div',
    { class: 'card' },
    h('h3', {}, 'Promote content from another server'),
    h(
      'p',
      { class: 'muted' },
      'Uses the source URL and token above. Exports items and file metadata, uploads missing binaries to this server, then imports. Same steps as ferro data push (see docs/DATA_SYNC.md).',
    ),
    labeled('Collection', selectEl(userCols.map((c) => ({ value: c.name, label: c.name })), '', (v) => (data.collection = v))),
    labeled('Filter (JSON, optional)', inputEl('', (v) => (data.filter = v), { placeholder: '{"slug":{"_eq":"home"}}' })),
    labeled(
      'Strategy',
      selectEl(
        [
          { value: 'overwrite', label: 'Overwrite' },
          { value: 'safe', label: 'Safe (conflicts if edited here)' },
          { value: 'insert-only', label: 'Insert only' },
        ],
        data.strategy,
        (v) => (data.strategy = v),
      ),
    ),
    h('label', { class: 'check' }, withDepsBox, ' Include related items (dependency closure)'),
    h('label', { class: 'check' }, pruneBox, ' Prune owned children removed at source'),
    h(
      'div',
      { class: 'row-actions' },
      button('Preview (dry run)', () => runDataPromotion(true).catch(fail)),
      button(
        'Promote content + files',
        async () => {
          if (!(await confirmDialog('Import selected content and file assets into this environment?'))) return;
          runDataPromotion(false).catch(fail);
        },
        'primary',
      ),
    ),
    dataResult,
  );

  const file = h('input', {
    type: 'file',
    accept: '.json,application/json',
    style: { display: 'none' },
    onChange: async (e) => {
      const f = e.target.files[0];
      if (!f) return;
      try {
        await promote(`Apply ${f.name}`, JSON.parse(await f.text()));
      } catch (err) {
        fail(err);
      }
      e.target.value = '';
    },
  });

  const rows = (history.data || []).map((hrow) =>
    h(
      'tr',
      {},
      h('td', {}, new Date(hrow.applied_at).toLocaleString()),
      h('td', { class: 'mono' }, `${shortId(hrow.from_hash)} → ${shortId(hrow.to_hash)}`),
      h('td', {}, String(hrow.plan?.changes?.length ?? 0)),
      h('td', { class: 'small' }, truncate((hrow.plan?.changes || []).map(describe).join('; '), 140)),
    ),
  );

  add(root, 
    h('div', { class: 'page-head' }, h('h1', {}, 'Environment')),
    h(
      'div',
      { class: 'card' },
      h('dl', {}, h('dt', {}, 'Name'), h('dd', {}, info.environment || '—'), h('dt', {}, 'Schema hash'), h('dd', { class: 'mono' }, info.schema_hash || '—'), h('dt', {}, 'Collections'), h('dd', {}, String(info.collections ?? S.userCollections().length))),
      h(
        'div',
        { class: 'row-actions' },
        button('Download snapshot (YAML)', () => download('/schema/snapshot', `snapshot-${info.environment || 'ferro'}.yaml`, { format: 'yaml' }).catch(fail)),
        button('Apply snapshot file (JSON)…', () => file.click()),
        file,
      ),
      h('p', { class: 'muted' }, 'Same hash = same model and permissions. Commit the YAML (or use "ferro schema pull") to version it in git.'),
    ),
    compare,
    dataPromo,
    h(
      'div',
      { class: 'card' },
      h('h3', {}, 'Schema history'),
      h('table', {}, h('thead', {}, h('tr', {}, ['When', 'Hash', 'Changes', 'Summary'].map((t) => h('th', {}, t)))), h('tbody', {}, rows.length ? rows : [h('tr', {}, h('td', { colspan: 4, class: 'empty' }, 'Nothing applied yet'))])),
    ),
  );
}

function describe(change) {
  const name = change.collection?.name || change.name || change.collection || change.field?.name || change.after?.name || change.role?.name || change.policy?.name || '';
  return `${change.op}${name ? ` ${typeof name === 'string' ? name : ''}` : ''}`.trim();
}

// ---------------------------------------------------------------------------
// activity
// ---------------------------------------------------------------------------

export async function activityView(root) {
  const state = { page: 1, collection: '', action: '' };
  const tbody = h('tbody');
  const pager = h('div', { class: 'pager' });

  async function load(page = state.page) {
    state.page = page;
    const conds = [];
    if (state.collection) conds.push({ collection: { _eq: state.collection } });
    if (state.action) conds.push({ action: { _eq: state.action } });
    const r = await api.get('/activity', {
      fields: '*,user.*',
      filter: conds.length ? { _and: conds } : undefined,
      sort: '-timestamp',
      limit: 50,
      page,
      meta: 'filter_count',
    });
    replace(tbody, 
      ...r.data.map((a) =>
        h(
          'tr',
          {},
          h('td', {}, new Date(a.timestamp).toLocaleString()),
          h('td', {}, a.user?.email || (a.user ? shortId(a.user) : 'system')),
          h('td', {}, a.action),
          h('td', {}, a.collection),
          h('td', { class: 'mono small' }, truncate(a.item, 36)),
          h('td', {}, a.origin || ''),
        ),
      ),
    );
    const total = r.meta?.filter_count ?? r.data.length;
    const pages = Math.max(1, Math.ceil(total / 50));
    replace(pager, 
      button('‹', () => state.page > 1 && load(state.page - 1).catch(fail), 'sm'),
      h('span', {}, ` ${state.page} / ${pages} · ${total} events `),
      button('›', () => state.page < pages && load(state.page + 1).catch(fail), 'sm'),
    );
  }

  add(root, 
    h(
      'div',
      { class: 'page-head' },
      h('h1', {}, 'Activity'),
      h('span', { class: 'spacer' }),
      selectEl([{ value: '', label: 'All collections' }, ...S.userCollections().map((c) => ({ value: c.name, label: c.name })), { value: 'ferro_files', label: 'files' }], '', (v) => {
        state.collection = v;
        load(1).catch(fail);
      }),
      selectEl([{ value: '', label: 'All actions' }, 'create', 'update', 'delete', 'import'], '', (v) => {
        state.action = v;
        load(1).catch(fail);
      }),
    ),
    h('div', { class: 'table-wrap' }, h('table', {}, h('thead', {}, h('tr', {}, ['When', 'User', 'Action', 'Collection', 'Item', 'Origin'].map((t) => h('th', {}, t)))), tbody)),
    pager,
  );
  await load(1);
}
