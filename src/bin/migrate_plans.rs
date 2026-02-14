//! Plan Migration Utility
//!
//! Migrates existing JSON plan files to the database.
//!
//! Usage: cargo run --bin migrate_plans -- --data-dir ~/.opencrab --working-dir .

use anyhow::{Context, Result};
use clap::Parser;
use opencrab::db::Database;
use opencrab::services::PlanService;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Data directory containing the database
    #[arg(long, default_value = "~/.opencrab")]
    data_dir: String,

    /// Working directory to search for plan JSON files
    #[arg(long, default_value = ".")]
    working_dir: PathBuf,

    /// Dry run - don't actually migrate, just show what would be migrated
    #[arg(long)]
    dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let args = Args::parse();

    // Expand tilde in data_dir
    let data_dir = shellexpand::tilde(&args.data_dir);
    let data_dir = PathBuf::from(data_dir.as_ref());

    let db_path = data_dir.join("opencrab.db");

    tracing::info!("Connecting to database: {:?}", db_path);
    let db = Database::connect(&db_path).await?;
    db.run_migrations().await?;

    let plan_service = PlanService::new(opencrab::services::ServiceContext::new(db.pool().clone()));

    // Find all .opencrab_plan_*.json files in the working directory
    let pattern = args
        .working_dir
        .join(".opencrab_plan_*.json")
        .to_string_lossy()
        .into_owned();

    tracing::info!("Searching for plan files: {}", pattern);

    let mut migrated_count = 0;
    let mut failed_count = 0;
    let mut skipped_count = 0;

    for entry in glob::glob(&pattern).context("Failed to read glob pattern")? {
        match entry {
            Ok(path) => {
                tracing::info!("Found plan file: {:?}", path);

                match migrate_plan_file(&plan_service, &path, args.dry_run).await {
                    Ok(migrated) => {
                        if migrated {
                            migrated_count += 1;
                        } else {
                            skipped_count += 1;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to migrate {:?}: {}", path, e);
                        failed_count += 1;
                    }
                }
            }
            Err(e) => {
                tracing::error!("Glob error: {}", e);
            }
        }
    }

    tracing::info!(
        "Migration complete: {} migrated, {} skipped, {} failed",
        migrated_count,
        skipped_count,
        failed_count
    );

    if args.dry_run {
        tracing::info!("Dry run - no changes were made");
    }

    Ok(())
}

async fn migrate_plan_file(
    plan_service: &PlanService,
    path: &std::path::Path,
    dry_run: bool,
) -> Result<bool> {
    // Read the JSON file
    let plan = plan_service.import_from_json(path).await?;

    // Check if plan already exists in database
    if let Ok(Some(existing)) = plan_service.find_by_id(plan.id).await {
        tracing::info!(
            "Plan {} already exists in database (updated: {}), skipping",
            plan.id,
            existing.updated_at
        );
        return Ok(false);
    }

    tracing::info!("Migrating plan: {} - {}", plan.id, plan.title);
    tracing::info!("  Session: {}", plan.session_id);
    tracing::info!("  Tasks: {}", plan.tasks.len());
    tracing::info!("  Status: {:?}", plan.status);

    if !dry_run {
        plan_service
            .create(&plan)
            .await
            .context("Failed to create plan in database")?;
        tracing::info!("✓ Migrated successfully");
    } else {
        tracing::info!("✓ Would migrate (dry run)");
    }

    Ok(true)
}
