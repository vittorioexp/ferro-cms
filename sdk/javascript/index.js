/**
 * Minimal Ferro API client (Node 18+ / modern browsers).
 *
 * @example
 * const ferro = new FerroClient('https://cms.example.com', { token: '…' });
 * const pages = await ferro.items('pages', { filter: { status: { _eq: 'published' } }, limit: 10 });
 */

export class FerroError extends Error {
  constructor(message, status, code) {
    super(message);
    this.name = 'FerroError';
    this.status = status;
    this.code = code;
  }
}

export class FerroClient {
  /**
   * @param {string} baseUrl API origin without trailing slash
   * @param {{ token?: string, fetch?: typeof fetch }} [opts]
   */
  constructor(baseUrl, opts = {}) {
    this.base = baseUrl.replace(/\/$/, '');
    this.token = opts.token || '';
    this.fetch = opts.fetch || globalThis.fetch;
    if (!this.fetch) throw new Error('fetch is not available; pass opts.fetch');
  }

  setToken(token) {
    this.token = token || '';
  }

  async #request(method, path, { query, body } = {}) {
    const url = new URL(path, this.base);
    if (query) {
      for (const [k, v] of Object.entries(query)) {
        if (v === undefined || v === null || v === '') continue;
        url.searchParams.set(k, typeof v === 'object' ? JSON.stringify(v) : String(v));
      }
    }
    const headers = { Accept: 'application/json' };
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    if (this.token) headers.Authorization = `Bearer ${this.token}`;
    const res = await this.fetch(url, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    let json = null;
    try {
      json = text ? JSON.parse(text) : null;
    } catch {
      /* ignore */
    }
    if (!res.ok) {
      const e = json?.errors?.[0];
      throw new FerroError(e?.message || `HTTP ${res.status}`, res.status, e?.extensions?.code);
    }
    return json;
  }

  /** @param {Record<string, unknown>} [query] */
  get(path, query) {
    return this.#request('GET', path, { query });
  }

  post(path, body, query) {
    return this.#request('POST', path, { body, query });
  }

  patch(path, body, query) {
    return this.#request('PATCH', path, { body, query });
  }

  del(path, query) {
    return this.#request('DELETE', path, { query });
  }

  /** List or read items. */
  async items(collection, query = {}) {
    const r = await this.get(`/items/${encodeURIComponent(collection)}`, query);
    return r.data;
  }

  /** Run aggregations (count, sum, avg, min, max) with optional group_by. */
  async aggregate(collection, body) {
    const r = await this.post(`/items/${encodeURIComponent(collection)}/aggregate`, body);
    return r.data;
  }

  /** Fetch OpenAPI document. */
  openapi() {
    return this.get('/server/openapi.json');
  }

  login(email, password, otp) {
    return this.post('/auth/login', { email, password, ...(otp ? { otp } : {}) }).then((r) => {
      this.setToken(r.data.access_token);
      return r.data;
    });
  }
}

export default FerroClient;
