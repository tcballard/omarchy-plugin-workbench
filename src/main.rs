mod coordination;
mod deploy;
mod manifest;
mod marketplace;
mod model;
mod paths;
mod process;
mod publishing;
mod registry;
mod security;
mod test_session;
mod updates;
mod workbench;

use crate::paths::AppPaths;
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "omarchy-plugin-workbench", version, about)]
struct Cli {
    #[arg(long, global = true, help = "Emit a stable JSON result")]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Register an explicit local plugin project.
    Add {
        path: PathBuf,
        #[arg(long)]
        plugin_path: Option<PathBuf>,
        #[arg(long, help = "Trust and permit project-defined check commands")]
        trust_project_checks: bool,
    },
    /// Reload the shared project definition and revoke prior trust and approvals.
    Refresh { id: String },
    /// Forget a registered project without deleting its checkout.
    Remove { id: String },
    /// List registered projects and their live state.
    List,
    /// Alias for list, intended for the native panel.
    Status,
    /// Validate one project against the Omarchy plugin contract.
    Validate { id: String },
    /// Link the mutable source tree into Omarchy for live development.
    Link { id: String },
    /// Deploy an immutable content-addressed snapshot.
    Snapshot { id: String },
    /// Switch back to the preceding managed deployment.
    Rollback { id: String },
    /// Remove the managed Omarchy link while retaining snapshots.
    Undeploy { id: String },
    /// Enable the deployed plugin through Omarchy.
    Enable { id: String },
    /// Disable the deployed plugin through Omarchy.
    Disable { id: String },
    /// Run validation followed by trusted project checks.
    Check {
        id: String,
        #[arg(long)]
        name: Option<String>,
    },
    /// Run a trusted, capability-gated project workflow.
    Workflow { id: String, name: String },
    /// Run trusted project-declared environment probes.
    Environment { id: String },
    /// Locally approve a workflow capability for one project.
    Approve { id: String, capability: String },
    /// Revoke a local workflow capability approval.
    Revoke { id: String, capability: String },
    /// Create an isolated Git worktree for an agent task.
    SessionStart {
        id: String,
        #[arg(long)]
        task: String,
        #[arg(long)]
        agent: Option<String>,
        #[arg(long)]
        objective: String,
    },
    /// List local task sessions, optionally for one project.
    Sessions { id: Option<String> },
    /// Mark a task session closed without deleting its worktree or branch.
    SessionClose { session_id: String },
    /// Launch the plugin in a disposable nested Hyprland window.
    TestSessionStart { id: String },
    /// List disposable nested test sessions, optionally for one project.
    TestSessions { id: Option<String> },
    /// Stop and erase a project's disposable nested test session.
    TestSessionStop { id: String },
    /// Record a structured, agent-neutral continuation handoff.
    Handoff {
        session_id: String,
        #[arg(long)]
        decision: Vec<String>,
        #[arg(long)]
        blocker: Vec<String>,
        #[arg(long)]
        next_action: String,
    },
    /// Read recent verification evidence for a project.
    Evidence {
        id: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Evaluate release readiness without tagging, publishing, or mutating Git.
    ReleaseCheck { id: String },
    /// Prepare a strictly read-only security-review brief at the exact current commit.
    SecurityReviewPrepare {
        id: String,
        #[arg(long, help = "Include the latest review and require fix-by-fix verification")]
        verify_fixes: bool,
    },
    /// Import a completed manual security review into private Workbench state.
    SecurityReviewImport {
        id: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, help = "Confirm the report came from a complete manual review")]
        confirm_manual_review: bool,
    },
    /// Show whether the latest manual security review is current or stale.
    SecurityReviewStatus { id: String },
    /// Combine project, environment, session, and evidence diagnostics.
    Diagnose { id: String },
    /// Allow the exact argv checks declared by a project.
    Trust { id: String },
    /// Revoke permission to run project-defined checks.
    Untrust { id: String },
    /// Show recent shell log lines mentioning a plugin id.
    Logs {
        id: String,
        #[arg(long, default_value_t = 200)]
        lines: usize,
    },
    /// Inspect the host tools and pinned Omarchy contract.
    Doctor,
    /// Show registered development projects and Workbench-managed installations.
    Portfolio,
    /// Fetch and review updates for installed Git-managed plugins.
    Updates { id: Option<String> },
    /// Apply one reviewed update through Omarchy's validator and rollback path.
    Update {
        id: String,
        #[arg(long, help = "Exact remote revision shown by the updates command")]
        revision: String,
        #[arg(long, help = "Confirm the reviewed update")]
        yes: bool,
    },
    /// Apply every safe reviewed update through Omarchy.
    UpdateAll {
        #[arg(
            long = "reviewed",
            value_name = "ID=REVISION",
            help = "A plugin and exact revision shown by the updates command"
        )]
        reviewed: Vec<String>,
        #[arg(long, help = "Confirm all reviewed updates")]
        yes: bool,
    },
    /// Refresh the cached official marketplace catalogue.
    MarketplaceRefresh,
    /// Search the cached official marketplace catalogue locally.
    MarketplaceSearch {
        query: Option<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        tag: Option<String>,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        built_in: bool,
        #[arg(long)]
        verified: bool,
        #[arg(long)]
        installable: bool,
        #[arg(long)]
        installed: bool,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Install and optionally enable one exact reviewed marketplace snapshot.
    MarketplaceInstall {
        id: String,
        #[arg(long, help = "Exact repository URL shown by marketplace-search")]
        repo: String,
        #[arg(long, help = "Exact reviewed revision shown by marketplace-search")]
        revision: String,
        #[arg(long)]
        enable: bool,
        #[arg(long, help = "Confirm installation of the reviewed snapshot")]
        yes: bool,
    },
    /// List marketplace installations owned by Workbench and reviewed updates.
    MarketplaceManaged,
    /// Update one Workbench-managed plugin to an exact marketplace-reviewed commit.
    MarketplaceUpdate {
        id: String,
        #[arg(long)]
        revision: String,
        #[arg(long)]
        yes: bool,
    },
    /// Replace a Workbench-managed installation with the latest reviewed snapshot.
    MarketplaceRepair {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Remove a Workbench-managed installation while retaining a recovery copy.
    MarketplaceUninstall {
        id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Produce an exact, reviewable release plan without tagging or publishing.
    ReleasePlan { id: String },
    /// Generate the current official marketplace submission body.
    SubmissionPrepare {
        id: String,
        #[arg(long)]
        repo: String,
        #[arg(long)]
        category: String,
        #[arg(long = "tag")]
        tags: Vec<String>,
        #[arg(long)]
        suggested_tag: Option<String>,
        #[arg(long)]
        notes: Option<String>,
        #[arg(
            long,
            help = "Confirm all five official submission checklist statements"
        )]
        confirm_checklist: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    if let Err(error) = run(&cli) {
        if error.downcast_ref::<ReportedFailure>().is_some() {
            std::process::exit(1);
        }
        if cli.json {
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "ok": false,
                    "error": format!("{error:#}")
                }))
                .expect("serialize error result")
            );
        } else {
            eprintln!("omarchy-plugin-workbench: {error:#}");
        }
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<()> {
    let paths = AppPaths::discover()?;
    paths.ensure()?;
    match &cli.command {
        Command::Add {
            path,
            plugin_path,
            trust_project_checks,
        } => {
            let project =
                registry::add_project(&paths, path, plugin_path.as_deref(), *trust_project_checks)?;
            emit(
                cli.json,
                &json!({
                    "ok": true,
                    "action": "add",
                    "project": project,
                    "checksRequireTrust": !project.checks.is_empty() && !project.project_checks_trusted
                }),
                &format!("Registered {} ({})", project.name, project.id),
            )
        }
        Command::Remove { id } => {
            let registry_config = registry::load(&paths)?;
            let project = registry::find_project(&registry_config, id)?;
            if workbench::managed_deployment_exists(&paths, project)? {
                bail!("project is still deployed; run undeploy before remove");
            }
            if test_session::active_count(&paths, &project.id)? > 0 {
                bail!("project has a running nested test session; stop it before remove");
            }
            let project = registry::remove_project(&paths, id)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "remove", "project": project}),
                &format!("Forgot {} without deleting its checkout", project.id),
            )
        }
        Command::Refresh { id } => {
            let project = registry::refresh_project(&paths, id)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "refresh", "project": project}),
                &format!(
                    "Refreshed {}; command trust and capability approvals were revoked",
                    project.id
                ),
            )
        }
        Command::List | Command::Status => {
            let registry_config = registry::load(&paths)?;
            let statuses = workbench::project_statuses(&paths, &registry_config.projects)?;
            if cli.json {
                print_json(&statuses)
            } else if statuses.is_empty() {
                println!("No projects registered. Add one with: omarchy-plugin-workbench add PATH");
                Ok(())
            } else {
                println!(
                    "{:<36} {:<13} {:<9} REVISION",
                    "PLUGIN", "DEPLOYMENT", "STATE"
                );
                for status in statuses {
                    let state = match status.enabled {
                        Some(true) => "enabled",
                        Some(false) => "disabled",
                        None => "unknown",
                    };
                    let revision = status.revision.as_deref().unwrap_or("-");
                    println!(
                        "{:<36} {:<13} {:<9} {}{}",
                        status.id,
                        status.deployment,
                        state,
                        &revision[..revision.len().min(12)],
                        if status.dirty { " dirty" } else { "" }
                    );
                }
                Ok(())
            }
        }
        Command::Validate { id } => {
            let registry_config = registry::load(&paths)?;
            let project = registry::find_project(&registry_config, id)?;
            let report = deploy::validate(project)?;
            emit(
                cli.json,
                &report,
                &format!("{} passed validation", report.plugin_id),
            )
        }
        Command::Link { id } => with_project(&paths, id, |project| {
            let report = deploy::deploy_live(&paths, project)?;
            emit(cli.json, &report, &report.message)
        }),
        Command::Snapshot { id } => with_project(&paths, id, |project| {
            let report = deploy::deploy_snapshot(&paths, project)?;
            emit(cli.json, &report, &report.message)
        }),
        Command::Rollback { id } => with_project(&paths, id, |project| {
            let report = deploy::rollback(&paths, project)?;
            emit(cli.json, &report, &report.message)
        }),
        Command::Undeploy { id } => with_project(&paths, id, |project| {
            let report = deploy::undeploy(&paths, project)?;
            emit(cli.json, &report, &report.message)
        }),
        Command::Enable { id } => with_project(&paths, id, |project| {
            let report = workbench::enable_project(project)?;
            emit(cli.json, &report, &report.message)
        }),
        Command::Disable { id } => with_project(&paths, id, |project| {
            let report = workbench::disable_project(project)?;
            emit(cli.json, &report, &report.message)
        }),
        Command::Check { id, name } => with_project(&paths, id, |project| {
            let report = workbench::run_project_checks(&paths, project, name.as_deref())?;
            let message = if report.ok {
                format!("{} checks passed", report.project_id)
            } else {
                format!("{} checks failed", report.project_id)
            };
            emit(cli.json, &report, &message)?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
        Command::Workflow { id, name } => with_project(&paths, id, |project| {
            let report = workbench::run_workflow(&paths, project, name)?;
            emit(
                cli.json,
                &report,
                &format!(
                    "{} workflow {}",
                    name,
                    if report.ok { "passed" } else { "failed" }
                ),
            )?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
        Command::Environment { id } => with_project(&paths, id, |project| {
            let report = workbench::inspect_environment(&paths, project)?;
            emit(
                cli.json,
                &report,
                if report.ok {
                    "Project environment is ready"
                } else {
                    "Project environment is missing requirements"
                },
            )?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
        Command::Approve { id, capability } => {
            let project = registry::set_capability_approval(&paths, id, capability, true)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "approve", "capability": capability, "project": project}),
                &format!("Approved {capability} for {}", project.id),
            )
        }
        Command::Revoke { id, capability } => {
            let project = registry::set_capability_approval(&paths, id, capability, false)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "revoke", "capability": capability, "project": project}),
                &format!("Revoked {capability} for {}", project.id),
            )
        }
        Command::SessionStart {
            id,
            task,
            agent,
            objective,
        } => with_project(&paths, id, |project| {
            let session =
                coordination::start_session(&paths, project, task, agent.as_deref(), objective)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "session-start", "session": session}),
                &format!(
                    "Created {} at {}",
                    session.branch,
                    session.worktree.display()
                ),
            )
        }),
        Command::Sessions { id } => {
            let sessions = coordination::list_sessions(&paths, id.as_deref())?;
            emit(
                cli.json,
                &sessions,
                &format!("{} task sessions", sessions.len()),
            )
        }
        Command::SessionClose { session_id } => {
            let session = coordination::close_session(&paths, session_id)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "session-close", "session": session}),
                &format!("Closed session {}; worktree retained", session.id),
            )
        }
        Command::TestSessionStart { id } => with_project(&paths, id, |project| {
            let session = test_session::start(&paths, project)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "test-session-start", "session": session}),
                &format!(
                    "Started disposable nested test session {} (not a security sandbox)",
                    session.id
                ),
            )
        }),
        Command::TestSessions { id } => {
            let sessions = test_session::list(&paths, id.as_deref())?;
            emit(
                cli.json,
                &sessions,
                &format!("{} nested test sessions", sessions.len()),
            )
        }
        Command::TestSessionStop { id } => with_project(&paths, id, |project| {
            let session = test_session::stop(&paths, project)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "test-session-stop", "session": session}),
                &format!("Stopped and erased nested test session {}", session.id),
            )
        }),
        Command::Handoff {
            session_id,
            decision,
            blocker,
            next_action,
        } => {
            let handoff = coordination::write_handoff(
                &paths,
                session_id,
                decision.clone(),
                blocker.clone(),
                next_action,
            )?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "handoff", "handoff": handoff}),
                &format!("Recorded handoff for {}", handoff.session_id),
            )
        }
        Command::Evidence { id, limit } => {
            let evidence = coordination::read_evidence(&paths, id, *limit)?;
            emit(
                cli.json,
                &evidence,
                &format!("{} evidence records", evidence.len()),
            )
        }
        Command::ReleaseCheck { id } => with_project(&paths, id, |project| {
            let report = workbench::release_readiness(&paths, project)?;
            emit(
                cli.json,
                &report,
                if report.ok {
                    "Project is release-ready"
                } else {
                    "Project is not release-ready"
                },
            )?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
        Command::SecurityReviewPrepare { id, verify_fixes } => {
            with_project(&paths, id, |project| {
                let report = security::prepare(&paths, project, *verify_fixes)?;
                emit(
                    cli.json,
                    &report,
                    &format!(
                        "Read-only security review brief prepared at {}",
                        report.prompt_file.display()
                    ),
                )
            })
        }
        Command::SecurityReviewImport {
            id,
            file,
            confirm_manual_review,
        } => with_project(&paths, id, |project| {
            let report = security::import_review(
                &paths,
                project,
                file,
                *confirm_manual_review,
            )?;
            emit(
                cli.json,
                &report,
                &format!("Security review recorded as {}", report.status),
            )
        }),
        Command::SecurityReviewStatus { id } => with_project(&paths, id, |project| {
            let report = security::status(&paths, project)?;
            emit(
                cli.json,
                &report,
                &format!("Security review status: {}", report.status),
            )
        }),
        Command::Diagnose { id } => with_project(&paths, id, |project| {
            let enabled = std::collections::HashMap::new();
            let status = workbench::project_status(&paths, project, &enabled)?;
            let environment = workbench::inspect_environment(&paths, project)?;
            let sessions = coordination::list_sessions(&paths, Some(&project.id))?;
            let test_sessions = test_session::list(&paths, Some(&project.id))?;
            let evidence = coordination::read_evidence(&paths, &project.id, 10)?;
            let ok = environment.ok;
            let report = json!({"ok": ok, "project": status, "environment": environment, "sessions": sessions, "testSessions": test_sessions, "recentEvidence": evidence});
            emit(
                cli.json,
                &report,
                if ok {
                    "Project diagnostics are healthy"
                } else {
                    "Project diagnostics found environment problems"
                },
            )?;
            if ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
        Command::Trust { id } => {
            let project = registry::set_trust(&paths, id, true)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "trust", "project": project}),
                &format!("Trusted {} project checks", project.id),
            )
        }
        Command::Untrust { id } => {
            let project = registry::set_trust(&paths, id, false)?;
            emit(
                cli.json,
                &json!({"ok": true, "action": "untrust", "project": project}),
                &format!("Revoked trust for {} project checks", project.id),
            )
        }
        Command::Logs { id, lines } => with_project(&paths, id, |project| {
            let report = workbench::logs(project, *lines)?;
            emit(cli.json, &report, &report.output)
        }),
        Command::Doctor => {
            let report = workbench::doctor(&paths);
            let message = if report.ok {
                "Workbench host is ready"
            } else {
                "Workbench host is missing required Omarchy tools"
            };
            emit(cli.json, &report, message)
        }
        Command::Portfolio => {
            let registry_config = registry::load(&paths)?;
            let projects = workbench::project_statuses(&paths, &registry_config.projects)?;
            let marketplace = marketplace::managed(&paths)?;
            let report = json!({
                "ok": marketplace.ok,
                "registeredProjects": projects.len(),
                "managedInstallations": marketplace.managed,
                "marketplaceUpdatesAvailable": marketplace.updates_available,
                "projects": projects,
                "marketplace": marketplace.plugins
            });
            emit(cli.json, &report, "Workbench portfolio loaded")
        }
        Command::Updates { id } => {
            let report = updates::inspect(&paths, id.as_deref())?;
            let message = format!(
                "{} update(s) available; {} plugin(s) need attention",
                report.available, report.blocked
            );
            emit(cli.json, &report, &message)
        }
        Command::Update { id, revision, yes } => {
            let report = updates::apply_one(&paths, id, revision, *yes)?;
            emit(cli.json, &report, &report.message)
        }
        Command::UpdateAll { reviewed, yes } => {
            let report = updates::apply_all(&paths, reviewed, *yes)?;
            emit(cli.json, &report, &report.message)?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }
        Command::MarketplaceRefresh => {
            let report = marketplace::refresh(&paths)?;
            emit(cli.json, &report, &report.message)
        }
        Command::MarketplaceSearch {
            query,
            category,
            tag,
            kind,
            built_in,
            verified,
            installable,
            installed,
            limit,
        } => {
            let filters = marketplace::SearchFilters {
                query: query.as_deref(),
                category: category.as_deref(),
                tag: tag.as_deref(),
                kind: kind.as_deref(),
                built_in_only: *built_in,
                verified_only: *verified,
                installable_only: *installable,
                installed_only: *installed,
                limit: *limit,
            };
            let report = marketplace::search(&paths, &filters)?;
            emit(
                cli.json,
                &report,
                &format!("{} marketplace result(s)", report.returned),
            )
        }
        Command::MarketplaceInstall {
            id,
            repo,
            revision,
            enable,
            yes,
        } => {
            let report = marketplace::install(&paths, id, repo, revision, *enable, *yes)?;
            emit(cli.json, &report, &report.message)
        }
        Command::MarketplaceManaged => {
            let report = marketplace::managed(&paths)?;
            emit(
                cli.json,
                &report,
                &format!(
                    "{} Workbench-managed plugin(s); {} reviewed update(s)",
                    report.managed, report.updates_available
                ),
            )
        }
        Command::MarketplaceUpdate { id, revision, yes } => {
            let report = marketplace::update_managed(&paths, id, revision, *yes)?;
            emit(cli.json, &report, &report.message)
        }
        Command::MarketplaceRepair { id, yes } => {
            let report = marketplace::repair(&paths, id, *yes)?;
            emit(cli.json, &report, &report.message)
        }
        Command::MarketplaceUninstall { id, yes } => {
            let report = marketplace::uninstall(&paths, id, *yes)?;
            emit(cli.json, &report, &report.message)
        }
        Command::ReleasePlan { id } => with_project(&paths, id, |project| {
            let report = publishing::release_plan(&paths, project)?;
            emit(
                cli.json,
                &report,
                if report.ok {
                    "Release plan is ready for review"
                } else {
                    "Release plan has blockers"
                },
            )?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
        Command::SubmissionPrepare {
            id,
            repo,
            category,
            tags,
            suggested_tag,
            notes,
            confirm_checklist,
        } => with_project(&paths, id, |project| {
            let report = publishing::submission_draft(
                &paths,
                project,
                repo,
                category,
                tags,
                suggested_tag.as_deref(),
                notes.as_deref(),
                *confirm_checklist,
            )?;
            emit(
                cli.json,
                &report,
                if report.ok {
                    "Marketplace submission draft is ready for owner review"
                } else {
                    "Marketplace submission draft has blockers"
                },
            )?;
            if report.ok {
                Ok(())
            } else {
                Err(ReportedFailure.into())
            }
        }),
    }
}

fn with_project<F>(paths: &AppPaths, id: &str, operation: F) -> Result<()>
where
    F: FnOnce(&model::Project) -> Result<()>,
{
    let registry_config = registry::load(paths)?;
    let project = registry::find_project(&registry_config, id)?;
    operation(project)
}

fn emit<T: Serialize>(json: bool, value: &T, human: &str) -> Result<()> {
    if json {
        print_json(value)
    } else {
        println!("{human}");
        Ok(())
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(value).context("serialize JSON output")?
    );
    Ok(())
}

#[derive(Debug)]
struct ReportedFailure;

impl fmt::Display for ReportedFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("command failed; details were already reported")
    }
}

impl std::error::Error for ReportedFailure {}
