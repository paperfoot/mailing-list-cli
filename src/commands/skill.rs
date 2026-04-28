use crate::cli::SkillAction;
use crate::error::AppError;
use crate::output::{self, Format};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const SKILL_NAME: &str = "mailing-list-cli";
const SKILL_FILE: &str = "SKILL.md";
const SKILL_CONTENT: &str = include_str!("../../assets/mailing-list-cli-skill.md");

#[derive(Debug, Serialize)]
struct SkillTarget {
    platform: String,
    path: PathBuf,
    exists: bool,
    current: bool,
}

pub fn run(format: Format, action: SkillAction) -> Result<(), AppError> {
    match action {
        SkillAction::Install => install(format),
        SkillAction::Status => status(format),
    }
}

fn install(format: Format) -> Result<(), AppError> {
    let targets = skill_targets()?;
    for target in &targets {
        if let Some(parent) = target.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| AppError::Transient {
                code: "skill_dir_create_failed".into(),
                message: format!("could not create {}: {e}", parent.display()),
                suggestion:
                    "Check filesystem permissions or set MLC_SKILL_ROOTS to writable skill roots"
                        .into(),
            })?;
        }
        std::fs::write(&target.path, SKILL_CONTENT).map_err(|e| AppError::Transient {
            code: "skill_write_failed".into(),
            message: format!("could not write {}: {e}", target.path.display()),
            suggestion: "Check filesystem permissions or set MLC_SKILL_ROOTS to writable skill roots".into(),
        })?;
    }

    let installed = target_statuses()?;
    output::success(
        format,
        "skill installed",
        json!({
            "skill": SKILL_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "sha256": skill_sha256(),
            "targets": installed,
        }),
    );
    Ok(())
}

fn status(format: Format) -> Result<(), AppError> {
    let targets = target_statuses()?;
    let current_count = targets.iter().filter(|t| t.current).count();
    output::success(
        format,
        "skill status",
        json!({
            "skill": SKILL_NAME,
            "version": env!("CARGO_PKG_VERSION"),
            "sha256": skill_sha256(),
            "current_count": current_count,
            "target_count": targets.len(),
            "targets": targets,
        }),
    );
    Ok(())
}

fn target_statuses() -> Result<Vec<SkillTarget>, AppError> {
    skill_targets()?
        .into_iter()
        .map(|mut target| {
            target.exists = target.path.exists();
            target.current = if target.exists {
                std::fs::read_to_string(&target.path)
                    .map(|s| s == SKILL_CONTENT)
                    .unwrap_or(false)
            } else {
                false
            };
            Ok(target)
        })
        .collect()
}

fn skill_targets() -> Result<Vec<SkillTarget>, AppError> {
    if let Ok(raw) = std::env::var("MLC_SKILL_ROOTS") {
        let targets = raw
            .split(':')
            .filter(|s| !s.trim().is_empty())
            .enumerate()
            .map(|(idx, root)| SkillTarget {
                platform: format!("custom_{}", idx + 1),
                path: PathBuf::from(root).join(SKILL_NAME).join(SKILL_FILE),
                exists: false,
                current: false,
            })
            .collect::<Vec<_>>();
        if !targets.is_empty() {
            return Ok(targets);
        }
    }

    let home = dirs::home_dir().ok_or_else(|| AppError::Config {
        code: "home_dir_missing".into(),
        message: "could not determine home directory for skill installation".into(),
        suggestion: "Set HOME or set MLC_SKILL_ROOTS to one or more writable skill roots".into(),
    })?;

    Ok([
        ("codex", ".codex/skills"),
        ("claude", ".claude/skills"),
        ("gemini", ".gemini/skills"),
        ("agents", ".agents/skills"),
    ]
    .into_iter()
    .map(|(platform, rel)| SkillTarget {
        platform: platform.to_string(),
        path: home.join(rel).join(SKILL_NAME).join(SKILL_FILE),
        exists: false,
        current: false,
    })
    .collect())
}

fn skill_sha256() -> String {
    let digest = Sha256::digest(SKILL_CONTENT.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}
