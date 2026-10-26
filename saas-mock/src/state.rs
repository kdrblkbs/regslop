//! In-memory store of the mock. Everything is lost on restart, which is the point.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;

use crate::content;
use crate::sim;

pub(crate) type Shared = Arc<Mutex<Store>>;

pub(crate) const DAY_MS: u64 = 86_400_000;
pub(crate) const CURRENT_USER: &str = "you@example.com";

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

// civil date from unix days (Howard Hinnant's algorithm), so we don't need chrono for a mock
pub(crate) fn date_string(ms: u64) -> String {
    let z = (ms / DAY_MS) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// "https://gitlab.example.com/group/sub/repo.git" -> "gitlab.example.com"
pub(crate) fn repo_host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split('/').next().unwrap_or(rest)
}

/// "https://gitlab.example.com/group/sub/repo.git" -> "group/sub/repo"
pub(crate) fn repo_path(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = rest.split_once('/').map_or("", |(_, path)| path);
    path.trim_end_matches('/').trim_end_matches(".git")
}

/// "https://gitlab.example.com/group/sub/repo.git" -> "repo", the folder git clone creates
pub(crate) fn repo_name(url: &str) -> &str {
    repo_path(url).rsplit('/').next().unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Provider {
    Github,
    Gitlab,
}

impl Provider {
    /// web url of a page in a repository, e.g. `tree/<branch>` or `commit/<sha>`
    pub(crate) fn web_url(self, repo: &str, page: &str) -> String {
        let separator = match self {
            Provider::Github => "",
            Provider::Gitlab => "-/",
        };
        format!("https://{}/{}/{separator}{page}", repo_host(repo), repo_path(repo))
    }

    pub(crate) fn merge_request_url(self, repo: &str, number: u64) -> (String, String) {
        match self {
            Provider::Github => (format!("#{number}"), self.web_url(repo, &format!("pull/{number}"))),
            Provider::Gitlab => (format!("!{number}"), self.web_url(repo, &format!("merge_requests/{number}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocKind {
    TestConcept,
    Architecture,
    BackupRecovery,
    SecurityConcept,
}

impl DocKind {
    pub(crate) const ALL: [DocKind; 4] = [
        DocKind::TestConcept,
        DocKind::Architecture,
        DocKind::BackupRecovery,
        DocKind::SecurityConcept,
    ];

    pub(crate) fn title(self) -> &'static str {
        match self {
            DocKind::TestConcept => "Test concept",
            DocKind::Architecture => "Architecture documentation",
            DocKind::BackupRecovery => "Backup & recovery concept",
            DocKind::SecurityConcept => "Security concept",
        }
    }

    /// folder below documents/ in the documentation repository
    pub(crate) fn slug(self) -> &'static str {
        match self {
            DocKind::TestConcept => "test-concept",
            DocKind::Architecture => "architecture",
            DocKind::BackupRecovery => "backup-recovery",
            DocKind::SecurityConcept => "security-concept",
        }
    }

    pub(crate) fn chapters(self) -> &'static [&'static str] {
        match self {
            DocKind::TestConcept => &[
                "01-introduction.md",
                "02-test-strategy.md",
                "03-test-levels.md",
                "04-test-environment.md",
                "05-test-execution-and-ci.md",
                "06-traceability.md",
            ],
            DocKind::Architecture => &[
                "01-overview.md",
                "02-system-context.md",
                "03-components.md",
                "04-deployment.md",
                "05-interfaces.md",
            ],
            DocKind::BackupRecovery => &[
                "01-scope.md",
                "02-data-inventory.md",
                "03-backup-procedure.md",
                "04-restore-procedure.md",
                "05-rto-rpo.md",
            ],
            DocKind::SecurityConcept => &[
                "01-scope.md",
                "02-threat-model.md",
                "03-authentication-and-authorization.md",
                "04-data-protection.md",
                "05-vulnerability-management.md",
            ],
        }
    }

    /// the "what matters for this document type" section of the task prompt
    pub(crate) fn focus(self) -> &'static [&'static str] {
        match self {
            DocKind::TestConcept => &[
                "All test levels present (unit, integration, end-to-end, manual) and which components they cover",
                "Test frameworks and tooling, and how tests are executed (local commands, CI pipelines and their triggers)",
                "Test environments and test data, as far as defined in the repositories",
                "Coverage measurement and thresholds, if configured",
                "Traceability between requirements/features and tests, if the template asks for it",
                "Areas with no or weak test coverage (report as findings; do not invent tests)",
            ],
            DocKind::Architecture => &[
                "Components, their responsibilities and how they interact",
                "External systems and interfaces (protocols, data formats, authentication)",
                "Deployment topology as far as defined in the repositories (containers, IaC, manifests)",
                "Technology stack and third-party dependencies with versions",
            ],
            DocKind::BackupRecovery => &[
                "Persistent data stores and what they contain",
                "Backup jobs, schedules and retention as configured in the repositories",
                "Restore procedures and whether they are scripted or tested",
                "RTO/RPO, if defined anywhere (do not invent targets)",
            ],
            DocKind::SecurityConcept => &[
                "Authentication and authorization mechanisms and where they are enforced",
                "Handling of secrets, personal and sensitive data, encryption in transit and at rest",
                "Dependency and vulnerability scanning in CI",
                "Security-relevant findings (report them; do not speculate about exploits)",
            ],
        }
    }
}

/// the [git] section of regslop_config.toml, minus the token (that comes from a connection)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GitSettings {
    pub(crate) provider: Provider,
    pub(crate) in_repos: Vec<String>,
    pub(crate) out_repo: String,
}

/// the [bedrock] section of regslop_config.toml, minus the token and the task prompt (per document)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BedrockSettings {
    pub(crate) model: String,
    pub(crate) base_url: String,
    pub(crate) max_iterations: u32,
    pub(crate) max_tokens: u32,
    pub(crate) client_timeout_mins: u32,
    /// None uses the default system prompt of the service
    pub(crate) system_prompt: Option<String>,
}

impl Default for BedrockSettings {
    fn default() -> Self {
        Self {
            model: sim::DEFAULT_MODEL.to_string(),
            base_url: sim::DEFAULT_BASE_URL.to_string(),
            max_iterations: 128,
            max_tokens: 8192,
            client_timeout_mins: 10,
            system_prompt: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DocStatus {
    NeverRun,
    Running,
    Complete,
    CompleteWithOpenItems,
    #[expect(dead_code, reason = "part of the report format, but every canned run finishes")]
    Incomplete,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Document {
    pub(crate) kind: DocKind,
    pub(crate) title: &'static str,
    pub(crate) path: String,
    /// `{{date}}` is replaced with the run date
    pub(crate) task_prompt: String,
    pub(crate) status: DocStatus,
    pub(crate) open_tasks: usize,
    pub(crate) last_run_id: Option<u64>,
    pub(crate) last_run_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProductStatus {
    /// the documentation repository is being created
    Onboarding,
    Ready,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Product {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) owner: String,
    pub(crate) status: ProductStatus,
    pub(crate) git: GitSettings,
    pub(crate) bedrock: BedrockSettings,
    pub(crate) documents: Vec<Document>,
    pub(crate) created_at: u64,
}

impl Product {
    pub(crate) fn document_mut(&mut self, kind: DocKind) -> Option<&mut Document> {
        self.documents.iter_mut().find(|d| d.kind == kind)
    }

    pub(crate) fn repos(&self) -> impl Iterator<Item = &String> {
        self.git.in_repos.iter().chain(std::iter::once(&self.git.out_repo))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunKind {
    /// creates the documentation repository from a scaffold
    Onboarding,
    /// one regslop invocation for one document
    Document(DocKind),
}

impl RunKind {
    pub(crate) fn title(self) -> &'static str {
        match self {
            RunKind::Onboarding => "Onboarding",
            RunKind::Document(kind) => kind.title(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EventKind {
    /// service side: sandbox, clone, push
    Phase,
    /// a toolUse of the model
    Tool,
    /// a text block of the model
    Message,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Event {
    pub(crate) seq: usize,
    pub(crate) at: u64,
    pub(crate) iteration: u32,
    pub(crate) kind: EventKind,
    pub(crate) tool: Option<&'static str>,
    pub(crate) detail: String,
}

/// where the result of a run lives in git
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GitResult {
    pub(crate) repo: String,
    pub(crate) branch: String,
    pub(crate) commit: String,
    pub(crate) branch_url: String,
    pub(crate) commit_url: String,
    pub(crate) report_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MergeRequest {
    pub(crate) reference: String,
    pub(crate) url: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ChangedFile {
    pub(crate) path: String,
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
    pub(crate) diff: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct RunResult {
    pub(crate) overall: Option<DocStatus>,
    pub(crate) git: GitResult,
    /// regslop doesn't open merge requests yet. the mock always computes one, the api only shows
    /// it when the preview setting is on
    pub(crate) merge_request: Option<MergeRequest>,
    pub(crate) files: Vec<ChangedFile>,
    pub(crate) report_md: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Run {
    pub(crate) id: u64,
    pub(crate) product_id: u64,
    pub(crate) product_name: String,
    pub(crate) kind: RunKind,
    pub(crate) title: &'static str,
    pub(crate) triggered_by: String,
    pub(crate) status: RunStatus,
    pub(crate) phase: String,
    pub(crate) model: String,
    pub(crate) iteration: u32,
    pub(crate) max_iterations: u32,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) context_tokens: u64,
    pub(crate) cost_usd: f64,
    pub(crate) started_at: u64,
    pub(crate) finished_at: Option<u64>,
    pub(crate) error: Option<String>,
    pub(crate) events: Vec<Event>,
    pub(crate) result: Option<RunResult>,
}

/// a run without the heavy parts, for lists
#[derive(Debug, Clone, Serialize)]
pub(crate) struct RunSummary {
    pub(crate) id: u64,
    pub(crate) product_id: u64,
    pub(crate) product_name: String,
    pub(crate) kind: RunKind,
    pub(crate) title: &'static str,
    pub(crate) triggered_by: String,
    pub(crate) status: RunStatus,
    pub(crate) phase: String,
    pub(crate) iteration: u32,
    pub(crate) max_iterations: u32,
    pub(crate) input_tokens: u64,
    pub(crate) output_tokens: u64,
    pub(crate) cost_usd: f64,
    pub(crate) started_at: u64,
    pub(crate) finished_at: Option<u64>,
    pub(crate) overall: Option<DocStatus>,
    pub(crate) branch: Option<String>,
    pub(crate) merge_request: Option<String>,
    pub(crate) open_tasks: usize,
}

/// a [[TODO-NNN]] placeholder from the latest status report of a document
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Task {
    pub(crate) id: u64,
    pub(crate) product_id: u64,
    pub(crate) product_name: String,
    pub(crate) run_id: u64,
    pub(crate) document: DocKind,
    pub(crate) document_title: &'static str,
    pub(crate) todo: String,
    pub(crate) file: String,
    /// the file on the run's branch
    pub(crate) file_url: String,
    pub(crate) description: String,
    pub(crate) role: String,
    pub(crate) reason: String,
    pub(crate) created_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ConnectionStatus {
    Connected,
    Expired,
}

/// a git token for one host/group, the token of [git] in the config
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Connection {
    pub(crate) id: u64,
    pub(crate) provider: Provider,
    pub(crate) host: String,
    pub(crate) scope: String,
    pub(crate) kind: &'static str,
    pub(crate) status: ConnectionStatus,
    pub(crate) connected_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub(crate) preview_merge_requests: bool,
}

#[derive(Debug)]
pub(crate) struct Store {
    next_id: u64,
    pub(crate) org: String,
    pub(crate) settings: Settings,
    pub(crate) products: Vec<Product>,
    pub(crate) runs: Vec<Run>,
    pub(crate) tasks: Vec<Task>,
    pub(crate) connections: Vec<Connection>,
    /// what the fake git hosts contain
    pub(crate) existing_repos: Vec<String>,
}

impl Store {
    pub(crate) fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub(crate) fn product(&self, id: u64) -> Option<&Product> {
        self.products.iter().find(|p| p.id == id)
    }

    pub(crate) fn product_mut(&mut self, id: u64) -> Option<&mut Product> {
        self.products.iter_mut().find(|p| p.id == id)
    }

    pub(crate) fn run(&self, id: u64) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }

    pub(crate) fn run_mut(&mut self, id: u64) -> Option<&mut Run> {
        self.runs.iter_mut().find(|r| r.id == id)
    }

    pub(crate) fn connection_for(&self, repo: &str) -> Option<&Connection> {
        let host = repo_host(repo);
        let path = repo_path(repo);
        self.connections
            .iter()
            .find(|c| c.host == host && path.starts_with(&format!("{}/", c.scope)))
    }

    pub(crate) fn repo_exists(&self, repo: &str) -> bool {
        self.existing_repos.iter().any(|r| r == repo)
    }

    pub(crate) fn open_tasks_of_run(&self, run_id: u64) -> usize {
        self.tasks.iter().filter(|t| t.run_id == run_id).count()
    }

    pub(crate) fn summary(&self, run: &Run) -> RunSummary {
        let result = run.result.as_ref();
        RunSummary {
            id: run.id,
            product_id: run.product_id,
            product_name: run.product_name.clone(),
            kind: run.kind,
            title: run.title,
            triggered_by: run.triggered_by.clone(),
            status: run.status,
            phase: run.phase.clone(),
            iteration: run.iteration,
            max_iterations: run.max_iterations,
            input_tokens: run.input_tokens,
            output_tokens: run.output_tokens,
            cost_usd: run.cost_usd,
            started_at: run.started_at,
            finished_at: run.finished_at,
            overall: result.and_then(|r| r.overall),
            branch: result.map(|r| r.git.branch.clone()),
            merge_request: result
                .and_then(|r| r.merge_request.as_ref())
                .filter(|_| self.settings.preview_merge_requests)
                .map(|mr| mr.reference.clone()),
            open_tasks: self.open_tasks_of_run(run.id),
        }
    }

    pub(crate) fn summaries<'a>(&self, runs: impl Iterator<Item = &'a Run>) -> Vec<RunSummary> {
        let mut summaries: Vec<RunSummary> = runs.map(|r| self.summary(r)).collect();
        summaries.sort_by_key(|s| std::cmp::Reverse(s.started_at));
        summaries
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentInput {
    pub(crate) kind: DocKind,
    /// None keeps the current prompt, or generates one from the template for a new document
    #[serde(default)]
    pub(crate) task_prompt: Option<String>,
}

/// body of create and update. it is the regslop_config.toml, split into product and documents
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProductInput {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    pub(crate) git: GitSettings,
    pub(crate) bedrock: BedrockSettings,
    pub(crate) documents: Vec<DocumentInput>,
    /// create only: create the documentation repository if it doesn't exist
    #[serde(default)]
    pub(crate) onboarding: bool,
}

fn document(kind: DocKind, git: &GitSettings, task_prompt: Option<String>) -> Document {
    Document {
        kind,
        title: kind.title(),
        path: format!("documents/{}", kind.slug()),
        task_prompt: task_prompt.unwrap_or_else(|| content::task_prompt(kind, git)),
        status: DocStatus::NeverRun,
        open_tasks: 0,
        last_run_id: None,
        last_run_at: None,
    }
}

pub(crate) fn add_product(store: &mut Store, input: ProductInput, owner: &str, created_at: u64) -> u64 {
    let id = store.next_id();
    let documents = input
        .documents
        .into_iter()
        .map(|d| document(d.kind, &input.git, d.task_prompt))
        .collect();
    store.products.push(Product {
        id,
        name: input.name,
        description: input.description,
        owner: owner.to_string(),
        status: ProductStatus::Ready,
        git: input.git,
        bedrock: input.bedrock,
        documents,
        created_at,
    });
    id
}

pub(crate) fn update_product(product: &mut Product, input: ProductInput) {
    let mut documents = Vec::new();
    for d in input.documents {
        let doc = match product.documents.iter().find(|existing| existing.kind == d.kind) {
            Some(existing) => Document {
                task_prompt: d.task_prompt.unwrap_or_else(|| existing.task_prompt.clone()),
                ..existing.clone()
            },
            None => document(d.kind, &input.git, d.task_prompt),
        };
        documents.push(doc);
    }
    product.name = input.name;
    product.description = input.description;
    product.git = input.git;
    product.bedrock = input.bedrock;
    product.documents = documents;
}

pub(crate) fn seeded() -> Shared {
    let now = now_ms();
    let mut store = Store {
        next_id: 0,
        org: "Example Org".to_string(),
        settings: Settings { preview_merge_requests: false },
        products: Vec::new(),
        runs: Vec::new(),
        tasks: Vec::new(),
        connections: Vec::new(),
        existing_repos: Vec::new(),
    };

    for (provider, host, scope, kind, status, age_days) in [
        (Provider::Github, "github.com", "example-org", "GitHub App installation", ConnectionStatus::Connected, 41),
        (Provider::Gitlab, "gitlab.example.com", "medtech", "Group access token", ConnectionStatus::Connected, 33),
        (Provider::Gitlab, "gitlab.com", "lab-tools", "Project access token", ConnectionStatus::Expired, 96),
    ] {
        let id = store.next_id();
        store.connections.push(Connection {
            id,
            provider,
            host: host.to_string(),
            scope: scope.to_string(),
            kind,
            status,
            connected_at: now - age_days * DAY_MS,
        });
    }

    let products = [
        (
            "Inventory",
            "Stock management for hospital pharmacies",
            CURRENT_USER,
            GitSettings {
                provider: Provider::Github,
                in_repos: vec![
                    "https://github.com/example-org/inventory-service.git".to_string(),
                    "https://github.com/example-org/inventory-web.git".to_string(),
                ],
                out_repo: "https://github.com/example-org/inventory-regulatory.git".to_string(),
            },
            vec![DocKind::TestConcept, DocKind::Architecture, DocKind::BackupRecovery],
            40,
        ),
        (
            "Patient Portal",
            "Appointment booking and lab results for patients",
            "qa-lead@example.com",
            GitSettings {
                provider: Provider::Gitlab,
                in_repos: vec![
                    "https://gitlab.example.com/medtech/portal/portal-api.git".to_string(),
                    "https://gitlab.example.com/medtech/portal/portal-frontend.git".to_string(),
                    "https://gitlab.example.com/medtech/portal/portal-infra.git".to_string(),
                ],
                out_repo: "https://gitlab.example.com/medtech/portal/portal-regulatory.git".to_string(),
            },
            vec![DocKind::TestConcept, DocKind::SecurityConcept],
            32,
        ),
        (
            "Lab Connector",
            "HL7 bridge between lab devices and the LIS",
            CURRENT_USER,
            GitSettings {
                provider: Provider::Gitlab,
                in_repos: vec!["https://gitlab.com/lab-tools/lab-connector.git".to_string()],
                out_repo: "https://gitlab.com/lab-tools/lab-connector-regulatory.git".to_string(),
            },
            vec![DocKind::TestConcept],
            95,
        ),
    ];
    let mut ids = Vec::new();
    for (name, description, owner, git, documents, age_days) in products {
        store.existing_repos.extend(git.in_repos.iter().cloned());
        store.existing_repos.push(git.out_repo.clone());
        let input = ProductInput {
            name: name.to_string(),
            description: description.to_string(),
            git,
            bedrock: BedrockSettings::default(),
            documents: documents.into_iter().map(|kind| DocumentInput { kind, task_prompt: None }).collect(),
            onboarding: false,
        };
        ids.push(add_product(&mut store, input, owner, now - age_days * DAY_MS));
    }
    let [inventory, portal, lab] = ids[..] else { unreachable!() };
    if let Some(lab) = store.product_mut(lab) {
        lab.bedrock.max_iterations = 96;
    }

    // repos that exist, but no product uses yet. the new product form starts with these, and its
    // documentation repo is missing on purpose, to show the onboarding
    store.existing_repos.extend(
        [
            "https://github.com/example-org/infusion-api.git",
            "https://github.com/example-org/infusion-app.git",
            "https://gitlab.example.com/medtech/infusion/infusion-api.git",
            "https://gitlab.example.com/medtech/infusion/infusion-app.git",
        ]
        .map(String::from),
    );

    // history, oldest first, so later runs supersede the placeholders of earlier ones
    for (product, kind, by, age_hours, fails) in [
        (lab, DocKind::TestConcept, CURRENT_USER, 20 * 24 + 3, false),
        (inventory, DocKind::Architecture, CURRENT_USER, 9 * 24 + 5, false),
        (inventory, DocKind::TestConcept, CURRENT_USER, 6 * 24 + 2, false),
        (lab, DocKind::TestConcept, CURRENT_USER, 3 * 24 + 7, true),
        (portal, DocKind::TestConcept, "qa-lead@example.com", 26, false),
        (portal, DocKind::SecurityConcept, "qa-lead@example.com", 25, false),
        (inventory, DocKind::TestConcept, CURRENT_USER, 19, false),
    ] {
        sim::replay(&mut store, product, RunKind::Document(kind), by.to_string(), now - age_hours * 3_600_000, fails);
    }

    Arc::new(Mutex::new(store))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_string_ok() {
        assert_eq!(date_string(0), "1970-01-01");
        // 2026-10-26T12:00:00Z
        assert_eq!(date_string(1_793_016_000_000), "2026-10-26");
        // leap day
        assert_eq!(date_string(1_709_208_000_000), "2024-02-29");
    }

    #[test]
    fn repo_parts_ok() {
        let url = "https://gitlab.example.com:8443/group/sub/repo.git";
        assert_eq!(repo_host(url), "gitlab.example.com:8443");
        assert_eq!(repo_path(url), "group/sub/repo");
        assert_eq!(repo_name(url), "repo");
    }

    #[test]
    fn web_urls_ok() {
        let gh = "https://github.com/org/repo.git";
        let gl = "https://gitlab.example.com/group/sub/repo.git";
        assert_eq!(Provider::Github.web_url(gh, "tree/b"), "https://github.com/org/repo/tree/b");
        assert_eq!(Provider::Gitlab.web_url(gl, "tree/b"), "https://gitlab.example.com/group/sub/repo/-/tree/b");
        assert_eq!(
            Provider::Gitlab.merge_request_url(gl, 7),
            ("!7".to_string(), "https://gitlab.example.com/group/sub/repo/-/merge_requests/7".to_string())
        );
    }

    #[test]
    fn seeded_store_is_consistent() {
        let shared = seeded();
        let store = shared.lock().unwrap();
        assert_eq!(store.products.len(), 3);
        assert!(store.runs.iter().all(|r| r.finished_at.is_some()));
        for product in &store.products {
            assert!(product.repos().all(|r| store.repo_exists(r)), "{}", product.name);
            for doc in &product.documents {
                let open = store
                    .tasks
                    .iter()
                    .filter(|t| t.product_id == product.id && t.document == doc.kind)
                    .count();
                assert_eq!(doc.open_tasks, open, "{} / {:?}", product.name, doc.kind);
                assert!(doc.task_prompt.contains(&format!("documents/{}", doc.kind.slug())));
            }
        }
    }
}
