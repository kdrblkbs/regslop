//! Fake regslop runs. A run is a scripted sequence of the events the real harness produces: sandbox
//! and clone (service side), then iterations of the model's tool calls, then push. Live runs play
//! the script with short delays, seeded runs replay it instantly.

use std::time::Duration;

use crate::content;
use crate::state::ConnectionStatus;
use crate::state::DocKind;
use crate::state::DocStatus;
use crate::state::Event;
use crate::state::EventKind;
use crate::state::GitResult;
use crate::state::MergeRequest;
use crate::state::Product;
use crate::state::ProductStatus;
use crate::state::Provider;
use crate::state::Run;
use crate::state::RunKind;
use crate::state::RunResult;
use crate::state::RunStatus;
use crate::state::Shared;
use crate::state::Store;
use crate::state::Task;
use crate::state::date_string;
use crate::state::now_ms;
use crate::state::repo_host;
use crate::state::repo_name;
use crate::state::repo_path;

pub(crate) const DEFAULT_MODEL: &str = "eu.anthropic.claude-sonnet-5-5";
pub(crate) const DEFAULT_BASE_URL: &str = "https://bedrock-runtime.eu-central-1.amazonaws.com";
pub(crate) const MODELS: &[&str] = &[
    "eu.anthropic.claude-sonnet-5-5",
    "eu.anthropic.claude-opus-5-5",
    "eu.anthropic.claude-haiku-5-5",
];

// the same names as in the cli's tools.rs
const TOOL_LIST: &str = "list_files";
const TOOL_READ: &str = "read_file";
const TOOL_WRITE: &str = "write_file";

// system prompt, tool specs and task message, sent with every request
const BASE_CONTEXT_TOKENS: u64 = 4_300;
// seeded runs replay the live script stretched to a realistic duration
const REPLAY_TIME_SCALE: u64 = 25;

/// list price estimate in USD per million tokens (input, output)
fn price(model: &str) -> (f64, f64) {
    if model.contains("opus") {
        (5.0, 25.0)
    } else if model.contains("haiku") {
        (1.0, 5.0)
    } else {
        (3.0, 15.0)
    }
}

pub(crate) fn cost_usd(model: &str, input_tokens: u64, output_tokens: u64) -> f64 {
    let (input, output) = price(model);
    (input_tokens as f64 * input + output_tokens as f64 * output) / 1_000_000.0
}

/// xorshift, deterministic per run so a run's numbers don't change between requests
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn range(&mut self, low: u64, high: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        low + self.0 % (high - low).max(1)
    }
}

pub(crate) struct Step {
    delay_ms: u64,
    /// true for the first block of a new model response, i.e. a new harness iteration
    starts_iteration: bool,
    kind: EventKind,
    tool: Option<&'static str>,
    detail: String,
    output_tokens: u64,
    /// size of the tool result that is appended to the context
    result_tokens: u64,
}

impl Step {
    fn phase(delay_ms: u64, detail: impl Into<String>) -> Self {
        Self {
            delay_ms,
            starts_iteration: false,
            kind: EventKind::Phase,
            tool: None,
            detail: detail.into(),
            output_tokens: 0,
            result_tokens: 0,
        }
    }

    fn tool(
        rng: &mut Rng,
        starts_iteration: bool,
        tool: &'static str,
        path: impl Into<String>,
        (result_low, result_high): (u64, u64),
    ) -> Self {
        let (delay_ms, output_tokens) = match (starts_iteration, tool) {
            (_, TOOL_WRITE) => (rng.range(1_800, 3_200), rng.range(1_400, 3_400)),
            (true, _) => (rng.range(700, 1_500), rng.range(60, 220)),
            (false, _) => (rng.range(80, 250), rng.range(30, 60)),
        };
        Self {
            delay_ms,
            starts_iteration,
            kind: EventKind::Tool,
            tool: Some(tool),
            detail: path.into(),
            output_tokens,
            result_tokens: rng.range(result_low, result_high),
        }
    }

    fn message(rng: &mut Rng, detail: impl Into<String>) -> Self {
        Self {
            delay_ms: rng.range(1_200, 2_200),
            starts_iteration: true,
            kind: EventKind::Message,
            tool: None,
            detail: detail.into(),
            output_tokens: rng.range(250, 600),
            result_tokens: 0,
        }
    }

    fn error(delay_ms: u64, detail: impl Into<String>) -> Self {
        Self {
            kind: EventKind::Error,
            ..Self::phase(delay_ms, detail)
        }
    }
}

fn is_frontend(repo: &str) -> bool {
    ["web", "frontend", "app", "ui"].iter().any(|s| repo.contains(s))
}

fn is_infra(repo: &str) -> bool {
    ["infra", "deploy", "ops"].iter().any(|s| repo.contains(s))
}

fn ci_file(provider: Provider) -> &'static str {
    match provider {
        Provider::Github => ".github/workflows/ci.yml",
        Provider::Gitlab => ".gitlab-ci.yml",
    }
}

fn source_files(repo: &str, kind: DocKind) -> Vec<&'static str> {
    let mut files = if is_infra(repo) {
        vec!["README.md", "terraform/main.tf", "k8s/values-production.yaml"]
    } else if is_frontend(repo) {
        vec!["README.md", "package.json", "vitest.config.ts", "playwright.config.ts"]
    } else {
        vec!["README.md", "Cargo.toml", "src/main.rs"]
    };
    let extra: &[&str] = match (kind, is_infra(repo), is_frontend(repo)) {
        (DocKind::TestConcept, false, true) => &["e2e/stock-correction.spec.ts", "src/components/StockTable.test.tsx"],
        (DocKind::TestConcept, false, false) => &["tests/api_orders.rs", "tests/common/mod.rs"],
        (DocKind::Architecture, false, false) => &["src/routes/mod.rs", "src/erp/client.rs", "docker-compose.yml"],
        (DocKind::BackupRecovery, true, _) => &["k8s/cronjob-pg-backup.yaml", "scripts/restore.sh"],
        (DocKind::BackupRecovery, false, false) => &["migrations/", "docker-compose.yml"],
        (DocKind::SecurityConcept, false, false) => &["src/auth/oidc.rs", "src/auth/permissions.rs", "deny.toml"],
        (DocKind::SecurityConcept, false, true) => &["src/auth/session.ts"],
        _ => &[],
    };
    files.extend(extra);
    files
}

fn branch_name(kind: RunKind, run_id: u64, at: u64) -> String {
    match kind {
        RunKind::Onboarding => "main".to_string(),
        RunKind::Document(kind) => format!("regslop/{}-{}-run{run_id}", kind.slug(), date_string(at)),
    }
}

fn commit_sha(run_id: u64) -> String {
    let hash = u128::from(run_id).wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C834);
    format!("{:032x}{:08x}", hash, (hash >> 96) as u32)
}

fn auth_error(repo: &str) -> String {
    format!(
        "git clone {repo} failed (exit status: 128): remote: HTTP Basic: Access denied. If a password was provided for Git authentication, the password was incorrect or you're required to use a token instead of a password. fatal: Authentication failed for '{repo}/'"
    )
}

/// why a run can't get its repositories, if it can't
fn clone_error(store: &Store, product: &Product, kind: RunKind) -> Option<(String, String)> {
    let repos: Vec<&String> = match kind {
        // onboarding only needs to create the documentation repository
        RunKind::Onboarding => vec![&product.git.out_repo],
        RunKind::Document(_) => product.repos().collect(),
    };
    for repo in repos {
        let expired = store
            .connection_for(repo)
            .is_none_or(|c| c.status == ConnectionStatus::Expired);
        if expired {
            return Some((repo.clone(), auth_error(repo)));
        }
        if kind != RunKind::Onboarding && !store.repo_exists(repo) {
            return Some((
                repo.clone(),
                format!("git clone {repo} failed (exit status: 128): remote: The project you were looking for could not be found. fatal: repository '{repo}/' not found"),
            ));
        }
    }
    None
}

fn sandbox_step() -> Step {
    Step::phase(400, "Provisioning ephemeral sandbox (agent has file tools only, no shell, no network)")
}

fn clone_failure(repo: &str, error: String) -> Vec<Step> {
    vec![
        sandbox_step(),
        Step::phase(900, format!("git clone {repo} → {}", repo_name(repo))),
        Step::error(1_400, error),
    ]
}

fn onboarding_script(product: &Product, rng: &mut Rng) -> Vec<Step> {
    let repo = &product.git.out_repo;
    let mut steps = vec![
        Step::phase(500, format!("Creating repository {} on {}", repo_path(repo), repo_host(repo))),
        Step::phase(rng.range(500, 900), "Generating README.md with documentation conventions"),
    ];
    for doc in &product.documents {
        steps.push(Step::phase(
            rng.range(400, 800),
            format!("Generating {} chapter templates in {}/", doc.kind.chapters().len(), doc.path),
        ));
    }
    steps.push(Step::phase(rng.range(600, 1_000), "git push origin main (initial commit)"));
    steps
}

fn document_script(product: &Product, kind: DocKind, rng: &mut Rng, branch: &str) -> Vec<Step> {
    let docs = repo_name(&product.git.out_repo);
    let doc_dir = format!("out/{docs}/documents/{}", kind.slug());
    let mut steps = vec![sandbox_step()];

    for repo in &product.git.in_repos {
        steps.push(Step::phase(rng.range(600, 1_400), format!("git clone {repo} → in/{}", repo_name(repo))));
    }
    steps.push(Step::phase(rng.range(500, 900), format!("git clone {} → out/{docs}", product.git.out_repo)));
    steps.push(Step::phase(
        300,
        format!("Agent started · {} · max {} iterations", product.bedrock.model, product.bedrock.max_iterations),
    ));

    // 1. understand the documentation repository
    steps.push(Step::tool(rng, true, TOOL_READ, format!("out/{docs}/README.md"), (1_200, 2_400)));
    steps.push(Step::tool(rng, false, TOOL_LIST, doc_dir.clone(), (150, 220)));
    for (i, chapter) in kind.chapters().iter().enumerate() {
        let starts_iteration = i == 0 || rng.range(0, 3) == 0;
        steps.push(Step::tool(rng, starts_iteration, TOOL_READ, format!("{doc_dir}/{chapter}"), (700, 2_600)));
    }

    // 2. understand the software
    for (i, repo) in product.git.in_repos.iter().enumerate() {
        steps.push(Step::tool(rng, i == 0, TOOL_LIST, format!("in/{}", repo_name(repo)), (600, 3_000)));
    }
    for repo in &product.git.in_repos {
        let name = repo_name(repo);
        let mut files = source_files(name, kind);
        files.insert(1, ci_file(product.git.provider));
        for (i, file) in files.iter().enumerate() {
            let tool = if file.ends_with('/') { TOOL_LIST } else { TOOL_READ };
            // the model often reads one file, thinks, then decides what to read next
            let starts_iteration = i == 0 || rng.range(0, 2) == 0;
            steps.push(Step::tool(rng, starts_iteration, tool, format!("in/{name}/{file}"), (400, 3_200)));
        }
    }

    // 3.+4. assess and apply
    let changes = content::changes(kind);
    steps.push(Step::message(rng, content::assessment(kind, product)));
    for (i, change) in changes.iter().enumerate() {
        let chapter = kind.chapters()[change.chapter];
        // the first write shares the turn with the assessment message
        steps.push(Step::tool(rng, i > 0, TOOL_WRITE, format!("{doc_dir}/{chapter}"), (15, 25)));
    }

    // 5. verify
    for (i, change) in changes.iter().enumerate() {
        let chapter = kind.chapters()[change.chapter];
        steps.push(Step::tool(rng, i == 0, TOOL_READ, format!("{doc_dir}/{chapter}"), (900, 2_400)));
    }

    // 6. report
    steps.push(Step::tool(rng, true, TOOL_WRITE, format!("{doc_dir}/REGULATORY_STATUS_REPORT.md"), (15, 25)));
    steps.push(Step::message(rng, content::final_message(kind)));

    steps.push(Step::phase(
        rng.range(600, 1_100),
        format!("Agent finished (stop reason: end_turn) · git push origin {branch}"),
    ));
    steps
}

fn successful_script(product: &Product, kind: RunKind, branch: &str, rng: &mut Rng) -> Vec<Step> {
    match kind {
        RunKind::Onboarding => onboarding_script(product, rng),
        RunKind::Document(doc) => document_script(product, doc, rng, branch),
    }
}

/// the steps of a run and whether it ends in failure
fn script(store: &Store, product: &Product, kind: RunKind, branch: &str, rng: &mut Rng) -> (Vec<Step>, bool) {
    match clone_error(store, product, kind) {
        Some((repo, error)) => (clone_failure(&repo, error), true),
        None => (successful_script(product, kind, branch, rng), false),
    }
}

fn new_run(store: &mut Store, product_id: u64, kind: RunKind, triggered_by: String, at: u64) -> Option<u64> {
    let id = store.next_id();
    let product = store.product_mut(product_id)?;
    match kind {
        RunKind::Onboarding => product.status = ProductStatus::Onboarding,
        RunKind::Document(kind) => {
            let doc = product.document_mut(kind)?;
            doc.status = DocStatus::Running;
            doc.last_run_id = Some(id);
            doc.last_run_at = Some(at);
        }
    }
    let run = Run {
        id,
        product_id,
        product_name: product.name.clone(),
        kind,
        title: kind.title(),
        triggered_by,
        status: RunStatus::Queued,
        phase: "Queued".to_string(),
        model: product.bedrock.model.clone(),
        iteration: 0,
        max_iterations: product.bedrock.max_iterations,
        input_tokens: 0,
        output_tokens: 0,
        context_tokens: if kind == RunKind::Onboarding { 0 } else { BASE_CONTEXT_TOKENS },
        cost_usd: 0.0,
        started_at: at,
        finished_at: None,
        error: None,
        events: Vec::new(),
        result: None,
    };
    store.runs.push(run);
    Some(id)
}

fn apply(run: &mut Run, step: Step, at: u64) {
    run.status = RunStatus::Running;
    if step.starts_iteration {
        run.iteration += 1;
        // every request re-sends the whole conversation
        run.input_tokens += run.context_tokens;
    }
    run.output_tokens += step.output_tokens;
    run.context_tokens += step.output_tokens + step.result_tokens;
    run.cost_usd = cost_usd(&run.model, run.input_tokens, run.output_tokens);
    run.phase = match step.kind {
        EventKind::Phase | EventKind::Error => step.detail.clone(),
        EventKind::Tool | EventKind::Message => format!("Iteration {} of {}", run.iteration, run.max_iterations),
    };
    run.events.push(Event {
        seq: run.events.len() + 1,
        at,
        iteration: run.iteration,
        kind: step.kind,
        tool: step.tool,
        detail: step.detail,
    });
}

fn fail(store: &mut Store, run_id: u64, at: u64) {
    let Some(run) = store.run_mut(run_id) else { return };
    run.status = RunStatus::Failed;
    run.finished_at = Some(at);
    run.error = run
        .events
        .iter()
        .rev()
        .find(|e| e.kind == EventKind::Error)
        .map(|e| e.detail.clone());
    let (product_id, kind) = (run.product_id, run.kind);
    let Some(product) = store.product_mut(product_id) else { return };
    match kind {
        RunKind::Onboarding => product.status = ProductStatus::Ready,
        RunKind::Document(kind) => {
            if let Some(doc) = product.document_mut(kind) {
                doc.status = DocStatus::Failed;
            }
        }
    }
}

fn finish(store: &mut Store, run_id: u64, branch: &str, at: u64) {
    let Some(run) = store.run(run_id) else { return };
    let Some(product) = store.product(run.product_id).cloned() else { return };
    let kind = run.kind;
    let provider = product.git.provider;
    let repo = &product.git.out_repo;
    let commit = commit_sha(run_id);
    let mut git = GitResult {
        repo: repo.clone(),
        branch: branch.to_string(),
        commit: commit.clone(),
        branch_url: provider.web_url(repo, &format!("tree/{branch}")),
        commit_url: provider.web_url(repo, &format!("commit/{commit}")),
        report_url: None,
    };

    let result = match kind {
        RunKind::Onboarding => {
            store.existing_repos.push(repo.clone());
            if let Some(p) = store.product_mut(product.id) {
                p.status = ProductStatus::Ready;
            }
            RunResult { overall: None, git, merge_request: None, files: content::scaffold(&product), report_md: None }
        }
        RunKind::Document(doc) => {
            let doc_dir = format!("documents/{}", doc.slug());
            let report_path = format!("{doc_dir}/REGULATORY_STATUS_REPORT.md");
            git.report_url = Some(provider.web_url(repo, &format!("blob/{branch}/{report_path}")));

            let todos = content::todos(doc);
            let overall = if todos.is_empty() { DocStatus::Complete } else { DocStatus::CompleteWithOpenItems };
            let report_md = content::report(&product, doc, &date_string(at), overall);
            let mut files: Vec<_> = content::changes(doc)
                .iter()
                .map(|change| content::changed_file(&product, &doc_dir, doc.chapters()[change.chapter], change))
                .collect();
            files.push(content::new_file(report_path, &report_md));

            // the report describes the current state only, so earlier placeholders are superseded
            store.tasks.retain(|t| !(t.product_id == product.id && t.document == doc));
            for (n, todo) in todos.iter().enumerate() {
                let file = format!("{doc_dir}/{}", doc.chapters()[todo.chapter]);
                let task = Task {
                    id: store.next_id(),
                    product_id: product.id,
                    product_name: product.name.clone(),
                    run_id,
                    document: doc,
                    document_title: doc.title(),
                    todo: format!("TODO-{:03}", n + 1),
                    file_url: provider.web_url(repo, &format!("blob/{branch}/{file}")),
                    file,
                    description: todo.description.to_string(),
                    role: todo.role.to_string(),
                    reason: todo.reason.to_string(),
                    created_at: at,
                };
                store.tasks.push(task);
            }
            if let Some(d) = store.product_mut(product.id).and_then(|p| p.document_mut(doc)) {
                d.status = overall;
                d.open_tasks = todos.len();
            }

            let (reference, url) = provider.merge_request_url(repo, Rng::new(run_id).range(40, 260));
            RunResult {
                overall: Some(overall),
                git,
                merge_request: Some(MergeRequest { reference, url }),
                files,
                report_md: Some(report_md),
            }
        }
    };

    if let Some(run) = store.run_mut(run_id) {
        run.status = RunStatus::Succeeded;
        run.finished_at = Some(at);
        run.phase = format!("Pushed {} to {branch}", &result.git.commit[..7]);
        run.events.push(Event {
            seq: run.events.len() + 1,
            at,
            iteration: run.iteration,
            kind: EventKind::Phase,
            tool: None,
            detail: run.phase.clone(),
        });
        run.result = Some(result);
    }
}

/// instantly plays a whole run with timestamps starting at `at`, for the seed history. history
/// doesn't depend on today's connections: `fails` decides whether the clone failed back then
pub(crate) fn replay(store: &mut Store, product_id: u64, kind: RunKind, by: String, at: u64, fails: bool) {
    let Some(run_id) = new_run(store, product_id, kind, by, at) else { return };
    let Some(product) = store.product(product_id).cloned() else { return };
    let branch = branch_name(kind, run_id, at);
    let (steps, failed) = if fails {
        let repo = &product.git.in_repos[0];
        (clone_failure(repo, auth_error(repo)), true)
    } else {
        (successful_script(&product, kind, &branch, &mut Rng::new(run_id)), false)
    };
    let mut t = at;
    for step in steps {
        t += step.delay_ms * REPLAY_TIME_SCALE;
        if let Some(run) = store.run_mut(run_id) {
            apply(run, step, t);
        }
    }
    if failed { fail(store, run_id, t) } else { finish(store, run_id, &branch, t) }
}

pub(crate) enum StartError {
    NotFound,
    AlreadyRunning(u64),
    Onboarding(u64),
}

pub(crate) fn start(shared: &Shared, product_id: u64, kind: RunKind, by: &str) -> Result<u64, StartError> {
    let mut store = shared.lock().unwrap();
    if let Some(running) = store.runs.iter().find(|r| {
        r.product_id == product_id
            && (r.kind == kind || r.kind == RunKind::Onboarding)
            && matches!(r.status, RunStatus::Queued | RunStatus::Running)
    }) {
        return Err(if running.kind == kind {
            StartError::AlreadyRunning(running.id)
        } else {
            StartError::Onboarding(running.id)
        });
    }
    let product = store.product(product_id).cloned().ok_or(StartError::NotFound)?;
    if let RunKind::Document(doc) = kind
        && !product.documents.iter().any(|d| d.kind == doc)
    {
        return Err(StartError::NotFound);
    }
    let at = now_ms();
    let run_id = new_run(&mut store, product_id, kind, by.to_string(), at).ok_or(StartError::NotFound)?;
    let branch = branch_name(kind, run_id, at);
    let (steps, fails) = script(&store, &product, kind, &branch, &mut Rng::new(run_id ^ at));
    drop(store);

    let shared = shared.clone();
    tokio::spawn(async move {
        for step in steps {
            tokio::time::sleep(Duration::from_millis(step.delay_ms)).await;
            let mut store = shared.lock().unwrap();
            if let Some(run) = store.run_mut(run_id) {
                apply(run, step, now_ms());
            }
        }
        let mut store = shared.lock().unwrap();
        if fails { fail(&mut store, run_id, now_ms()) } else { finish(&mut store, run_id, &branch, now_ms()) }
    });
    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state;

    #[test]
    fn replayed_run_counts_iterations_and_tokens() {
        let shared = state::seeded();
        let store = shared.lock().unwrap();
        for run in store.runs.iter().filter(|r| r.status == RunStatus::Succeeded) {
            assert!(run.iteration > 5, "run {} has {} iterations", run.id, run.iteration);
            assert!(run.input_tokens > run.output_tokens);
            let result = run.result.as_ref().unwrap();
            assert_eq!(result.git.commit.len(), 40);
            assert!(result.git.report_url.as_ref().unwrap().contains(&result.git.branch));
            let seqs: Vec<usize> = run.events.iter().map(|e| e.seq).collect();
            assert_eq!(seqs, (1..=run.events.len()).collect::<Vec<_>>());
        }
    }

    #[test]
    fn seeded_history_does_not_depend_on_current_connections() {
        let shared = state::seeded();
        let store = shared.lock().unwrap();
        let lab: Vec<RunStatus> = store
            .runs
            .iter()
            .filter(|r| r.product_name == "Lab Connector")
            .map(|r| r.status)
            .collect();
        assert_eq!(lab, [RunStatus::Succeeded, RunStatus::Failed]);
    }

    #[test]
    fn expired_connection_fails_the_run() {
        let shared = state::seeded();
        let store = shared.lock().unwrap();
        let lab = store.products.iter().find(|p| p.name == "Lab Connector").unwrap();
        assert!(clone_error(&store, lab, RunKind::Document(DocKind::TestConcept)).is_some());
        let inventory = store.products.iter().find(|p| p.name == "Inventory").unwrap();
        assert!(clone_error(&store, inventory, RunKind::Document(DocKind::TestConcept)).is_none());
    }

    #[test]
    fn onboarding_scaffold_has_readme_and_all_chapters() {
        let shared = state::seeded();
        let store = shared.lock().unwrap();
        let inventory = store.products.iter().find(|p| p.name == "Inventory").unwrap();
        let files = content::scaffold(inventory);
        let chapters: usize = inventory.documents.iter().map(|d| d.kind.chapters().len()).sum();
        assert_eq!(files.len(), 1 + chapters);
        assert_eq!(files[0].path, "README.md");
    }
}
