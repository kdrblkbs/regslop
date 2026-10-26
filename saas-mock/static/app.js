'use strict';

const view = document.getElementById('view');
let timer = null;
let checkTimer = null;
let renderToken = 0;
let catalog = null;
let cachedRun = null;
let runTab = 'timeline';

// ---------- html: interpolations are escaped unless wrapped in raw() ----------

class Raw { constructor(s) { this.s = s; } }
const raw = (s) => new Raw(s);
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const fmt = (v) => (v instanceof Raw ? v.s : Array.isArray(v) ? v.map(fmt).join('') : v == null || v === false ? '' : esc(v));
const html = (strings, ...values) => raw(strings.reduce((out, s, i) => out + s + (i < values.length ? fmt(values[i]) : ''), ''));

async function api(path, options = {}) {
  const res = await fetch(path, {
    method: options.method || (options.body ? 'POST' : 'GET'),
    headers: { 'content-type': 'application/json' },
    body: options.body && JSON.stringify(options.body),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw Object.assign(new Error(data.error || res.statusText), { status: res.status, data });
  return data;
}

const getCatalog = async () => (catalog = catalog || (await api('/api/catalog')));

// ---------- formatting ----------

const providerName = (p) => (p === 'github' ? 'GitHub' : 'GitLab');
const mrName = (p) => (p === 'github' ? 'Pull request' : 'Merge request');
const repoName = (url) => url.replace(/\.git$/, '').split('/').pop();
const repoPath = (url) => url.replace(/^https:\/\/[^/]+\//, '').replace(/\.git$/, '');
const usd = (n) => `$${n.toFixed(2)}`;
const tokens = (n) => (n >= 1e6 ? `${(n / 1e6).toFixed(2)} M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)} k` : String(n));
const dateTime = (ms) => new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;
const lines = (text) => String(text || '').split('\n').map((s) => s.trim()).filter(Boolean);
const ext = (href, label, cls = '') => html`<a class="ext ${cls}" href="${href}" target="_blank" rel="noopener">${label}</a>`;

function ago(ms) {
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 45) return 'just now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  return d === 1 ? 'yesterday' : `${d} days ago`;
}

function duration(ms) {
  const s = Math.round(ms / 1000);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`;
}

function offset(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  return `+${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

const DOC_STATUS = {
  never_run: ['Not run yet', ''],
  running: ['Running', 'info live'],
  complete: ['Complete', 'ok'],
  complete_with_open_items: ['Open TODOs', 'warn'],
  incomplete: ['Incomplete', 'warn'],
  failed: ['Last run failed', 'err'],
};
const RUN_STATUS = {
  queued: ['Queued', 'info live'],
  running: ['Running', 'info live'],
  succeeded: ['Succeeded', 'ok'],
  failed: ['Failed', 'err'],
};
const REPO_STATUS = {
  ok: ['Found', 'ok'],
  missing: ['Missing', 'warn'],
  no_access: ['No access', 'err'],
  no_connection: ['No connection', 'err'],
  invalid: ['Invalid', 'err'],
};
const OVERALL = { complete: 'Complete', complete_with_open_items: 'Complete with open items', incomplete: 'Incomplete' };
const TOOL = { list_files: ['LIST', 'list'], read_file: ['READ', 'read'], write_file: ['WRITE', 'write'] };

const badge = ([label, cls]) => html`<span class="badge ${cls}">${label}</span>`;
const isLive = (status) => status === 'queued' || status === 'running';

// ---------- markdown and diff, just enough for status reports ----------

function inline(text) {
  return esc(text)
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\[\[(TODO-\d+)([^\]]*)\]\]/g, '<mark>[[$1$2]]</mark>');
}

function markdown(md) {
  let out = '';
  let inList = false;
  for (const line of md.split('\n')) {
    const isItem = line.startsWith('- ');
    if (inList && !isItem) { out += '</ul>'; inList = false; }
    const heading = line.match(/^(#{1,3}) (.*)/);
    if (heading) {
      const level = heading[1].length + 1;
      out += `<h${level}>${inline(heading[2])}</h${level}>`;
    } else if (isItem) {
      if (!inList) { out += '<ul>'; inList = true; }
      const task = line.match(/^- \[( |x)\] (.*)/);
      out += task
        ? `<li class="check"><input type="checkbox" disabled${task[1] === 'x' ? ' checked' : ''}><span>${inline(task[2])}</span></li>`
        : `<li>${inline(line.slice(2))}</li>`;
    } else if (line.trim()) {
      out += `<p>${inline(line)}</p>`;
    }
  }
  return raw(out + (inList ? '</ul>' : ''));
}

function diff(file) {
  const all = file.diff.split('\n');
  const rows = all.filter((l, i) => l || i < all.length - 1);
  return html`<div class="diff">
    <div class="diff-head"><span>${file.path}</span><span><span class="add">+${file.additions}</span> <span class="del">−${file.deletions}</span></span></div>
    <pre>${rows.map((l) => {
      const cls = l.startsWith('+++') || l.startsWith('---') || l.startsWith('@@') ? 'h' : l[0] === '+' ? 'a' : l[0] === '-' ? 'r' : '';
      return raw(`<span class="${cls}">${esc(l).replace(/\[\[(TODO-\d+)([^\]]*)\]\]/g, '<mark>[[$1$2]]</mark>') || ' '}</span>`);
    })}</pre>
  </div>`;
}

// ---------- shared pieces ----------

function runsTable(runs, { showProduct = true } = {}) {
  if (!runs.length) return html`<div class="empty">No runs yet.</div>`;
  return html`<div class="table-wrap"><table>
    <thead><tr><th>Run</th>${showProduct ? html`<th>Product</th>` : ''}<th>Document</th><th>Status</th><th>Result</th><th>Started by</th><th class="num">Iterations</th><th class="num">Est. cost</th><th>Started</th></tr></thead>
    <tbody>${runs.map((r) => html`<tr class="click" data-href="#/runs/${r.id}">
      <td class="mono">#${r.id}</td>
      ${showProduct ? html`<td>${r.product_name}</td>` : ''}
      <td>${r.title}</td>
      <td>${badge(RUN_STATUS[r.status])}</td>
      <td class="small">${isLive(r.status)
        ? html`<span class="muted">${r.phase}</span>`
        : r.branch
          ? html`${r.merge_request ? html`<strong>${r.merge_request}</strong> · ` : ''}<span class="mono">${r.branch}</span>${r.open_tasks ? html` <span class="muted">· ${plural(r.open_tasks, 'TODO')}</span>` : ''}`
          : html`<span class="muted">—</span>`}</td>
      <td class="small muted">${r.triggered_by}</td>
      <td class="num">${r.kind === 'onboarding' ? '—' : r.iteration}</td>
      <td class="num">${r.kind === 'onboarding' ? '—' : usd(r.cost_usd)}</td>
      <td class="small muted" title="${dateTime(r.started_at)}">${ago(r.started_at)}</td>
    </tr>`)}</tbody>
  </table></div>`;
}

function todoRow(t, { showDocument = true } = {}) {
  return html`<div class="task">
    <div class="stack" style="gap:4px">
      <div class="row"><span class="todo">${t.todo}</span>${showDocument ? html`<span class="small muted">${t.product_name} · ${t.document_title}</span>` : ''}</div>
      <strong>${t.description}</strong>
      <div class="small muted">${t.reason}</div>
    </div>
    <div class="small stack" style="gap:6px">
      <span>Likely provided by: <strong>${t.role}</strong></span>
      <span>${ext(t.file_url, t.file, 'mono')}</span>
      <span class="muted">From <a href="#/runs/${t.run_id}">run #${t.run_id}</a>, ${ago(t.created_at)}</span>
    </div>
  </div>`;
}

function expiredAlert(count) {
  if (!count) return '';
  return html`<div class="alert warn">
    <span><strong>${plural(count, 'git connection')} expired.</strong> Runs for repositories behind ${count === 1 ? 'it' : 'them'} fail at clone.</span>
    <a class="btn small" href="#/settings">Reconnect</a>
  </div>`;
}

// ---------- pages ----------

async function overview(token) {
  const o = await api('/api/overview');
  const docs = o.documents;
  const complete = docs.filter((d) => d.document.status === 'complete').length;
  paint(token, html`
    <div class="page-head">
      <div><h1>Overview</h1><div class="sub">${o.org} · ${plural(o.products, 'product')} · ${plural(docs.length, 'document')}</div></div>
      <div class="actions"><a class="btn primary" href="#/products/new">Register product</a></div>
    </div>
    <div class="stack">
      ${expiredAlert(o.expired_connections)}
      <div class="grid grid-4">
        <div class="card tile"><div class="label">Products</div><div class="value">${o.products}</div><div class="hint"><a href="#/products">All products →</a></div></div>
        <div class="card tile"><div class="label">Documents without open TODOs</div><div class="value">${complete} / ${docs.length}</div><div class="hint">according to the latest status reports</div></div>
        <div class="card tile"><div class="label">Open TODOs</div><div class="value">${o.open_tasks}</div><div class="hint"><a href="#/todos">Placeholders waiting for people →</a></div></div>
        <div class="card tile"><div class="label">Runs, last 30 days</div><div class="value">${o.usage.runs}</div><div class="hint">${usd(o.usage.cost_usd)} Bedrock (est.)${o.running ? html` · <strong>${o.running} running</strong>` : ''}</div></div>
      </div>

      <div class="card">
        <div class="card-head"><h2>Documents</h2><span class="small muted">Status from the latest status report</span></div>
        <div class="card-body flush table-wrap"><table>
          <thead><tr><th>Product</th><th>Document</th><th>Status</th><th class="num">Open TODOs</th><th>Last run</th><th></th></tr></thead>
          <tbody>${docs.map(({ product_id, product_name, provider, document: d }) => html`<tr class="click" data-href="#/products/${product_id}">
            <td><strong>${product_name}</strong> <span class="chip">${providerName(provider)}</span></td>
            <td>${d.title}</td>
            <td>${badge(DOC_STATUS[d.status])}</td>
            <td class="num">${d.open_tasks || '—'}</td>
            <td class="small muted">${d.last_run_at ? ago(d.last_run_at) : 'never'}</td>
            <td class="num"><button class="btn small" data-action="run" data-product="${product_id}" data-doc="${d.kind}"${d.status === 'running' ? ' disabled' : ''}>Run</button></td>
          </tr>`)}</tbody>
        </table></div>
      </div>

      <div class="card">
        <div class="card-head"><h2>Recent runs</h2><a class="small" href="#/runs">All runs →</a></div>
        <div class="card-body flush">${runsTable(o.recent_runs)}</div>
      </div>
    </div>`);
  if (o.running) poll(token, () => overview(token), 2000);
}

async function products(token) {
  const list = await api('/api/products');
  paint(token, html`
    <div class="page-head">
      <div><h1>Products</h1><div class="sub">A product is one or more source repositories and one documentation repository, like the <code>[git]</code> section of <code>regslop_config.toml</code>.</div></div>
      <div class="actions"><a class="btn primary" href="#/products/new">Register product</a></div>
    </div>
    <div class="grid grid-3">${list.map((p) => html`<a class="card doc" href="#/products/${p.id}" style="color:inherit;text-decoration:none">
      <div class="spread"><h2>${p.name}</h2><span class="chip">${providerName(p.git.provider)}</span></div>
      <div class="muted">${p.description}</div>
      <div class="path">${plural(p.git.in_repos.length, 'source repo')} → ${repoPath(p.git.out_repo)}</div>
      <div class="row">${p.status === 'onboarding' ? badge(['Onboarding', 'info live']) : p.documents.map((d) => html`<span class="badge ${DOC_STATUS[d.status][1]}">${d.title}</span>`)}</div>
      <div class="meta"><span>${p.owner}</span><span>${p.bedrock.model.replace(/^eu\.anthropic\./, '')}</span></div>
    </a>`)}</div>`);
}

function configList(p, defaultSystemPrompt) {
  const custom = p.bedrock.system_prompt != null;
  return html`<dl class="kv">
    <dt>git.provider</dt><dd>${providerName(p.git.provider)}</dd>
    <dt>git.in_repos</dt><dd>${p.git.in_repos.map((r) => html`<div class="mono small">${r}</div>`)}</dd>
    <dt>git.out_repo</dt><dd class="mono small">${p.git.out_repo}</dd>
    <dt>git.token</dt><dd class="muted">From the git connection for the repositories' host and group</dd>
    <dt>bedrock.model</dt><dd class="mono small">${p.bedrock.model}</dd>
    <dt>bedrock.base_url</dt><dd class="mono small">${p.bedrock.base_url}</dd>
    <dt>bedrock.max_iterations</dt><dd>${p.bedrock.max_iterations}</dd>
    <dt>bedrock.max_tokens</dt><dd>${p.bedrock.max_tokens}</dd>
    <dt>bedrock.client_timeout_mins</dt><dd>${p.bedrock.client_timeout_mins}</dd>
    <dt>bedrock.system_prompt</dt><dd>${custom ? html`Custom <span class="muted">(${plural(lines(p.bedrock.system_prompt).length, 'line')})</span>` : 'Service default'}
      <details class="prompt" style="margin-top:6px"><summary>Show</summary><pre class="code">${custom ? p.bedrock.system_prompt : defaultSystemPrompt}</pre></details></dd>
    <dt>bedrock.task_prompt</dt><dd>One per document, see Edit configuration</dd>
  </dl>`;
}

async function product(id, token) {
  const { product: p, connections, runs, open_tasks, out_repo_exists, default_system_prompt } = await api(`/api/products/${id}`);
  const expired = connections.filter((c) => c.status === 'expired');
  const onboardingRun = runs.find((r) => r.kind === 'onboarding' && isLive(r.status));
  const blocked = p.status === 'onboarding' || !out_repo_exists;
  paint(token, html`
    <div class="crumbs"><a href="#/products">Products</a> › ${p.name}</div>
    <div class="page-head">
      <div>
        <div class="row"><h1>${p.name}</h1><span class="chip">${providerName(p.git.provider)}</span>${p.status === 'onboarding' ? badge(['Onboarding', 'info live']) : ''}</div>
        <div class="sub">${p.description}${p.description ? ' · ' : ''}registered by ${p.owner}</div>
      </div>
      <div class="actions"><a class="btn" href="#/products/${p.id}/edit">Edit configuration</a></div>
    </div>
    <div class="stack">
      ${expiredAlert(expired.length)}
      ${onboardingRun ? html`<div class="alert ok"><span><strong>Onboarding in progress.</strong> The documentation repository is being created.</span><a class="btn small" href="#/runs/${onboardingRun.id}">Follow onboarding</a></div>` : ''}
      ${!out_repo_exists && !onboardingRun ? html`<div class="alert err"><span><strong>The documentation repository doesn't exist.</strong> Onboarding didn't finish. Runs need the repository.</span><button class="btn small primary" data-action="onboarding" data-product="${p.id}">Retry onboarding</button></div>` : ''}
      <div class="card card-body">
        <div class="flow">
          <div class="node"><div class="label">in_repos · read-only</div>${p.git.in_repos.map((r) => html`<span class="mono small" title="${r}">${repoPath(r)}</span>`)}</div>
          <div class="arrow" aria-hidden="true">→</div>
          <div class="node agent"><div class="label">regslop</div><span class="mono small">${p.bedrock.model}</span><span class="small muted">up to ${p.bedrock.max_iterations} iterations</span></div>
          <div class="arrow" aria-hidden="true">→</div>
          <div class="node"><div class="label">out_repo · branch per run</div><span class="mono small" title="${p.git.out_repo}">${repoPath(p.git.out_repo)}</span></div>
        </div>
      </div>

      <div class="grid grid-3">${p.documents.map((d) => html`<div class="card doc">
        <div class="spread"><h2>${d.title}</h2>${badge(DOC_STATUS[d.status])}</div>
        <div class="path">${repoName(p.git.out_repo)}/${d.path}/</div>
        <div class="meta"><span>${d.open_tasks ? plural(d.open_tasks, 'open TODO') : 'No open TODOs'}</span><span>Last run ${d.last_run_at ? ago(d.last_run_at) : 'never'}</span></div>
        <div class="actions">
          <button class="btn primary small" data-action="run" data-product="${p.id}" data-doc="${d.kind}"${d.status === 'running' || blocked ? ' disabled' : ''}>${d.status === 'running' ? 'Running…' : 'Run regslop'}</button>
          ${d.last_run_id ? html`<a class="btn small" href="#/runs/${d.last_run_id}">Last run</a>` : ''}
          <a class="btn small" href="/api/products/${p.id}/regslop_config.toml?document=${d.kind}" target="_blank" title="regslop_config.toml for this document, for running the CLI locally">Export config</a>
        </div>
      </div>`)}</div>

      <div class="stack">
        <div class="card">
          <div class="card-head"><h2>Configuration</h2><a class="small" href="#/products/${p.id}/edit">Edit →</a></div>
          ${configList(p, default_system_prompt)}
        </div>
        <div class="card">
          <div class="card-head"><h2>Open TODOs</h2><a class="small" href="#/todos">All open TODOs →</a></div>
          <div>${open_tasks.length ? open_tasks.map((t) => todoRow(t)) : html`<div class="empty">No placeholders in the latest status reports.</div>`}</div>
        </div>
      </div>

      <div class="card">
        <div class="card-head"><h2>Runs</h2></div>
        <div class="card-body flush">${runsTable(runs, { showProduct: false })}</div>
      </div>
    </div>`);
  if (runs.some((r) => isLive(r.status))) poll(token, () => product(id, token), 2000);
}

async function runs(token) {
  const list = await api('/api/runs');
  const live = list.filter((r) => isLive(r.status)).length;
  paint(token, html`
    <div class="page-head">
      <div><h1>Runs</h1><div class="sub">Each run is one regslop invocation for one document: sandbox, clone, agent loop, push.${live ? html` <strong>${live} running.</strong>` : ''}</div></div>
    </div>
    <div class="card">${runsTable(list)}</div>`);
  if (live) poll(token, () => runs(token), 2000);
}

function timeline(r, freshAfter) {
  const items = [];
  let section = null;
  for (const e of r.events) {
    const agent = e.kind === 'tool' || e.kind === 'message';
    const next = agent ? `Iteration ${e.iteration}` : 'Service';
    if (next !== section) {
      section = next;
      items.push(html`<li class="iter">${section}</li>`);
    }
    const [label, cls] = e.kind === 'tool' ? TOOL[e.tool] : e.kind === 'message' ? ['MODEL', 'model'] : e.kind === 'error' ? ['ERROR', 'error'] : ['SERVICE', ''];
    items.push(html`<li class="${e.kind}${e.seq > freshAfter ? ' fresh' : ''}">
      <span class="t">${offset(e.at - r.started_at)}</span><span class="k ${cls}">${label}</span><span class="d${e.kind === 'tool' ? ' mono' : ''}">${e.detail}</span>
    </li>`);
  }
  if (isLive(r.status)) {
    items.push(html`<li><span class="t"></span><span class="k model">…</span><span class="d muted">working</span></li>`);
  }
  return html`<ul class="timeline">${items}</ul>`;
}

function gitResult(r) {
  const res = r.result;
  const g = res.git;
  const mr = mrName(r.provider);
  return html`<div class="card">
    <div class="card-head"><h2>Result in git</h2><span class="small muted mono">${repoPath(g.repo)}</span></div>
    <dl class="gitlinks">
      <dt>Branch</dt><dd>${ext(g.branch_url, g.branch, 'mono')}</dd>
      <dt>Commit</dt><dd>${ext(g.commit_url, g.commit.slice(0, 8), 'mono')}</dd>
      ${g.report_url ? html`<dt>Status report</dt><dd>${ext(g.report_url, 'REGULATORY_STATUS_REPORT.md', 'mono')}</dd>` : ''}
      ${r.kind === 'onboarding' ? '' : html`<dt>${mr}</dt><dd>${res.merge_request
        ? html`${ext(res.merge_request.url, `Draft ${mr.toLowerCase()} ${res.merge_request.reference}`)} <span class="chip">preview</span>`
        : html`<span class="todo-slot">Not created. regslop doesn't open ${mr.toLowerCase()}s yet. Once it does, the ${mr.toLowerCase()} is linked here.</span>`}</dd>`}
    </dl>
  </div>`;
}

async function run(id, tab, token, refresh = false) {
  const previous = cachedRun && cachedRun.id === Number(id) ? cachedRun : null;
  let r;
  if (refresh && previous) {
    r = await api(`/api/runs/${id}?after=${previous.events.length}`);
    r.events = previous.events.concat(r.events);
  } else {
    r = await api(`/api/runs/${id}`);
  }
  const resultTab = r.kind === 'onboarding' ? 'changes' : 'report';
  if (tab) runTab = tab;
  else if (!previous) runTab = r.status === 'succeeded' ? resultTab : 'timeline';
  else if (isLive(previous.status) && r.status === 'succeeded') runTab = resultTab;
  const freshAfter = refresh && previous ? previous.events.length : Infinity;
  cachedRun = r;

  const res = r.result;
  const onboarding = r.kind === 'onboarding';
  const elapsed = (r.finished_at || Date.now()) - r.started_at;
  const tabs = [['timeline', `Timeline · ${r.events.length}`]];
  if (res && !onboarding) tabs.push(['report', 'Status report'], ['changes', `Changes · ${res.files.length}`], ['todos', `TODOs · ${r.tasks.length}`]);
  if (res && onboarding) tabs.push(['changes', `Files · ${res.files.length}`]);
  if (!tabs.some(([key]) => key === runTab)) runTab = 'timeline';

  let body;
  if (runTab === 'report') body = html`<div class="markdown">${markdown(res.report_md)}</div>`;
  else if (runTab === 'changes') body = res.files.map(diff);
  else if (runTab === 'todos') body = r.tasks.length ? r.tasks.map((t) => todoRow(t, { showDocument: false })) : html`<div class="empty">No placeholders in this run.</div>`;
  else body = timeline(r, freshAfter);

  const nearBottom = window.innerHeight + window.scrollY >= document.body.scrollHeight - 120;
  paint(token, html`
    <div class="crumbs"><a href="#/products">Products</a> › <a href="#/products/${r.product_id}">${r.product_name}</a> › Run #${r.id}</div>
    <div class="page-head">
      <div>
        <div class="row"><h1>${r.title}</h1>${badge(RUN_STATUS[r.status])}</div>
        <div class="sub">${r.product_name} · started by ${r.triggered_by} · ${dateTime(r.started_at)} · ${isLive(r.status) ? 'running for' : 'took'} ${duration(elapsed)}</div>
      </div>
      <div class="actions">
        ${onboarding
          ? html`<a class="btn" href="#/products/${r.product_id}">Go to product</a>`
          : html`<button class="btn" data-action="run" data-product="${r.product_id}" data-doc="${r.kind.document}"${isLive(r.status) ? ' disabled' : ''}>Run again</button>`}
      </div>
    </div>
    <div class="stack">
      ${r.status === 'failed' ? html`<div class="alert err"><div class="stack" style="gap:6px"><strong>The run failed before the agent started.</strong><span class="mono small">${r.error}</span></div><a class="btn small" href="#/settings">Check git connections</a></div>` : ''}
      ${res && onboarding ? html`<div class="alert ok"><span><strong>Documentation repository created.</strong> It contains a README with the conventions and ${plural(res.files.length - 1, 'chapter template')}. The product is ready for its first run.</span><a class="btn small primary" href="#/products/${r.product_id}">Start the first run</a></div>` : ''}
      ${res && !onboarding ? html`<div class="alert ok"><span>Overall status: <strong>${OVERALL[res.overall]}</strong>${r.tasks.length ? html`, ${plural(r.tasks.length, 'placeholder')} waiting for input` : ''}.</span></div>` : ''}
      ${res ? gitResult(r) : ''}
      ${onboarding ? '' : html`<div class="card stats">
        <div><div class="label">Iteration</div><div class="value">${r.iteration} <span class="faint small">/ ${r.max_iterations}</span></div><div class="bar" style="margin-top:6px"><span style="width:${(r.iteration / r.max_iterations) * 100}%"></span></div></div>
        <div><div class="label">Input tokens</div><div class="value">${tokens(r.input_tokens)}</div></div>
        <div><div class="label">Output tokens</div><div class="value">${tokens(r.output_tokens)}</div></div>
        <div><div class="label">Context size</div><div class="value">${tokens(r.context_tokens)}</div></div>
        <div><div class="label">Est. Bedrock cost</div><div class="value">${usd(r.cost_usd)}</div></div>
      </div>`}
      <div class="card">
        <div class="tabs" role="tablist">${tabs.map(([key, label]) => html`<button role="tab" aria-selected="${key === runTab}" class="${key === runTab ? 'active' : ''}" data-action="tab" data-run="${r.id}" data-tab="${key}">${label}</button>`)}</div>
        <div>${body}</div>
      </div>
      <p class="small faint">${onboarding ? '' : html`Model: <span class="mono">${r.model}</span>`}${isLive(r.status) ? html` · ${r.phase}` : ''}</p>
    </div>`);
  if (refresh && nearBottom && isLive(r.status) && runTab === 'timeline') window.scrollTo(0, document.body.scrollHeight);
  if (isLive(r.status)) poll(token, () => run(id, tab, token, true), 700);
}

async function todos(token) {
  const all = await api('/api/tasks');
  const groups = new Map();
  for (const t of all) {
    const key = `${t.product_name} · ${t.document_title}`;
    if (!groups.has(key)) groups.set(key, { run: t.run_id, items: [] });
    groups.get(key).items.push(t);
  }
  paint(token, html`
    <div class="page-head">
      <div><h1>Open TODOs</h1><div class="sub">Placeholders from the latest status report of each document. They need information that isn't in the repositories; fill them in on the branch in git, during review.</div></div>
    </div>
    <div class="stack">${groups.size
      ? [...groups].map(([title, g]) => html`<div class="card">
          <div class="card-head"><h2>${title}</h2><a class="small" href="#/runs/${g.run}">Run #${g.run} →</a></div>
          <div>${g.items.map((t) => todoRow(t, { showDocument: false }))}</div>
        </div>`)
      : html`<div class="card empty">No open TODOs.</div>`}
    </div>`);
}

// ---------- register and edit ----------

function formBody(form) {
  const d = new FormData(form);
  const custom = d.get('system_prompt_mode') === 'custom';
  return {
    name: String(d.get('name') || ''),
    description: String(d.get('description') || ''),
    git: { provider: d.get('provider'), in_repos: lines(d.get('in_repos')), out_repo: String(d.get('out_repo') || '').trim() },
    bedrock: {
      model: String(d.get('model') || ''),
      base_url: String(d.get('base_url') || '').trim(),
      max_iterations: Number(d.get('max_iterations')),
      max_tokens: Number(d.get('max_tokens')),
      client_timeout_mins: Number(d.get('client_timeout_mins')),
      system_prompt: custom ? String(d.get('system_prompt') || '') : null,
    },
    documents: d.getAll('documents').map((kind) => {
      const prompt = form.elements[`task_prompt_${kind}`];
      return { kind, task_prompt: prompt ? prompt.value : null };
    }),
    onboarding: d.get('onboarding') === 'on',
  };
}

function tomlPreview(body) {
  const q = (s) => JSON.stringify(s);
  const docs = body.documents.map((d) => catalog.documents.find((c) => c.kind === d.kind).slug);
  return `[git]
provider = ${q(body.git.provider)}
token = "<from the git connection>"
in_repos = [
${body.git.in_repos.map((r) => `    ${q(r)},`).join('\n')}
]
out_repo = ${q(body.git.out_repo)}

[bedrock]
token = "<managed by the service>"
max_iterations = ${body.bedrock.max_iterations}
client_timeout_mins = ${body.bedrock.client_timeout_mins}
max_tokens = ${body.bedrock.max_tokens}
base_url = ${q(body.bedrock.base_url)}
model = ${q(body.bedrock.model)}

system_prompt = '''${body.bedrock.system_prompt == null ? '<service default>' : `<custom, ${lines(body.bedrock.system_prompt).length} lines>`}'''
task_prompt = '''<one per document: ${docs.join(', ') || 'none selected'}>'''`;
}

function scaffoldPreview(form) {
  const kinds = new FormData(form).getAll('documents');
  const docs = catalog.documents.filter((c) => kinds.includes(c.kind));
  return html`<pre class="code">README.md
${docs.map((d) => `documents/${d.slug}/\n${d.chapters.map((c) => `  ${c}`).join('\n')}`).join('\n')}</pre>`;
}

async function checkRepos(form, editing) {
  const body = formBody(form);
  const target = document.getElementById('repo-checks');
  if (!target || !body.git.in_repos.length || !body.git.out_repo) return;
  const checks = await api('/api/repos/check', { body: body.git });
  if (!document.body.contains(target)) return;
  const outMissing = checks.some((c) => c.role === 'out' && c.status === 'missing');
  target.innerHTML = html`<ul class="checks">${checks.map((c) => html`<li>
    ${badge(REPO_STATUS[c.status])}<span class="mono">${repoPath(c.url)} <span class="faint">(${c.role === 'in' ? 'in_repos' : 'out_repo'})</span></span>
    ${c.status === 'ok' ? '' : html`<span class="msg">${c.message}</span>`}
  </li>`)}</ul>`.s;
  const box = document.getElementById('onboarding');
  box.hidden = !outMissing || editing;
  if (!box.hidden) document.getElementById('scaffold').innerHTML = scaffoldPreview(form).s;
}

async function productForm(id, token) {
  await getCatalog();
  const editing = id != null;
  const detail = editing ? await api(`/api/products/${id}`) : null;
  const p = detail ? detail.product : {
    name: '', description: '',
    git: {
      provider: 'github',
      in_repos: ['https://github.com/example-org/infusion-api.git', 'https://github.com/example-org/infusion-app.git'],
      out_repo: 'https://github.com/example-org/infusion-regulatory.git',
    },
    bedrock: { ...catalog.defaults },
    documents: [{ kind: 'test_concept' }],
  };
  const has = (kind) => p.documents.find((d) => d.kind === kind);
  const custom = p.bedrock.system_prompt != null;
  const models = catalog.models.includes(p.bedrock.model) ? catalog.models : [p.bedrock.model, ...catalog.models];

  paint(token, html`
    <div class="crumbs"><a href="#/products">Products</a> › ${editing ? html`<a href="#/products/${p.id}">${p.name}</a> › Configuration` : 'Register'}</div>
    <div class="page-head"><div><h1>${editing ? `Configure ${p.name}` : 'Register a product'}</h1>
      <div class="sub">The same values as <code>regslop_config.toml</code>. Tokens are not part of a product: git access comes from the git connections, the Bedrock token from the service.</div></div></div>
    <form data-form="product" data-id="${editing ? p.id : ''}" class="grid grid-2" style="align-items:start">
      <div class="stack">
        <div class="card card-body stack">
          <fieldset class="stack" style="gap:14px"><legend>Product</legend>
            <label class="field">Name<input type="text" name="name" required value="${p.name}" placeholder="e.g. Infusion Planner"></label>
            <label class="field">Description <span class="hint">Optional</span><input type="text" name="description" value="${p.description}" placeholder="What the product does"></label>
          </fieldset>
        </div>

        <div class="card card-body stack">
          <fieldset class="stack" style="gap:14px"><legend>Repositories <span class="section-label">[git]</span></legend>
            <div class="grid grid-2" style="gap:8px">
              ${['github', 'gitlab'].map((prov) => html`<label class="choice"><input type="radio" name="provider" value="${prov}"${p.git.provider === prov ? ' checked' : ''}><span><strong>${providerName(prov)}</strong><br><span class="small muted">${prov === 'github' ? 'github.com' : 'GitLab.com or self-managed'}</span></span></label>`)}
            </div>
            <label class="field">Source repositories <span class="mono small muted">in_repos</span><span class="hint">One HTTPS URL per line. Together they form the product; regslop only reads them.</span>
              <textarea name="in_repos" required>${p.git.in_repos.join('\n')}</textarea></label>
            <label class="field">Documentation repository <span class="mono small muted">out_repo</span><span class="hint">Its README defines the structure and conventions. Each run pushes a branch here.</span>
              <input type="url" name="out_repo" required value="${p.git.out_repo}"></label>
            <div><div class="spread"><strong class="small">Repository check</strong><button type="button" class="btn small" data-action="check">Check again</button></div>
              <div id="repo-checks" class="small muted" style="margin-top:8px">Checking…</div></div>
            <div id="onboarding" class="onboarding stack" style="gap:8px" hidden>
              <label class="check-row"><input type="checkbox" name="onboarding" checked>
                <span><strong>Create the documentation repository (onboarding)</strong><br>
                <span class="small muted">The repository doesn't exist yet. regslop creates it and pushes a README with documentation conventions and a template with instruction comments for every chapter of the selected documents:</span></span></label>
              <div id="scaffold"></div>
            </div>
          </fieldset>
        </div>

        <div class="card card-body stack">
          <fieldset class="stack" style="gap:8px"><legend>Documents</legend>
            <p class="small muted">Each document is one regslop run with its own <code>task_prompt</code>, generated from a template.${editing ? ' The prompts can be edited below.' : ' You can edit them after registering.'}</p>
            ${catalog.documents.map((d) => html`<div class="choice" style="display:block">
              <label class="check-row"><input type="checkbox" name="documents" value="${d.kind}"${has(d.kind) ? ' checked' : ''}>
                <span><strong>${d.title}</strong> <span class="mono small muted">documents/${d.slug}/</span><ul>${d.focus.slice(0, 3).map((f) => html`<li>${f}</li>`)}</ul></span></label>
              ${editing && has(d.kind) ? html`<details class="prompt" style="margin-top:8px"><summary>task_prompt</summary>
                <textarea name="task_prompt_${d.kind}" rows="14" style="margin-top:8px">${has(d.kind).task_prompt}</textarea>
                <p class="small muted" style="margin-top:4px"><code>{{date}}</code> is replaced with the run date.</p></details>` : ''}
            </div>`)}
          </fieldset>
        </div>

        <div class="card card-body stack">
          <fieldset class="stack" style="gap:14px"><legend>Model <span class="section-label">[bedrock]</span></legend>
            <div class="grid grid-2" style="gap:14px">
              <label class="field">model<select name="model">${models.map((m) => html`<option${m === p.bedrock.model ? ' selected' : ''}>${m}</option>`)}</select></label>
              <label class="field">base_url<input type="url" name="base_url" required value="${p.bedrock.base_url}"></label>
            </div>
            <div class="grid grid-3" style="gap:14px;grid-template-columns:repeat(auto-fit,minmax(140px,1fr))">
              <label class="field">max_iterations<input type="number" name="max_iterations" min="1" max="512" value="${p.bedrock.max_iterations}"></label>
              <label class="field">max_tokens<input type="number" name="max_tokens" min="256" max="64000" value="${p.bedrock.max_tokens}"></label>
              <label class="field">client_timeout_mins<input type="number" name="client_timeout_mins" min="1" max="60" value="${p.bedrock.client_timeout_mins}"></label>
            </div>
            <div class="stack" style="gap:8px"><strong>system_prompt</strong>
              <div class="grid grid-2" style="gap:8px">
                <label class="choice"><input type="radio" name="system_prompt_mode" value="default"${custom ? '' : ' checked'}><span><strong>Service default</strong><br><span class="small muted">Updated centrally for every product</span></span></label>
                <label class="choice"><input type="radio" name="system_prompt_mode" value="custom"${custom ? ' checked' : ''}><span><strong>Custom</strong><br><span class="small muted">Only for this product</span></span></label>
              </div>
              <textarea name="system_prompt" rows="14" id="system-prompt"${custom ? '' : ' hidden'}>${custom ? p.bedrock.system_prompt : catalog.default_system_prompt}</textarea>
            </div>
          </fieldset>
        </div>

        <p class="form-error" id="form-error" hidden></p>
        <div class="actions"><button class="btn primary">${editing ? 'Save configuration' : 'Register product'}</button><a class="btn" href="${editing ? `#/products/${p.id}` : '#/products'}">Cancel</a></div>
      </div>

      <div class="card" style="position:sticky;top:16px">
        <div class="card-head"><h2>regslop_config.toml</h2><span class="small muted">per document</span></div>
        <div class="card-body"><pre class="code" id="config-preview"></pre>
          <p class="small muted" style="margin-top:10px">After registering, every document can be exported as a complete config with both prompts, for running the CLI locally.</p></div>
      </div>
    </form>`);

  const form = view.querySelector('form');
  const update = () => { document.getElementById('config-preview').textContent = tomlPreview(formBody(form)); };
  const recheck = () => { clearTimeout(checkTimer); checkTimer = setTimeout(() => checkRepos(form, editing).catch(() => {}), 400); };
  form.addEventListener('input', (e) => {
    update();
    if (['in_repos', 'out_repo', 'provider'].includes(e.target.name)) recheck();
    if (e.target.name === 'documents' && !document.getElementById('onboarding').hidden) document.getElementById('scaffold').innerHTML = scaffoldPreview(form).s;
    if (e.target.name === 'system_prompt_mode') document.getElementById('system-prompt').hidden = e.target.value !== 'custom';
  });
  if (!editing) {
    form.querySelectorAll('input[name="provider"]').forEach((input) => input.addEventListener('change', () => {
      const [from, to] = input.value === 'gitlab'
        ? ['https://github.com/example-org/', 'https://gitlab.example.com/medtech/infusion/']
        : ['https://gitlab.example.com/medtech/infusion/', 'https://github.com/example-org/'];
      for (const field of [form.elements.in_repos, form.elements.out_repo]) field.value = field.value.split(from).join(to);
      update();
      recheck();
    }));
  }
  update();
  checkRepos(form, editing).catch(() => {});
}

const newProduct = (token) => productForm(null, token);
const editProduct = (id, token) => productForm(id, token);

// ---------- settings ----------

async function settings(token) {
  const [connections, s] = await Promise.all([api('/api/connections'), api('/api/settings'), getCatalog()]);
  paint(token, html`
    <div class="page-head"><div><h1>Settings</h1><div class="sub">Service-wide configuration, managed by the team that runs regslop.</div></div></div>
    <div class="stack">
      <div class="card">
        <div class="card-head"><h2>Git connections</h2><span class="small muted">The <code>git.token</code> for every product whose repositories are on that host and group</span></div>
        <div class="card-body flush table-wrap"><table>
          <thead><tr><th>Provider</th><th>Host</th><th>Group</th><th>Credential</th><th>Status</th><th></th></tr></thead>
          <tbody>${connections.map((c) => html`<tr>
            <td>${providerName(c.provider)}</td>
            <td class="mono">${c.host}</td>
            <td class="mono">${c.scope}</td>
            <td class="small muted">${c.kind}</td>
            <td>${c.status === 'connected' ? badge(['Connected', 'ok']) : badge(['Token expired', 'err'])}</td>
            <td class="num">${c.status === 'expired' ? html`<button class="btn small primary" data-action="reconnect" data-id="${c.id}">Reconnect</button>` : html`<span class="small muted">since ${ago(c.connected_at)}</span>`}</td>
          </tr>`)}</tbody>
        </table></div>
      </div>

      <div class="grid grid-2" style="align-items:start">
        <div class="card">
          <div class="card-head"><h2>Bedrock defaults</h2><span class="small muted">for new products</span></div>
          <dl class="kv">
            <dt>bedrock.token</dt><dd class="muted">Managed by the service, never stored per product</dd>
            <dt>bedrock.model</dt><dd class="mono small">${catalog.defaults.model}</dd>
            <dt>bedrock.base_url</dt><dd class="mono small">${catalog.defaults.base_url}</dd>
            <dt>bedrock.max_iterations</dt><dd>${catalog.defaults.max_iterations}</dd>
            <dt>bedrock.max_tokens</dt><dd>${catalog.defaults.max_tokens}</dd>
            <dt>bedrock.client_timeout_mins</dt><dd>${catalog.defaults.client_timeout_mins}</dd>
            <dt>bedrock.system_prompt</dt><dd><details class="prompt"><summary>Service default</summary><pre class="code">${catalog.default_system_prompt}</pre></details></dd>
          </dl>
        </div>
        <div class="card">
          <div class="card-head"><h2>Preview</h2></div>
          <div class="card-body stack" style="gap:10px">
            <label class="switch"><input type="checkbox" data-action="preview-mr"${s.preview_merge_requests ? ' checked' : ''}>
              <span><strong>Show merge requests</strong><br><span class="small muted">regslop doesn't open merge requests yet. Turn this on to see how runs will link to them once it does.</span></span></label>
          </div>
        </div>
      </div>
    </div>`);
}

// ---------- plumbing ----------

function paint(token, content) {
  if (token === renderToken) view.innerHTML = content.s;
}

function poll(token, fn, ms) {
  clearTimeout(timer);
  timer = setTimeout(() => { if (token === renderToken) fn().catch(showError); }, ms);
}

function toast(message) {
  document.querySelector('.toast')?.remove();
  const el = document.createElement('div');
  el.className = 'toast';
  el.setAttribute('role', 'status');
  el.textContent = message;
  document.body.append(el);
  setTimeout(() => el.remove(), 3600);
}

function showError(e) {
  view.innerHTML = html`<div class="alert err"><span><strong>Something went wrong.</strong> ${e.message}</span><a class="btn small" href="#/">Back to overview</a></div>`.s;
}

async function refreshChrome() {
  const o = await api('/api/overview');
  const count = document.getElementById('task-count');
  count.hidden = !o.open_tasks;
  count.textContent = o.open_tasks;
  document.getElementById('org').innerHTML = html`<strong>${o.org}</strong>${o.user}`.s;
}

const routes = [
  [/^#\/?$/, overview, 'overview'],
  [/^#\/products$/, products, 'products'],
  [/^#\/products\/new$/, newProduct, 'products'],
  [/^#\/products\/(\d+)$/, product, 'products'],
  [/^#\/products\/(\d+)\/edit$/, editProduct, 'products'],
  [/^#\/runs$/, runs, 'runs'],
  [/^#\/runs\/(\d+)(?:\/(timeline|report|changes|todos))?$/, run, 'runs'],
  [/^#\/todos$/, todos, 'todos'],
  [/^#\/settings$/, settings, 'settings'],
];

async function render() {
  clearTimeout(timer);
  clearTimeout(checkTimer);
  const hash = location.hash || '#/';
  const [re, page, nav] = routes.find(([pattern]) => pattern.test(hash)) || routes[0];
  const params = (hash.match(re) || []).slice(1);
  document.querySelectorAll('[data-nav]').forEach((a) => a.classList.toggle('active', a.dataset.nav === nav));
  const token = ++renderToken;
  try {
    await page(...params, token);
  } catch (e) {
    showError(e);
  }
  refreshChrome().catch(() => {});
}

async function startRun(productId, doc) {
  try {
    const { run_id } = await api(`/api/products/${productId}/runs`, { body: { document: doc } });
    location.hash = `#/runs/${run_id}`;
  } catch (e) {
    if (e.status === 409 && e.data.run_id) location.hash = `#/runs/${e.data.run_id}`;
    else toast(e.message);
  }
}

document.addEventListener('click', async (event) => {
  const action = event.target.closest('[data-action]');
  if (action) {
    const d = action.dataset;
    if (d.action === 'preview-mr') {
      await api('/api/settings', { method: 'PUT', body: { preview_merge_requests: action.checked } });
      toast(action.checked ? 'Runs now show their merge request (preview)' : 'Merge requests hidden again');
      return;
    }
    event.preventDefault();
    try {
      if (d.action === 'run') await startRun(d.product, d.doc);
      if (d.action === 'tab') location.hash = `#/runs/${d.run}/${d.tab}`;
      if (d.action === 'check') await checkRepos(action.closest('form'), Boolean(action.closest('form').dataset.id));
      if (d.action === 'reconnect') {
        await api(`/api/connections/${d.id}/reconnect`, { method: 'POST' });
        toast('Reconnected. Runs can clone again.');
        await render();
      }
      if (d.action === 'onboarding') {
        const { run_id } = await api(`/api/products/${d.product}/onboarding`, { method: 'POST' });
        location.hash = `#/runs/${run_id}`;
      }
    } catch (e) {
      toast(e.message);
    }
    return;
  }
  const row = event.target.closest('[data-href]');
  if (row && !event.target.closest('a, button, input')) location.hash = row.dataset.href;
});

document.addEventListener('submit', async (event) => {
  const form = event.target;
  event.preventDefault();
  if (form.dataset.form !== 'product') return;
  const body = formBody(form);
  const id = form.dataset.id;
  try {
    if (id) {
      await api(`/api/products/${id}`, { method: 'PUT', body: { ...body, onboarding: false } });
      toast('Configuration saved');
      location.hash = `#/products/${id}`;
    } else {
      const { product: created, onboarding_run } = await api('/api/products', { body });
      toast(onboarding_run ? `${created.name} registered, creating its documentation repository` : `${created.name} registered`);
      location.hash = onboarding_run ? `#/runs/${onboarding_run}` : `#/products/${created.id}`;
    }
  } catch (e) {
    const el = document.getElementById('form-error');
    el.textContent = e.message;
    el.hidden = false;
  }
});

window.addEventListener('hashchange', render);
render();
