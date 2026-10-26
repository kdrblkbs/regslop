//! Canned results per document type, plus the prompts and the onboarding scaffold. The shapes
//! follow the status report and placeholder format the cli's system prompt demands, so the mock
//! shows what a reviewer would actually get.

use crate::state::ChangedFile;
use crate::state::DocKind;
use crate::state::DocStatus;
use crate::state::GitSettings;
use crate::state::Product;
use crate::state::Provider;
use crate::state::repo_name;

const EXAMPLE_CONFIG: &str = include_str!("../../regslop_config.example.toml");

/// the system prompt of the cli's example config, so both stay in sync
pub(crate) fn default_system_prompt() -> &'static str {
    EXAMPLE_CONFIG
        .split_once("system_prompt = '''")
        .and_then(|(_, rest)| rest.split_once("'''"))
        .map_or("", |(prompt, _)| prompt.trim_start_matches('\n'))
}

/// the task prompt of the example config, generalized over document types. `{{date}}` is replaced
/// at run time
pub(crate) fn task_prompt(kind: DocKind, git: &GitSettings) -> String {
    let docs = repo_name(&git.out_repo);
    let slug = kind.slug();
    let mut prompt = format!(
        "Task: review and complete the regulatory **{}** for the software product below.\n\n## Source repositories (read-only, together one product)\n",
        kind.title().to_lowercase()
    );
    for repo in &git.in_repos {
        prompt += &format!("- everything in ./in/{}\n", repo_name(repo));
    }
    prompt += &format!(
        "\n## Documentation repository\n- Root and conventions: ./out/{docs}/README.md\n- Target documents:     ./out/{docs}/documents/{slug}/*\n- Status report:        ./out/{docs}/documents/{slug}/REGULATORY_STATUS_REPORT.md\n\n## Run context\n- Date: {{{{date}}}}\n- Product name / version: determine from the repositories\n\n## What matters for this document type\n"
    );
    for focus in kind.focus() {
        prompt += &format!("- {focus}\n");
    }
    prompt += "\nBegin by reading the documentation repository's README.md. Continue with reading the current state of the regulatory documents. Afterwards familiarize yourself with the source repositories that constitute the software product. And finally ensure that the regulatory documentation is up to date.\n";
    prompt
}

pub(crate) struct Todo {
    pub(crate) chapter: usize,
    pub(crate) description: &'static str,
    pub(crate) role: &'static str,
    pub(crate) reason: &'static str,
}

pub(crate) struct Change {
    pub(crate) chapter: usize,
    pub(crate) summary: &'static str,
    /// unified diff hunk body, `{product}`, `{svc}`, `{web}` and `{ci}` get replaced
    pub(crate) diff: &'static str,
}

pub(crate) fn todos(kind: DocKind) -> &'static [Todo] {
    match kind {
        DocKind::TestConcept => &[
            Todo {
                chapter: 0,
                description: "Document ID and revision according to the QMS document register",
                role: "Quality manager",
                reason: "Document IDs are assigned in the QMS and do not appear in any repository.",
            },
            Todo {
                chapter: 0,
                description: "Name and role of the approver and the approval date",
                role: "Quality manager",
                reason: "Approvals are not recorded in the repositories.",
            },
            Todo {
                chapter: 3,
                description: "Hostname and access policy of the staging test environment",
                role: "DevOps engineer",
                reason: "The deploy job reads STAGING_URL from a CI secret; its value is not in the repositories.",
            },
            Todo {
                chapter: 4,
                description: "Date and result of the last manual exploratory test session",
                role: "QA engineer",
                reason: "Manual test executions are not recorded in the repositories.",
            },
        ],
        DocKind::Architecture => &[
            Todo {
                chapter: 0,
                description: "Document ID and revision according to the QMS document register",
                role: "Quality manager",
                reason: "Document IDs are assigned in the QMS and do not appear in any repository.",
            },
            Todo {
                chapter: 3,
                description: "Production hosting provider and region",
                role: "Platform engineer",
                reason: "Production infrastructure is provisioned outside the source repositories.",
            },
        ],
        DocKind::BackupRecovery => &[
            Todo {
                chapter: 2,
                description: "Backup retention period approved by the data protection officer",
                role: "Data protection officer",
                reason: "The cronjob keeps 14 daily dumps, but no approved retention policy is referenced.",
            },
            Todo {
                chapter: 3,
                description: "Date and result of the last restore test",
                role: "Operations",
                reason: "scripts/restore.sh exists, but no restore test execution is recorded.",
            },
            Todo {
                chapter: 4,
                description: "Approved RTO and RPO targets",
                role: "Product owner",
                reason: "No RTO/RPO is defined in any repository; targets must not be invented.",
            },
        ],
        DocKind::SecurityConcept => &[
            Todo {
                chapter: 0,
                description: "Document ID and revision according to the QMS document register",
                role: "Quality manager",
                reason: "Document IDs are assigned in the QMS and do not appear in any repository.",
            },
            Todo {
                chapter: 1,
                description: "Risk acceptance for threats T-04 (session fixation) and T-07 (log injection)",
                role: "Information security officer",
                reason: "Risk acceptance is an organisational decision.",
            },
            Todo {
                chapter: 4,
                description: "Reference to the most recent penetration test report",
                role: "Information security officer",
                reason: "Penetration test reports are not stored in the repositories.",
            },
        ],
    }
}

pub(crate) fn findings(kind: DocKind) -> &'static [&'static str] {
    match kind {
        DocKind::TestConcept => &[
            "`in/{web}/src/components/StockTable.tsx` implements manual stock corrections but has no unit or end-to-end test. Reported as a potential coverage gap; no tests were invented.",
            "CI enforces a line coverage threshold of 80 % (`{ci}`, job `coverage`), while chapter 02 stated 70 %. Chapter 02 now states 80 %; please confirm this is the intended threshold.",
            "The template asks for a requirements-to-test traceability matrix, but no requirement IDs exist in the source repositories. Chapter 06 maps features to tests instead; please confirm this interpretation.",
        ],
        DocKind::Architecture => &[
            "`in/{svc}/src/erp/client.rs` calls an external ERP API that is not mentioned in chapter 05. It was added as interface IF-04; the SLA is unknown.",
            "docker-compose.yml starts Redis, but no code path uses it. Possibly a leftover; documented as \"provisioned, usage not evident\".",
        ],
        DocKind::BackupRecovery => &[
            "Backups are written to the same cluster as the database (`k8s/cronjob-pg-backup.yaml`). There is no off-site copy, which may not meet the template's requirement for geographically separate storage.",
            "`scripts/restore.sh` is not executed by any CI job, so the restore procedure is untested.",
        ],
        DocKind::SecurityConcept => &[
            "`in/{svc}/src/auth/permissions.rs` grants the role `support` read access to lab results. This is not covered by the access matrix in chapter 03; please confirm it is intended.",
            "`cargo deny` runs in CI, but advisories are only warnings (`deny.toml`: `vulnerability = \"warn\"`).",
        ],
    }
}

pub(crate) fn changes(kind: DocKind) -> &'static [Change] {
    match kind {
        DocKind::TestConcept => &[
            Change {
                chapter: 0,
                summary: "Added document control placeholders required by the template (ID, approval).",
                diff: " ## Document control\n \n | Field | Value |\n |---|---|\n-| Document ID | |\n-| Approved by | |\n+| Document ID | [[TODO-001: Document ID and revision from the QMS document register]] |\n+| Approved by | [[TODO-002: Name, role and date of approval]] |\n | Product | {product} 2.4.0 (from `in/{svc}/Cargo.toml`) |\n",
            },
            Change {
                chapter: 1,
                summary: "Corrected the coverage threshold to the value CI enforces (evidence: `{ci}`, job `coverage`).",
                diff: " ### Coverage\n \n-Line coverage is measured for the backend and must not fall below 70 %.\n+Line coverage is measured for the backend with `cargo llvm-cov` and for the frontend with\n+Vitest (`in/{web}/vitest.config.ts`). The CI job `coverage` fails below 80 % line coverage\n+for either component (`{ci}`).\n",
            },
            Change {
                chapter: 4,
                summary: "Documented the CI pipeline stages and their triggers, which were missing.",
                diff: " ## Automated execution\n \n-Tests are executed in CI.\n+| Stage | Command | Trigger |\n+|---|---|---|\n+| unit | `cargo nextest run` / `npm run test` | every push and merge request |\n+| integration | `cargo nextest run --test 'api_*'` against a PostgreSQL service container | every push and merge request |\n+| e2e | `npx playwright test` against the staging deployment | merge to `main` |\n+\n+Source: `{ci}` in `in/{svc}` and `in/{web}`.\n",
            },
        ],
        DocKind::Architecture => &[
            Change {
                chapter: 2,
                summary: "Added the ERP system as an external actor (evidence: `in/{svc}/src/erp/client.rs`).",
                diff: " ## External actors\n \n | Actor | Description |\n |---|---|\n | Pharmacy staff | Uses the web UI |\n+| ERP system | Receives purchase orders via REST (`in/{svc}/src/erp/client.rs`) |\n",
            },
            Change {
                chapter: 4,
                summary: "Added interface IF-04 for the ERP connection.",
                diff: " | ID | Interface | Protocol | Authentication |\n |---|---|---|---|\n | IF-01 | Web UI → API | HTTPS/JSON | OIDC bearer token |\n+| IF-04 | API → ERP | HTTPS/JSON | mTLS client certificate (`ERP_CLIENT_CERT`) |\n",
            },
        ],
        DocKind::BackupRecovery => &[
            Change {
                chapter: 2,
                summary: "Documented the backup cronjob schedule and retention as configured.",
                diff: " ## Database backups\n \n-Backups are created regularly.\n+A Kubernetes CronJob (`k8s/cronjob-pg-backup.yaml`) runs `pg_dump` daily at 02:15 UTC and\n+keeps the 14 most recent dumps. Approved retention: [[TODO-001: Retention period approved by the DPO]]\n",
            },
            Change {
                chapter: 3,
                summary: "Described the scripted restore procedure (evidence: `scripts/restore.sh`).",
                diff: " ## Restore\n \n+1. Scale the API deployment to zero replicas.\n+2. Run `scripts/restore.sh <dump>`; it drops and recreates the database and applies the dump.\n+3. Run the migrations check (`sqlx migrate info`) and scale the API back up.\n+\n+Last restore test: [[TODO-002: Date and result of the last restore test]]\n",
            },
        ],
        DocKind::SecurityConcept => &[
            Change {
                chapter: 2,
                summary: "Updated the authentication chapter to the OIDC implementation in the API.",
                diff: " ## Authentication\n \n-Users log in with username and password.\n+Users authenticate via OpenID Connect against the hospital IdP. The API validates the\n+bearer token signature and audience in `in/{svc}/src/auth/oidc.rs`; sessions in the\n+frontend expire after 30 minutes of inactivity (`src/auth/session.ts`).\n",
            },
            Change {
                chapter: 4,
                summary: "Documented dependency scanning in CI.",
                diff: " ## Vulnerability management\n \n+Dependencies are checked with `cargo deny check advisories` and `npm audit` in the CI stage\n+`security` (`{ci}`). Penetration test: [[TODO-003: Reference to the latest pentest report]]\n",
            },
        ],
    }
}

fn fill(template: &str, product: &Product) -> String {
    let svc = repo_name(&product.git.in_repos[0]);
    let web = product.git.in_repos.get(1).map_or(svc, |r| repo_name(r));
    let ci = match product.git.provider {
        Provider::Github => ".github/workflows/ci.yml",
        Provider::Gitlab => ".gitlab-ci.yml",
    };
    template
        .replace("{product}", &product.name)
        .replace("{svc}", svc)
        .replace("{web}", web)
        .replace("{ci}", ci)
}

pub(crate) fn assessment(kind: DocKind, product: &Product) -> String {
    let changed: Vec<String> = changes(kind)
        .iter()
        .map(|c| kind.chapters()[c.chapter].trim_end_matches(".md").to_string())
        .collect();
    format!(
        "Assessment of {} chapters against the template and {} source repositories: {} need an update, the rest are correct and complete. Applying minimal changes now.",
        kind.chapters().len(),
        product.git.in_repos.len(),
        changed.join(", "),
    )
}

pub(crate) fn final_message(kind: DocKind) -> String {
    let todos = todos(kind).len();
    format!(
        "All chapters assessed, {} files changed and re-read, status report written. Overall status: {}. {todos} placeholders need input from people outside the repositories.",
        changes(kind).len(),
        if todos == 0 { "Complete" } else { "Complete with open items" },
    )
}

pub(crate) fn report(product: &Product, kind: DocKind, date: &str, overall: DocStatus) -> String {
    let doc_dir = format!("documents/{}", kind.slug());
    let overall = match overall {
        DocStatus::Complete => "Complete",
        DocStatus::CompleteWithOpenItems => "Complete with open items",
        _ => "Incomplete",
    };
    let mut md = format!(
        "# Regulatory Status Report\n**Document:** {} · **Date:** {date} · **Overall status:** {overall}\n\n## Summary\n",
        kind.title()
    );
    md += &format!(
        "The {} of {} was reviewed against its template and the source repositories {}. {} chapters were updated where the documentation no longer matched the code; the remaining chapters were confirmed. {} placeholders require information that is not available in the repositories.\n\n",
        kind.title().to_lowercase(),
        product.name,
        product
            .git
            .in_repos
            .iter()
            .map(|r| format!("`{}`", repo_name(r)))
            .collect::<Vec<_>>()
            .join(", "),
        changes(kind).len(),
        todos(kind).len(),
    );

    md += "## Open tasks\n";
    for (n, todo) in todos(kind).iter().enumerate() {
        md += &format!(
            "- [ ] **TODO-{:03}** – `{doc_dir}/{}` – {}. Likely provided by: {}. {}\n",
            n + 1,
            kind.chapters()[todo.chapter],
            todo.description,
            todo.role.to_lowercase(),
            todo.reason
        );
    }

    md += "\n## Findings requiring human judgment\n";
    for finding in findings(kind) {
        md += &format!("- {}\n", fill(finding, product));
    }

    md += "\n## Changes made\n";
    let changed: Vec<usize> = changes(kind).iter().map(|c| c.chapter).collect();
    for change in changes(kind) {
        md += &format!("- `{doc_dir}/{}` – {}\n", kind.chapters()[change.chapter], fill(change.summary, product));
    }

    md += "\n## Confirmed without changes\n";
    for (_, chapter) in kind.chapters().iter().enumerate().filter(|(i, _)| !changed.contains(i)) {
        md += &format!("- `{doc_dir}/{chapter}`\n");
    }
    md
}

fn count(diff: &str, prefix: char) -> usize {
    diff.lines().filter(|l| l.starts_with(prefix)).count()
}

pub(crate) fn changed_file(product: &Product, doc_dir: &str, chapter: &str, change: &Change) -> ChangedFile {
    let body = fill(change.diff, product);
    let (additions, deletions) = (count(&body, '+'), count(&body, '-'));
    let context = body.lines().count() - additions;
    let path = format!("{doc_dir}/{chapter}");
    ChangedFile {
        diff: format!("--- a/{path}\n+++ b/{path}\n@@ -12,{} +12,{} @@\n{body}", context, context - deletions + additions),
        path,
        additions,
        deletions,
    }
}

/// a file that didn't exist before, shown as a diff that only adds lines
pub(crate) fn new_file(path: String, contents: &str) -> ChangedFile {
    let lines: Vec<&str> = contents.lines().collect();
    let body: String = lines.iter().map(|l| format!("+{l}\n")).collect();
    ChangedFile {
        diff: format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n{body}", lines.len()),
        path,
        additions: lines.len(),
        deletions: 0,
    }
}

/// what onboarding pushes to a new documentation repository: a README with the conventions the
/// system prompt relies on, and one template per chapter with instruction comments
pub(crate) fn scaffold(product: &Product) -> Vec<ChangedFile> {
    let mut readme = format!(
        "# {} – regulatory documentation\n\nThis repository holds the regulatory documents of {}. It is maintained with regslop:\neach run reviews one document against the source repositories and updates it.\n\n## Source repositories\n",
        product.name, product.name
    );
    for repo in &product.git.in_repos {
        readme += &format!("- {repo}\n");
    }
    readme += "\n## Structure\n";
    for doc in &product.documents {
        readme += &format!("- `{}/` – {}, one file per chapter\n", doc.path, doc.title);
    }
    readme += "\n## Conventions\n- Language: English. Regulatory style: precise, factual, no marketing language.\n- Every chapter keeps the instruction comments (`<!-- Instruction: … -->`) of its template.\n- Missing information is marked as `[[TODO-NNN: what is needed]]`, numbered per document.\n- References to code name the repository, the relative path and the test or function.\n- `REGULATORY_STATUS_REPORT.md` in each document folder describes the latest run only.\n";

    let mut files = vec![new_file("README.md".to_string(), &readme)];
    for doc in &product.documents {
        for (i, chapter) in doc.kind.chapters().iter().enumerate() {
            let heading = chapter
                .trim_end_matches(".md")
                .split_once('-')
                .map_or(*chapter, |(_, rest)| rest)
                .replace('-', " ");
            let mut template = format!("# {}{}\n\n", heading[..1].to_uppercase(), &heading[1..]);
            if i == 0 {
                template += "<!-- Instruction: State purpose and scope of this document, the product name and version, and a document control table (document ID, revision, approver). -->\n";
            } else {
                let focus = doc.kind.focus();
                template += &format!("<!-- Instruction: {} -->\n", focus[(i - 1) % focus.len()]);
            }
            files.push(new_file(format!("{}/{chapter}", doc.path), &template));
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DocKind;

    #[test]
    fn default_system_prompt_is_extracted() {
        let prompt = default_system_prompt();
        assert!(prompt.starts_with("You are a senior regulatory documentation specialist"));
        assert!(!prompt.contains("'''"));
    }

    #[test]
    fn task_prompt_matches_the_example_config() {
        let git = GitSettings {
            provider: Provider::Github,
            in_repos: vec![
                "https://github.com/example-org/inventory-service.git".to_string(),
                "https://github.com/example-org/inventory-web.git".to_string(),
            ],
            out_repo: "https://github.com/example-org/inventory-regulatory.git".to_string(),
        };
        let expected = EXAMPLE_CONFIG
            .split_once("task_prompt = '''\n")
            .and_then(|(_, rest)| rest.split_once("'''"))
            .map(|(prompt, _)| prompt.replace("2026-01-15", "{{date}}"))
            .unwrap();
        assert_eq!(task_prompt(DocKind::TestConcept, &git), expected);
    }
}
