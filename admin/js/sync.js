// Cross-environment data promotion (content + file binaries) via /sync/*.

import { api, ApiError, currentToken, baseUrl } from './api.js';

async function remotePost(sourceUrl, sourceToken, path, body) {
  const res = await fetch(`${sourceUrl.replace(/\/$/, '')}${path}`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${sourceToken}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    /* not JSON */
  }
  if (!res.ok) {
    const msg = json?.errors?.[0]?.message || text || `HTTP ${res.status}`;
    throw new ApiError(msg, res.status);
  }
  return json?.data ?? json;
}

/** Export a bundle from another Ferro server (admin token on source). */
export async function exportFromRemote(sourceUrl, sourceToken, selections) {
  return remotePost(sourceUrl, sourceToken, '/sync/export', { selections });
}

/** Upload missing binaries to this environment before import. */
export async function transferBundleFiles(sourceUrl, sourceToken, bundle) {
  const files = bundle?.files || [];
  if (!files.length) return 0;
  const refs = files.map((f) => ({ id: f.id, checksum: f.checksum ?? null }));
  const missing = (await api.post('/sync/missing-files', refs)).data || [];
  const token = currentToken();
  const target = baseUrl();
  for (const id of missing) {
    const res = await fetch(`${sourceUrl.replace(/\/$/, '')}/sync/files/${id}`, {
      headers: { Authorization: `Bearer ${sourceToken}` },
    });
    if (!res.ok) throw new ApiError(`Failed to read file ${id} from source (HTTP ${res.status})`, res.status);
    const blob = await res.arrayBuffer();
    const up = await fetch(`${target}/sync/files/${id}`, {
      method: 'PUT',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/octet-stream',
      },
      body: blob,
    });
    if (!up.ok) {
      const t = await up.text();
      throw new ApiError(t || `Failed to upload file ${id} (HTTP ${up.status})`, up.status);
    }
  }
  return missing.length;
}

export async function importBundle(bundle, options) {
  return (await api.post('/sync/import', { bundle, options })).data;
}

export function buildSelection(collection, filterText, withDependencies) {
  const sel = { collection, with_dependencies: withDependencies };
  const raw = (filterText || '').trim();
  if (raw) sel.filter = JSON.parse(raw);
  return sel;
}

export function formatImportReport(report) {
  if (!report) return '';
  const lines = [];
  const mode = report.dry_run ? 'DRY RUN' : report.committed ? 'APPLIED' : report.ok ? 'OK' : 'FAILED';
  lines.push(`${report.source || 'bundle'} → ${report.target || 'this environment'}  (${mode})`);
  for (const [c, s] of Object.entries(report.summary || {})) {
    lines.push(`  ${c}: +${s.create} ~${s.update} -${s.delete} conflict ${s.conflict} = ${s.unchanged}`);
  }
  for (const w of report.warnings || []) lines.push(`  warn: ${w}`);
  for (const e of report.errors || []) lines.push(`  error: ${e}`);
  return lines.join('\n');
}
