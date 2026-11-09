use std::path::Path;

use super::*;
use crate::core as c;
use c::types::*;

// ---------------------------------------------------------------------------
// task / approval
// ---------------------------------------------------------------------------

fn slugify_title(title: &str, max: usize) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in title.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(max).collect();
    if slug.is_empty() {
        format!("task-{}", c::now_millis())
    } else {
        slug
    }
}

pub(super) fn task(cmd: TaskCommand) -> Result<(), String> {
    match cmd.command {
        TaskSubcommand::Add {
            project,
            title,
            from_ref,
            kind,
            risk,
            scope,
            depends_on,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let now = c::now_iso();

            let r#ref = from_ref.clone();
            let title_str = match (&title, &r#ref) {
                // TS truthiness: an empty --title is treated as absent.
                (Some(t), _) if !t.trim().is_empty() => t.clone(),
                (_, Some(r)) => format!("Queued work from {r}"),
                _ => {
                    eprintln!("Error: provide --title or --from-ref");
                    std::process::exit(1);
                }
            };

            // W10: Validate scope paths are relative and free of traversal.
            let mut raw_scope: Vec<String> = Vec::new();
            for path in &scope {
                if !raw_scope.contains(path) {
                    raw_scope.push(path.clone());
                }
            }
            for scope_path in &raw_scope {
                if scope_path.starts_with('/') || scope_path.starts_with('\\') {
                    eprintln!("Error: scope path must be relative, got: {scope_path}");
                    std::process::exit(1);
                }
                if scope_path.split('/').any(|segment| segment == "..") {
                    eprintln!("Error: scope path must not contain '..', got: {scope_path}");
                    std::process::exit(1);
                }
            }

            let mut unique_deps: Vec<String> = Vec::new();
            for dep in &depends_on {
                if !unique_deps.contains(dep) {
                    unique_deps.push(dep.clone());
                }
            }

            let mut task = ProjectTask {
                id: slugify_title(&title_str, 64),
                title: title_str,
                kind: TaskKind::parse(&kind).ok_or_else(|| format!("Invalid kind: {kind}"))?,
                status: TaskStatus::Proposed,
                risk: TaskRisk::parse(&risk).ok_or_else(|| format!("Invalid risk: {risk}"))?,
                scope: (!raw_scope.is_empty()).then_some(TaskScope { paths: raw_scope }),
                source: match &r#ref {
                    Some(r) => TaskSource {
                        source_type: "issue".to_string(),
                        r#ref: r.clone(),
                    },
                    None => TaskSource {
                        source_type: "human".to_string(),
                        r#ref: "openloop task add".to_string(),
                    },
                },
                spec_id: None,
                branch: None,
                owner: Some("openloop".to_string()),
                acceptance_criteria: vec!["To be defined during planning.".to_string()],
                attempts: 0,
                last_failure_signature: None,
                promotion: TaskPromotion::PullRequest,
                promoted_at: None,
                depends_on: (!unique_deps.is_empty()).then_some(unique_deps),
                estimated_cost_usd: None,
                notes: if r#ref.is_some() {
                    Some(vec!["Created by openloop task add --from-ref.".to_string()])
                } else {
                    Some(Vec::new())
                },
                last_run: None,
                created_at: now.clone(),
                updated_at: now,
            };

            c::task_ledger::add_task(Path::new(&project.path), &mut task)?;
            println!("Added task {} to {}", task.id, project.alias);
            Ok(())
        }
        TaskSubcommand::List {
            project,
            status,
            risk,
            format,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let filters = c::task_ledger::TaskListFilters {
                status: status.as_deref().and_then(TaskStatus::parse),
                risk: risk.as_deref().and_then(TaskRisk::parse),
            };
            let tasks = c::task_ledger::list_tasks(Path::new(&project.path), &filters)?;
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = tasks
                        .iter()
                        .map(|task| {
                            c::table::Row(vec![
                                task.id.clone(),
                                task.status.as_str().to_string(),
                                task.risk.as_str().to_string(),
                                task.kind.as_str().to_string(),
                                task.promotion.as_str().to_string(),
                                task.title.chars().take(40).collect(),
                            ])
                        })
                        .collect();
                    c::table::print_table(
                        &["id", "status", "risk", "kind", "promotion", "title"],
                        &rows,
                    );
                }
                c::table::OutputFormat::Json => {
                    let summary = c::task_ledger::summarize_tasks(&tasks);
                    let mapped: Vec<serde_json::Value> = tasks
                        .iter()
                        .map(|task| {
                            serde_json::json!({
                                "id": task.id,
                                "title": task.title,
                                "status": task.status.as_str(),
                                "risk": task.risk.as_str(),
                                "kind": task.kind.as_str(),
                                "scopePaths": task.scope.as_ref().map(|s| s.paths.clone()).unwrap_or_default(),
                                "promotion": task.promotion.as_str(),
                                "updatedAt": task.updated_at,
                                "promotedAt": task.promoted_at,
                            })
                        })
                        .collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "summary": {
                                "total": summary.total,
                                "byStatus": ordered_status_map(&summary),
                                "byRisk": ordered_risk_map(&summary),
                            },
                            "tasks": mapped,
                        }))
                        .unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        TaskSubcommand::Show { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            let inspection =
                c::task_inspection::get_task_inspection(Path::new(&project.path), &task)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&inspection).unwrap_or_default()
            );
            Ok(())
        }
        TaskSubcommand::Update {
            project,
            task,
            title,
            status,
            risk,
            kind,
        } => {
            let project = c::project_registry::get_project(&project, None)?;
            let patch = c::task_ledger::TaskPatch {
                // TS truthiness: an empty --title is a no-op, not a blanking.
                title: title.filter(|t| !t.trim().is_empty()),
                status: status.as_deref().and_then(TaskStatus::parse),
                risk: risk.as_deref().and_then(TaskRisk::parse),
                kind: kind.as_deref().and_then(TaskKind::parse),
                scope: None,
            };
            let updated = c::task_ledger::update_task(Path::new(&project.path), &task, &patch)?;
            println!("Updated task {}", updated.id);
            Ok(())
        }
        TaskSubcommand::Remove { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            c::task_ledger::remove_task(Path::new(&project.path), &task)?;
            println!("Removed task {task} from {}", project.alias);
            Ok(())
        }
        TaskSubcommand::Recover { project } => {
            let project = c::project_registry::get_project(&project, None)?;
            let recovered = c::task_ledger::with_task_ledger(Path::new(&project.path), |ledger| {
                let mut recovered = Vec::new();
                for task in ledger.tasks.iter_mut() {
                    if task.status != TaskStatus::InProgress {
                        continue;
                    }
                    task.status = TaskStatus::Ready;
                    let mut notes = task.notes.clone().unwrap_or_default();
                    notes.push("Manually recovered via 'task recover'.".to_string());
                    task.notes = Some(notes);
                    task.updated_at = c::now_iso();
                    recovered.push(task.id.clone());
                }
                Ok(recovered)
            })?;
            if recovered.is_empty() {
                println!("No in_progress tasks found in {}.", project.alias);
                return Ok(());
            }
            println!(
                "Recovered {} task(s) in {}: {}",
                recovered.len(),
                project.alias,
                recovered.join(", ")
            );
            Ok(())
        }
        TaskSubcommand::Approve { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            let approved_id =
                c::task_ledger::with_task_ledger(Path::new(&project.path), |ledger| {
                    let found = ledger
                        .tasks
                        .iter_mut()
                        .find(|t| t.id == task)
                        .ok_or_else(|| format!("Task {task} not found in {}.", project.alias))?;
                    if found.status != TaskStatus::AwaitingApproval {
                        return Err(format!(
                            "Task {} is not awaiting approval (current status: {}).",
                            found.id,
                            found.status.as_str()
                        ));
                    }
                    found.status = TaskStatus::Ready;
                    let mut notes = found.notes.clone().unwrap_or_default();
                    notes.push("Approved via 'task approve'.".to_string());
                    found.notes = Some(notes);
                    found.updated_at = c::now_iso();
                    Ok(found.id.clone())
                });
            match approved_id {
                Ok(approved_id) => {
                    println!(
                        "Approved task {approved_id} in {} — now ready for implementation.",
                        project.alias
                    );
                    Ok(())
                }
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
    }
}

pub(super) fn approval(cmd: ApprovalCommand) -> Result<(), String> {
    match cmd.command {
        ApprovalSubcommand::List { project, format } => {
            let project = c::project_registry::get_project(&project, None)?;
            let packets = c::approval_packets::list_approval_packets(Path::new(&project.path))?;
            match c::table::resolve_output_format(format.as_deref()) {
                c::table::OutputFormat::Table => {
                    let rows: Vec<c::table::Row> = packets
                        .iter()
                        .map(|(_, packet)| {
                            c::table::Row(vec![
                                packet.task_id.clone(),
                                packet.risk.as_str().to_string(),
                                packet.attempts.to_string(),
                                format!("{:.4}", packet.cost_usd),
                                packet.cost_source.as_str().to_string(),
                                packet.branch.clone().unwrap_or_else(|| "-".into()),
                                packet.title.chars().take(40).collect(),
                            ])
                        })
                        .collect();
                    c::table::print_table(
                        &[
                            "taskId",
                            "risk",
                            "attempts",
                            "costUsd",
                            "costSource",
                            "branch",
                            "title",
                        ],
                        &rows,
                    );
                }
                c::table::OutputFormat::Json => {
                    let mapped: Vec<&c::approval_packets::ApprovalPacket> =
                        packets.iter().map(|(_, packet)| packet).collect();
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&mapped).unwrap_or_default()
                    );
                }
            }
            Ok(())
        }
        ApprovalSubcommand::Show { project, task } => {
            let project = c::project_registry::get_project(&project, None)?;
            match c::approval_packets::read_approval_packet(Path::new(&project.path), &task)? {
                Some(packet) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&packet).unwrap_or_default()
                    );
                    Ok(())
                }
                None => {
                    eprintln!("No approval packet for task {task} in {}.", project.alias);
                    std::process::exit(1);
                }
            }
        }
    }
}

/// TS emitted the task summary maps in fixed declaration order; BTreeMap
/// would alphabetize the keys. Rebuild them as ordered JSON objects.
fn ordered_status_map(summary: &c::task_ledger::TaskListSummary) -> serde_json::Value {
    const ORDER: [&str; 10] = [
        "proposed",
        "planned",
        "ready",
        "awaiting-approval",
        "in_progress",
        "blocked",
        "done",
        "failed",
        "cancelled",
        "promoted",
    ];
    let mut map = serde_json::Map::new();
    for key in ORDER {
        map.insert(
            key.into(),
            summary.by_status.get(key).copied().unwrap_or(0).into(),
        );
    }
    serde_json::Value::Object(map)
}

fn ordered_risk_map(summary: &c::task_ledger::TaskListSummary) -> serde_json::Value {
    const ORDER: [&str; 3] = ["low-risk", "medium-risk", "high-risk"];
    let mut map = serde_json::Map::new();
    for key in ORDER {
        map.insert(
            key.into(),
            summary.by_risk.get(key).copied().unwrap_or(0).into(),
        );
    }
    serde_json::Value::Object(map)
}
