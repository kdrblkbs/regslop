# regslop service (mock)

A clickable mock of regslop as an internal service, for discussing the idea. There is no real git
and no real model: runs are simulated, and all state lives in memory and resets on restart.

```sh
cargo run            # http://127.0.0.1:8080, or set PORT
cargo test
```

- `/`: the proposal. It covers what regslop is, how it works today, and what the service should
  offer.
- `/app`: the mocked service.

This is a standalone crate and is not part of the CLI's build. It uses one file from the parent
directory: `../regslop_config.example.toml` provides the default system prompt and the reference
for the task prompt template (a test checks that the template matches it).

## What the mock shows

| Service | regslop today |
|---|---|
| **Product**: 1..n source repos plus one documentation repo | `[git] in_repos`, `out_repo`, `provider` |
| Product model settings, prefilled with service defaults | `[bedrock] model`, `base_url`, `max_iterations`, `max_tokens`, `client_timeout_mins` |
| System prompt: service default or custom per product | `[bedrock] system_prompt` |
| One task prompt per document, from a template, editable (`{{date}}` filled in at run time) | `[bedrock] task_prompt` |
| Git connections per host and group | `[git] token` |
| **Onboarding**: create a missing documentation repo with a README and chapter templates | not in regslop |
| **Run**: one click per document, live timeline of iterations and tool calls | `harness::run`, `tools.rs` |
| Status report, diff and open TODOs per run | status report and placeholder rules in the system prompt |
| **Links to git**: branch, commit, report file, TODO files | regslop doesn't push yet (TODO) |
| Merge request link, behind *Settings → Preview* | regslop doesn't open merge requests yet (TODO) |
| *Export config* per document | a `regslop_config.toml` the CLI accepts as is |

The git links point at the configured hosts (e.g. `github.com/example-org/...`). Those
repositories don't exist, so the links show what a link would be rather than working.

## Things to try

- **Products → Register product**: the defaults point at two existing source repos and a
  documentation repo that doesn't exist yet. The repository check offers onboarding. Register to
  follow the onboarding run, then start the first document run.
- **Run regslop** on a document and watch it live (about 30–40 s). It ends on the status report,
  with links to the branch, the commit and the report in git.
- **Settings → Preview → Show merge requests** fills the merge request slot on finished runs.
- **Lab Connector**: its GitLab.com token has expired, so runs fail at clone. Use **Settings →
  Reconnect**.
- **Edit configuration** on a product changes the config values, including the prompts. **Export
  config** shows the result as `regslop_config.toml`.

## Layout

- `src/state.rs`: domain model (products, runs, TODOs, connections), in-memory store, seed data
- `src/sim.rs`: scripted runs and onboarding (live with delays, or replayed instantly for history)
- `src/content.rs`: prompts, onboarding scaffold, canned findings, diffs and status reports
- `src/api.rs`: JSON API and static files
- `static/`: proposal page, app shell, vanilla JS SPA (no build step, no external requests)

## API

| Method | Path | |
|---|---|---|
| GET | `/api/overview` | KPIs, documents, recent runs |
| GET | `/api/catalog` | document types, models, defaults, default system prompt |
| POST | `/api/repos/check` | `{provider, in_repos, out_repo}`: found, missing, no access, no connection |
| GET, POST | `/api/products` | list; register (`onboarding: true` creates a missing `out_repo`) |
| GET, PUT | `/api/products/{id}` | detail; update configuration |
| POST | `/api/products/{id}/runs` | `{"document": "test_concept"}` starts a run |
| POST | `/api/products/{id}/onboarding` | retry a failed onboarding |
| GET | `/api/products/{id}/regslop_config.toml?document=…` | config export for the CLI |
| GET | `/api/runs`, `/api/runs/{id}?after=N` | `after` returns only events with seq > N, for polling |
| GET | `/api/tasks` | open TODOs from the latest status reports |
| GET | `/api/connections` | git connections |
| POST | `/api/connections/{id}/reconnect` | |
| GET, PUT | `/api/settings` | `{"preview_merge_requests": bool}` |
