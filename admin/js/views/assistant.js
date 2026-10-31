// AI assistant: natural-language queries and optional writes against CMS data.

import { api } from '../api.js';
import { add, button, fail, h } from '../dom.js';

export async function assistantView(root) {
  let cfg = { enabled: false, model: '' };
  try {
    cfg = (await api.get('/assistant/config')).data;
  } catch (err) {
    if (err.status === 401) {
      add(root, h('div', { class: 'page-head' }, h('h1', {}, 'Assistant')), h('div', { class: 'error-box' }, 'Sign in to use the assistant.'));
      return;
    }
  }

  if (!cfg.enabled) {
    add(
      root,
      h('div', { class: 'page-head' }, h('h1', {}, 'Assistant')),
      h('div', { class: 'card' }, h('p', {}, 'The AI assistant is not enabled on this server.'), h('p', { class: 'muted small' }, 'Set FERRO_AI_ENABLED=true and FERRO_AI_API_KEY on the API. See docs/AI_ASSISTANT.md.')),
    );
    return;
  }

  const messages = [];
  const allowWrites = h('input', { type: 'checkbox', id: 'ai-writes' });
  const log = h('div', { class: 'assistant-log' });
  const input = h('textarea', { rows: 3, placeholder: 'Ask about your content…', class: 'assistant-input' });
  const status = h('p', { class: 'muted small' }, `Model: ${cfg.model || 'default'}`);

  function renderLog() {
    log.replaceChildren();
    for (const m of messages) {
      const bubble = h('div', { class: `assistant-msg assistant-${m.role}` }, h('div', { class: 'assistant-role' }, m.role === 'user' ? 'You' : 'Assistant'), h('div', { class: 'assistant-text' }, m.content || ''));
      if (m.steps?.length) bubble.append(h('div', { class: 'assistant-steps muted small' }, `Tools: ${m.steps.join(', ')}`));
      log.append(bubble);
    }
    log.scrollTop = log.scrollHeight;
  }

  const sendBtn = button('Send', () => send().catch(fail), 'primary');

  async function send() {
    const text = input.value.trim();
    if (!text) return;
    input.value = '';
    messages.push({ role: 'user', content: text });
    renderLog();
    sendBtn.disabled = true;
    try {
      let ctxCol = null;
      try {
        ctxCol = sessionStorage.getItem('ferro_content_collection');
      } catch {
        /* ignore */
      }
      const payload = {
        messages: messages
          .filter((m) => (m.role === 'user' || m.role === 'assistant') && !m.error)
          .map(({ role, content }) => ({ role, content: content || '' })),
        allow_writes: allowWrites.checked,
        ...(ctxCol ? { context: { collection: ctxCol } } : {}),
      };
      const res = await api.post('/assistant/chat', payload);
      const { message, steps } = res.data;
      messages.push({ role: 'assistant', content: message.content || '', steps });
      renderLog();
    } catch (err) {
      messages.push({ role: 'assistant', content: err.message || String(err), error: true });
      renderLog();
    } finally {
      sendBtn.disabled = false;
      input.focus();
    }
  }

  add(
    root,
    h('div', { class: 'page-head' }, h('h1', {}, 'Assistant'), h('span', { class: 'spacer' }), status),
    h('div', { class: 'card assistant-panel' }, log, h('div', { class: 'assistant-compose' }, input, h('div', { class: 'assistant-actions' }, h('label', { class: 'check' }, allowWrites, ' Allow create, update and delete'), sendBtn))),
    h('p', { class: 'muted small' }, 'Answers use your account permissions. Writes only run when the checkbox is on.'),
  );

  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      send().catch(fail);
    }
  });
  renderLog();
  input.focus();
}
