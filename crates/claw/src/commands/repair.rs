use clap::{Args, Subcommand};

use claw_store::ClawStore;

use crate::config::find_repo_root;

use super::repo_health;

#[derive(Args)]
pub struct RepairArgs {
    /// Output command results as JSON
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: RepairCommand,
}

#[derive(Subcommand)]
enum RepairCommand {
    /// Print detected repository issues and safe repair plans
    Plan,
    /// Apply safe repairs
    Apply {
        /// Preview repairs without changing refs
        #[arg(long)]
        dry_run: bool,
    },
}

pub fn run(args: RepairArgs) -> anyhow::Result<()> {
    match args.command {
        RepairCommand::Plan => run_plan(args.json),
        RepairCommand::Apply { dry_run } => run_apply(args.json, dry_run),
    }
}

fn run_plan(json: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let report = repo_health::scan(&store);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "repair.plan",
                "object_count": report.object_count,
                "ref_count": report.ref_count,
                "issue_count": report.issue_count,
                "repairable_count": report.repairable_count,
                "summary": report.summary,
                "issues": report.issues,
            }))?
        );
    } else {
        print_plan(&report);
    }

    Ok(())
}

fn run_apply(json: bool, dry_run: bool) -> anyhow::Result<()> {
    let root = find_repo_root()?;
    let store = ClawStore::open(&root)?;
    let report = repo_health::scan(&store);
    let planned = report
        .issues
        .iter()
        .filter_map(|issue| issue.repair.as_ref())
        .filter(|repair| repo_health::is_safe_automatic_repair(repair))
        .cloned()
        .collect::<Vec<_>>();
    let applied = if dry_run {
        Vec::new()
    } else {
        repo_health::apply_safe_repairs(&store)?
    };
    let post_report = if dry_run {
        None
    } else {
        Some(repo_health::scan(&store))
    };
    let post_summary = post_report.as_ref().map(|report| &report.summary);
    let remaining_issue_count = post_report.as_ref().map(|report| report.issue_count);
    let remaining_repairable_count = post_report.as_ref().map(|report| report.repairable_count);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "schema_version": 1,
                "action": "repair.apply",
                "dry_run": dry_run,
                "planned_count": planned.len(),
                "applied_count": applied.len(),
                "summary": report.summary,
                "post_summary": post_summary,
                "remaining_issue_count": remaining_issue_count,
                "remaining_repairable_count": remaining_repairable_count,
                "planned": planned,
                "applied": applied,
            }))?
        );
    } else if dry_run {
        println!("Dry run: would apply {} safe repair(s).", planned.len());
        print_repairs(&planned);
    } else {
        println!("Applied {} safe repair(s).", applied.len());
        print_repairs(&applied);
        if let Some(report) = post_report {
            println!(
                "Remaining after repair: {} issue(s), {} safe repair(s).",
                report.issue_count, report.repairable_count
            );
        }
    }

    Ok(())
}

fn print_plan(report: &repo_health::DeepHealthReport) {
    println!(
        "Repository scan: {} object(s), {} ref(s), {} issue(s), {} safe repair(s)",
        report.object_count, report.ref_count, report.issue_count, report.repairable_count
    );
    if report.issues.is_empty() {
        println!("No repository issues found.");
        return;
    }

    for issue in &report.issues {
        println!("{} [{}]: {}", issue.code, issue.severity, issue.message);
        if let Some(repair) = &issue.repair {
            println!("  Repair: {} - {}", repair.kind, repair.description);
            if repo_health::is_safe_automatic_repair(repair) {
                println!("  Try: claw repair apply --dry-run");
            }
        }
    }
}

fn print_repairs(repairs: &[repo_health::RepairPlan]) {
    if repairs.is_empty() {
        println!("No safe repairs available.");
        return;
    }
    for repair in repairs {
        println!("{}: {}", repair.kind, repair.description);
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{RepairArgs, RepairCommand};

    #[derive(Parser)]
    struct TestCli {
        #[command(flatten)]
        args: RepairArgs,
    }

    #[test]
    fn parses_repair_plan_json() {
        let cli = TestCli::parse_from(["claw", "--json", "plan"]);
        assert!(cli.args.json);
        assert!(matches!(cli.args.command, RepairCommand::Plan));
    }

    #[test]
    fn parses_repair_apply_dry_run() {
        let cli = TestCli::parse_from(["claw", "apply", "--dry-run"]);
        match cli.args.command {
            RepairCommand::Apply { dry_run } => assert!(dry_run),
            RepairCommand::Plan => panic!("expected apply"),
        }
    }
}
