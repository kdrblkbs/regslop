use axum::Json;
use axum::Router;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::http::header;
use axum::response::Html;
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::get;
use axum::routing::post;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::content;
use crate::sim;
use crate::state;
use crate::state::BedrockSettings;
use crate::state::ConnectionStatus;
use crate::state::DocKind;
use crate::state::GitSettings;
use crate::state::ProductInput;
use crate::state::ProductStatus;
use crate::state::Provider;
use crate::state::RunKind;
use crate::state::RunStatus;
use crate::state::Settings;
use crate::state::Shared;
use crate::state::Store;
use crate::state::now_ms;

pub(crate) fn router(shared: Shared) -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("../static/index.html")) }))
        .route("/app", get(|| async { Html(include_str!("../static/app.html")) }))
        .route("/static/style.css", get(|| async { asset("text/css", include_str!("../static/style.css")) }))
        .route("/static/app.js", get(|| async { asset("text/javascript", include_str!("../static/app.js")) }))
        .route("/api/overview", get(overview))
        .route("/api/catalog", get(catalog))
        .route("/api/repos/check", post(check_repos))
        .route("/api/products", get(list_products).post(create_product))
        .route("/api/products/{id}", get(get_product).put(update_product))
        .route("/api/products/{id}/runs", post(start_run))
        .route("/api/products/{id}/onboarding", post(start_onboarding))
        .route("/api/products/{id}/regslop_config.toml", get(export_config))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/{id}", get(get_run))
        .route("/api/tasks", get(list_tasks))
        .route("/api/connections", get(list_connections))
        .route("/api/connections/{id}/reconnect", post(reconnect))
        .route("/api/settings", get(get_settings).put(put_settings))
        .with_state(shared)
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}

type ApiError = (StatusCode, Json<Value>);
type ApiResult = Result<Json<Value>, ApiError>;

fn error(status: StatusCode, message: impl Into<String>) -> ApiError {
    (status, Json(json!({ "error": message.into() })))
}

fn not_found(what: &str) -> ApiError {
    error(StatusCode::NOT_FOUND, format!("{what} not found"))
}

fn invalid(message: impl Into<String>) -> ApiError {
    error(StatusCode::UNPROCESSABLE_ENTITY, message)
}

async fn overview(State(shared): State<Shared>) -> ApiResult {
    let store = shared.lock().unwrap();
    let month_start = now_ms() - 30 * state::DAY_MS;
    let month: Vec<_> = store.runs.iter().filter(|r| r.started_at >= month_start).collect();
    let documents: Vec<Value> = store
        .products
        .iter()
        .flat_map(|p| {
            p.documents.iter().map(move |d| {
                json!({ "product_id": p.id, "product_name": p.name, "provider": p.git.provider, "document": d })
            })
        })
        .collect();
    Ok(Json(json!({
        "org": store.org,
        "user": state::CURRENT_USER,
        "products": store.products.len(),
        "usage": {
            "runs": month.len(),
            "input_tokens": month.iter().map(|r| r.input_tokens).sum::<u64>(),
            "output_tokens": month.iter().map(|r| r.output_tokens).sum::<u64>(),
            "cost_usd": month.iter().map(|r| r.cost_usd).sum::<f64>(),
        },
        "open_tasks": store.tasks.len(),
        "running": store.runs.iter().filter(|r| matches!(r.status, RunStatus::Queued | RunStatus::Running)).count(),
        "expired_connections": store.connections.iter().filter(|c| c.status == ConnectionStatus::Expired).count(),
        "documents": documents,
        "recent_runs": store.summaries(store.runs.iter()).into_iter().take(8).collect::<Vec<_>>(),
    })))
}

async fn catalog() -> Json<Value> {
    let documents: Vec<Value> = DocKind::ALL
        .iter()
        .map(|k| json!({ "kind": k, "title": k.title(), "slug": k.slug(), "chapters": k.chapters(), "focus": k.focus() }))
        .collect();
    Json(json!({
        "documents": documents,
        "models": sim::MODELS,
        "defaults": BedrockSettings::default(),
        "default_system_prompt": content::default_system_prompt(),
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum RepoStatus {
    Ok,
    Missing,
    NoAccess,
    NoConnection,
    Invalid,
}

#[derive(Debug, Serialize)]
struct RepoCheck {
    url: String,
    role: &'static str,
    status: RepoStatus,
    message: String,
}

fn check(store: &Store, url: &str, role: &'static str) -> RepoCheck {
    let (status, message) = if !url.starts_with("https://") || state::repo_name(url).is_empty() {
        // same rule as the cli: the token is only ever sent over https
        (RepoStatus::Invalid, "Not an https repository URL.".to_string())
    } else {
        match store.connection_for(url) {
            None => (
                RepoStatus::NoConnection,
                format!("No git connection covers {}. Ask an admin to add one in Settings.", state::repo_host(url)),
            ),
            Some(c) if c.status == ConnectionStatus::Expired => (
                RepoStatus::NoAccess,
                format!("The token for {}/{} has expired. Reconnect it in Settings.", c.host, c.scope),
            ),
            Some(_) if !store.repo_exists(url) => (RepoStatus::Missing, "Repository does not exist.".to_string()),
            Some(_) => (RepoStatus::Ok, "Found, default branch main.".to_string()),
        }
    };
    RepoCheck { url: url.to_string(), role, status, message }
}

fn check_all(store: &Store, git: &GitSettings) -> Vec<RepoCheck> {
    git.in_repos
        .iter()
        .map(|r| check(store, r, "in"))
        .chain(std::iter::once(check(store, &git.out_repo, "out")))
        .collect()
}

async fn check_repos(State(shared): State<Shared>, Json(git): Json<GitSettings>) -> Json<Value> {
    let store = shared.lock().unwrap();
    Json(json!(check_all(&store, &git)))
}

fn validate(input: &ProductInput) -> Result<(), ApiError> {
    let b = &input.bedrock;
    if input.name.trim().is_empty() {
        return Err(invalid("name must not be empty"));
    }
    if input.git.in_repos.is_empty() {
        return Err(invalid("in_repos needs at least one repository"));
    }
    if input.documents.is_empty() {
        return Err(invalid("select at least one document"));
    }
    if input.documents.iter().enumerate().any(|(i, d)| input.documents[..i].iter().any(|e| e.kind == d.kind)) {
        return Err(invalid("each document can only be selected once"));
    }
    if !(1..=512).contains(&b.max_iterations) {
        return Err(invalid("max_iterations must be between 1 and 512"));
    }
    if !(256..=64_000).contains(&b.max_tokens) {
        return Err(invalid("max_tokens must be between 256 and 64000"));
    }
    if !(1..=60).contains(&b.client_timeout_mins) {
        return Err(invalid("client_timeout_mins must be between 1 and 60"));
    }
    if b.model.trim().is_empty() || !b.base_url.starts_with("https://") {
        return Err(invalid("model must be set and base_url must be an https URL"));
    }
    let prompts = input
        .documents
        .iter()
        .filter_map(|d| d.task_prompt.as_deref())
        .chain(b.system_prompt.as_deref());
    for prompt in prompts {
        if prompt.contains("'''") {
            return Err(invalid("prompts must not contain ''' (it ends a TOML literal string)"));
        }
        if prompt.trim().is_empty() {
            return Err(invalid("prompts must not be empty"));
        }
    }
    Ok(())
}

/// every repo must be usable, except a missing documentation repo when onboarding creates it
fn ensure_repos_usable(store: &Store, git: &GitSettings, onboarding: bool) -> Result<(), ApiError> {
    for c in check_all(store, git) {
        match c.status {
            RepoStatus::Ok => {}
            RepoStatus::Missing if c.role == "out" && onboarding => {}
            RepoStatus::Missing if c.role == "out" => {
                return Err(invalid(format!(
                    "{}: the documentation repository does not exist. Enable onboarding to create it.",
                    c.url
                )));
            }
            _ => return Err(invalid(format!("{}: {}", c.url, c.message))),
        }
    }
    Ok(())
}

fn trimmed(mut input: ProductInput) -> ProductInput {
    input.name = input.name.trim().to_string();
    input.description = input.description.trim().to_string();
    input.git.in_repos = input.git.in_repos.iter().map(|r| r.trim().to_string()).filter(|r| !r.is_empty()).collect();
    input.git.out_repo = input.git.out_repo.trim().to_string();
    input
}

async fn list_products(State(shared): State<Shared>) -> Json<Value> {
    let store = shared.lock().unwrap();
    Json(json!(store.products))
}

async fn get_product(State(shared): State<Shared>, Path(id): Path<u64>) -> ApiResult {
    let store = shared.lock().unwrap();
    let product = store.product(id).ok_or_else(|| not_found("product"))?;
    let mut connections: Vec<&state::Connection> = Vec::new();
    for c in product.repos().filter_map(|r| store.connection_for(r)) {
        if !connections.iter().any(|a| a.id == c.id) {
            connections.push(c);
        }
    }
    Ok(Json(json!({
        "product": product,
        "connections": connections,
        "runs": store.summaries(store.runs.iter().filter(|r| r.product_id == id)),
        "open_tasks": store.tasks.iter().filter(|t| t.product_id == id).collect::<Vec<_>>(),
        "out_repo_exists": store.repo_exists(&product.git.out_repo),
    })))
}

async fn create_product(State(shared): State<Shared>, Json(input): Json<ProductInput>) -> ApiResult {
    let input = trimmed(input);
    validate(&input)?;
    let onboarding = {
        let store = shared.lock().unwrap();
        ensure_repos_usable(&store, &input.git, input.onboarding)?;
        input.onboarding && !store.repo_exists(&input.git.out_repo)
    };
    let id = state::add_product(&mut shared.lock().unwrap(), input, state::CURRENT_USER, now_ms());
    let onboarding_run = if onboarding {
        sim::start(&shared, id, RunKind::Onboarding, state::CURRENT_USER).ok()
    } else {
        None
    };
    let store = shared.lock().unwrap();
    Ok(Json(json!({ "product": store.product(id), "onboarding_run": onboarding_run })))
}

async fn update_product(State(shared): State<Shared>, Path(id): Path<u64>, Json(input): Json<ProductInput>) -> ApiResult {
    let input = trimmed(input);
    validate(&input)?;
    let mut store = shared.lock().unwrap();
    ensure_repos_usable(&store, &input.git, false)?;
    let running = store
        .runs
        .iter()
        .any(|r| r.product_id == id && matches!(r.status, RunStatus::Queued | RunStatus::Running));
    let product = store.product_mut(id).ok_or_else(|| not_found("product"))?;
    if running || product.status == ProductStatus::Onboarding {
        return Err(error(StatusCode::CONFLICT, "wait until the running runs have finished"));
    }
    state::update_product(product, input);
    Ok(Json(json!(product)))
}

#[derive(Deserialize)]
struct StartRun {
    document: DocKind,
}

fn start(shared: &Shared, id: u64, kind: RunKind) -> ApiResult {
    match sim::start(shared, id, kind, state::CURRENT_USER) {
        Ok(run_id) => Ok(Json(json!({ "run_id": run_id }))),
        Err(sim::StartError::NotFound) => Err(not_found("product or document")),
        Err(sim::StartError::AlreadyRunning(run_id)) => Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "a run for this document is already in progress", "run_id": run_id })),
        )),
        Err(sim::StartError::Onboarding(run_id)) => Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "the documentation repository is still being created", "run_id": run_id })),
        )),
    }
}

async fn start_run(State(shared): State<Shared>, Path(id): Path<u64>, Json(body): Json<StartRun>) -> ApiResult {
    start(&shared, id, RunKind::Document(body.document))
}

/// retries the onboarding, e.g. after a failure
async fn start_onboarding(State(shared): State<Shared>, Path(id): Path<u64>) -> ApiResult {
    {
        let store = shared.lock().unwrap();
        let product = store.product(id).ok_or_else(|| not_found("product"))?;
        if store.repo_exists(&product.git.out_repo) {
            return Err(error(StatusCode::CONFLICT, "the documentation repository already exists"));
        }
    }
    start(&shared, id, RunKind::Onboarding)
}

#[derive(Deserialize)]
struct ExportQuery {
    document: DocKind,
}

/// a TOML basic string
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// renders a config the cli accepts as is (it denies unknown fields)
async fn export_config(
    State(shared): State<Shared>,
    Path(id): Path<u64>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let store = shared.lock().unwrap();
    let product = store.product(id).ok_or_else(|| not_found("product"))?;
    let doc = product
        .documents
        .iter()
        .find(|d| d.kind == query.document)
        .ok_or_else(|| not_found("document"))?;
    let (git, b) = (&product.git, &product.bedrock);
    let provider = match git.provider {
        Provider::Github => "github",
        Provider::Gitlab => "gitlab",
    };
    let in_repos: String = git.in_repos.iter().map(|r| format!("    {},\n", quote(r))).collect();
    let system_prompt = b.system_prompt.as_deref().unwrap_or(content::default_system_prompt());
    let task_prompt = doc.task_prompt.replace("{{date}}", &state::date_string(now_ms()));
    let toml = format!(
        "# {} · {} · exported from the regslop service (mock)\n[git]\nprovider = \"{provider}\"\ntoken = \"<insert token>\"\nin_repos = [\n{in_repos}]\nout_repo = {}\n\n[bedrock]\ntoken = \"<insert token>\"\nmax_iterations = {}\nclient_timeout_mins = {}\nmax_tokens = {}\nbase_url = {}\nmodel = {}\n\nsystem_prompt = '''\n{}'''\ntask_prompt = '''\n{}'''\n",
        product.name,
        doc.title,
        quote(&git.out_repo),
        b.max_iterations,
        b.client_timeout_mins,
        b.max_tokens,
        quote(&b.base_url),
        quote(&b.model),
        system_prompt.trim_end_matches('\n').to_string() + "\n",
        task_prompt.trim_end_matches('\n').to_string() + "\n",
    );
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], toml).into_response())
}

async fn list_runs(State(shared): State<Shared>) -> Json<Value> {
    let store = shared.lock().unwrap();
    Json(json!(store.summaries(store.runs.iter())))
}

#[derive(Deserialize)]
struct RunQuery {
    /// only events with a higher seq, so a live view can poll cheaply
    after: Option<usize>,
}

async fn get_run(State(shared): State<Shared>, Path(id): Path<u64>, Query(query): Query<RunQuery>) -> ApiResult {
    let store = shared.lock().unwrap();
    let run = store.run(id).ok_or_else(|| not_found("run"))?;
    let mut value = json!(run);
    if let Some(after) = query.after {
        value["events"] = json!(run.events.iter().skip(after).collect::<Vec<_>>());
    }
    if !store.settings.preview_merge_requests && value["result"].is_object() {
        value["result"]["merge_request"] = Value::Null;
    }
    value["tasks"] = json!(store.tasks.iter().filter(|t| t.run_id == id).collect::<Vec<_>>());
    value["provider"] = json!(store.product(run.product_id).map(|p| p.git.provider));
    Ok(Json(value))
}

async fn list_tasks(State(shared): State<Shared>) -> Json<Value> {
    let store = shared.lock().unwrap();
    let mut tasks = store.tasks.clone();
    tasks.sort_by(|a, b| (&a.product_name, a.document_title, &a.todo).cmp(&(&b.product_name, b.document_title, &b.todo)));
    Json(json!(tasks))
}

async fn list_connections(State(shared): State<Shared>) -> Json<Value> {
    let store = shared.lock().unwrap();
    Json(json!(store.connections))
}

async fn reconnect(State(shared): State<Shared>, Path(id): Path<u64>) -> ApiResult {
    let mut store = shared.lock().unwrap();
    let connection = store
        .connections
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| not_found("connection"))?;
    connection.status = ConnectionStatus::Connected;
    connection.connected_at = now_ms();
    Ok(Json(json!(connection)))
}

async fn get_settings(State(shared): State<Shared>) -> Json<Value> {
    Json(json!(shared.lock().unwrap().settings))
}

async fn put_settings(State(shared): State<Shared>, Json(settings): Json<Settings>) -> Json<Value> {
    let mut store = shared.lock().unwrap();
    store.settings = settings;
    Json(json!(store.settings))
}
