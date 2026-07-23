mod discovery;
mod frontmatter;
mod loading;
mod model;
mod search;
mod snapshot;

pub use discovery::{
    authorized_skill_directories, discover_available_skills, discover_skill_catalog,
    discover_workspace_skills, invalidate_skill_cache, is_path_in_authorized_skill_directory,
    skill_diagnostics_report,
};
pub use loading::{load_skill, load_skill_chunk, load_workspace_skill};
pub use model::{
    LoadedSkill, SkillCatalog, SkillCatalogEntry, SkillDiagnosticsReport, SkillDiscoveryDiagnostic,
    SkillSource,
};
pub use search::list_skills;
pub use snapshot::build_host_skills_snapshot;

pub const SKILL_DISCOVERY_PROMPT: &str = "When a specialized workflow could materially help, call `list_skills` with a focused query, then call `load_skill` only for the selected result. Do not list or load skills for ordinary tasks. Skills do not persist across turns.";

#[cfg(test)]
mod tests {
    use super::discovery::{
        discover_available_skills_with_global_root, discover_catalog_with_roots,
        discover_global_skills_from_root, DiscoveryRoots,
    };
    use super::loading::load_skill_with_global_root;
    use super::snapshot::snapshot_from_catalog;
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().expect("test path has parent")).unwrap();
        fs::write(path, content).unwrap();
    }

    fn skill(name: &str, description: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n{body}")
    }

    #[test]
    fn missing_skills_directory_returns_empty_catalog() {
        let dir = TempDir::new().unwrap();

        assert!(discover_workspace_skills(dir.path()).is_empty());
        assert!(discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(dir.path().join("missing")),
            user: None,
            legacy_user: None,
        })
        .skills
        .is_empty());
    }

    #[test]
    fn discovers_valid_frontmatter_and_ignores_unknown_fields() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            "---\nname: example\ndescription: >-\n  Do the thing\n  carefully.\nunknown-field: ignored\ntriggers:\n  - thing\nmetadata:\n  short-description: Short thing\n  vendor: zaguan\nallowed-tools: read_file run_command\n---\nUse this workflow.",
        );

        let skills = discover_workspace_skills(dir.path());

        assert_eq!(skills.len(), 1);
        assert!(skills[0].skill_id.starts_with("host:"));
        assert_eq!(skills[0].name.as_deref(), Some("example"));
        assert_eq!(
            skills[0].description.as_deref(),
            Some("Do the thing carefully.")
        );
        assert_eq!(skills[0].short_description.as_deref(), Some("Short thing"));
        assert_eq!(skills[0].triggers, vec!["thing"]);
        assert_eq!(
            skills[0].metadata.get("vendor").map(String::as_str),
            Some("zaguan")
        );
        assert_eq!(skills[0].allowed_tools, vec!["read_file", "run_command"]);
    }

    #[test]
    fn missing_name_is_recovered_with_a_diagnostic() {
        let workspace = TempDir::new().unwrap();
        write(
            &workspace.path().join("example/SKILL.md"),
            "---\ndescription: Helpful workflow\n---\nBody",
        );

        let catalog = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(workspace.path().to_path_buf()),
            user: None,
            legacy_user: None,
        });

        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].name.as_deref(), Some("example"));
        assert!(catalog
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("missing name recovered")));
    }

    #[test]
    fn invalid_sibling_does_not_hide_valid_skill() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join("valid/SKILL.md"),
            &skill("valid", "Valid workflow", "Body"),
        );
        write(
            &dir.path().join("invalid/SKILL.md"),
            "---\nname: wrong-name\ndescription: Invalid workflow\n---\nBody",
        );

        let catalog = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(dir.path().to_path_buf()),
            user: None,
            legacy_user: None,
        });

        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].name.as_deref(), Some("valid"));
        assert_eq!(catalog.diagnostics.len(), 1);
    }

    #[test]
    fn duplicate_names_in_distinct_scopes_are_preserved() {
        let workspace = TempDir::new().unwrap();
        let user = TempDir::new().unwrap();
        write(
            &workspace.path().join("shared/SKILL.md"),
            &skill("shared", "Repository workflow", "Repo body"),
        );
        write(
            &user.path().join("shared/SKILL.md"),
            &skill("shared", "User workflow", "User body"),
        );

        let catalog = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(workspace.path().to_path_buf()),
            user: Some(user.path().to_path_buf()),
            legacy_user: None,
        });

        assert_eq!(catalog.skills.len(), 2);
        assert_ne!(catalog.skills[0].skill_id, catalog.skills[1].skill_id);
    }

    #[test]
    fn body_only_edit_does_not_change_skill_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("example/SKILL.md");
        write(&path, &skill("example", "Helpful workflow", "First body"));
        let first = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(dir.path().to_path_buf()),
            user: None,
            legacy_user: None,
        });

        thread::sleep(Duration::from_millis(2));
        write(
            &path,
            &skill("example", "Helpful workflow", "A different body"),
        );
        let second = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(dir.path().to_path_buf()),
            user: None,
            legacy_user: None,
        });

        assert_eq!(first.skills[0].skill_id, second.skills[0].skill_id);
        assert_ne!(
            snapshot_from_catalog(&first).digest,
            snapshot_from_catalog(&second).digest
        );
    }

    #[test]
    fn load_skill_returns_body_and_base_dir() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            &skill("example", "Helpful workflow", "Read references/guide.md."),
        );
        let skill_id = discover_workspace_skills(dir.path())[0].skill_id.clone();

        let loaded = load_workspace_skill(dir.path(), &skill_id).unwrap();

        assert_eq!(
            loaded.base_dir,
            dir.path()
                .join(".agents/skills/example")
                .to_string_lossy()
                .to_string()
        );
        assert_eq!(loaded.instructions, "Read references/guide.md.");
        assert!(loaded.complete);
        assert_eq!(loaded.next_offset, None);
    }

    #[test]
    fn load_skill_paginates_large_utf8_instructions() {
        let dir = TempDir::new().unwrap();
        let body = format!("{}é-tail", "a".repeat(12 * 1024 - 1));
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            &skill("example", "Helpful workflow", &body),
        );
        let skill_id = discover_workspace_skills(dir.path())[0].skill_id.clone();

        let first = load_skill_chunk(dir.path(), &skill_id, 0).unwrap();
        assert!(!first.complete);
        assert_eq!(first.instructions.len(), 12 * 1024 - 1);
        let next_offset = first.next_offset.expect("next offset");
        let second = load_skill_chunk(dir.path(), &skill_id, next_offset).unwrap();

        assert_eq!(second.instructions, "é-tail");
        assert!(second.complete);
        assert_eq!(second.next_offset, None);
    }

    #[test]
    fn load_skill_rejects_files_over_forty_kibibytes() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            &skill("example", "Helpful workflow", &"x".repeat(40 * 1024)),
        );
        let skill_id = discover_workspace_skills(dir.path())[0].skill_id.clone();

        let error = load_skill(dir.path(), &skill_id).unwrap_err();

        assert!(error.contains("40960-byte limit"));
        assert!(error.contains("referenced files"));
    }

    #[test]
    fn discovers_global_skills_from_global_root() {
        let global = TempDir::new().unwrap();
        write(
            &global.path().join("shared/SKILL.md"),
            &skill("shared", "Shared workflow", "Use globally."),
        );

        let skills = discover_global_skills_from_root(global.path());

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source, SkillSource::Global);
        assert_eq!(skills[0].description.as_deref(), Some("Shared workflow"));
    }

    #[test]
    fn canonical_path_deduplication_keeps_first_scope() {
        let workspace = TempDir::new().unwrap();
        let shared = TempDir::new().unwrap();
        write(
            &shared.path().join("example/SKILL.md"),
            &skill("example", "Shared workflow", "Body"),
        );

        let catalog = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(shared.path().to_path_buf()),
            user: Some(shared.path().to_path_buf()),
            legacy_user: Some(workspace.path().to_path_buf()),
        });

        assert_eq!(catalog.skills.len(), 1);
        assert_eq!(catalog.skills[0].source, SkillSource::Workspace);
    }

    #[test]
    fn load_skill_can_load_legacy_global_skill() {
        let workspace = TempDir::new().unwrap();
        let global = TempDir::new().unwrap();
        write(
            &global.path().join("shared/SKILL.md"),
            &skill("shared", "Shared workflow", "Use globally."),
        );
        let skill_id = discover_global_skills_from_root(global.path())[0]
            .skill_id
            .clone();

        let loaded =
            load_skill_with_global_root(workspace.path(), global.path(), &skill_id).unwrap();

        assert_eq!(loaded.name, "shared");
        assert_eq!(loaded.instructions, "Use globally.");
        assert_eq!(
            loaded.base_dir,
            global.path().join("shared").to_string_lossy().to_string()
        );
    }

    #[test]
    fn prompt_uses_a_stable_token_bounded_discovery_instruction() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join(".agents/skills/example/SKILL.md"),
            &skill("example", "Helpful workflow", "Secret body instructions."),
        );

        assert!(SKILL_DISCOVERY_PROMPT.contains("list_skills"));
        assert!(SKILL_DISCOVERY_PROMPT.contains("load_skill"));
        assert!(!SKILL_DISCOVERY_PROMPT.contains("Helpful workflow"));
        assert!(SKILL_DISCOVERY_PROMPT.split_whitespace().count() <= 60);
    }

    #[test]
    fn skill_config_name_disable_can_be_overridden_by_path() {
        let workspace = TempDir::new().unwrap();
        let skill_path = workspace.path().join(".agents/skills/example/SKILL.md");
        write(
            &skill_path,
            &skill("example", "Helpful workflow", "Instructions."),
        );
        let mut settings = crate::project_settings::ProjectSettings::default();
        settings.skills.config = vec![
            crate::project_settings::SkillConfig {
                path: None,
                name: Some("example".to_string()),
                enabled: false,
            },
            crate::project_settings::SkillConfig {
                path: Some(
                    fs::canonicalize(&skill_path)
                        .unwrap()
                        .to_string_lossy()
                        .to_string(),
                ),
                name: None,
                enabled: true,
            },
        ];
        crate::project_settings::save_project_settings(workspace.path(), &settings).unwrap();
        invalidate_skill_cache(Some(workspace.path()));

        let catalog = discover_skill_catalog(Some(workspace.path()));

        assert!(catalog
            .skills
            .iter()
            .any(|entry| entry.name.as_deref() == Some("example")));
        assert_eq!(catalog.disabled_count, 0);
    }

    #[test]
    fn skill_config_disables_by_name_and_reports_invalid_rules() {
        let workspace = TempDir::new().unwrap();
        write(
            &workspace.path().join(".agents/skills/example/SKILL.md"),
            &skill("example", "Helpful workflow", "Instructions."),
        );
        let mut settings = crate::project_settings::ProjectSettings::default();
        settings.skills.config = vec![
            crate::project_settings::SkillConfig {
                path: None,
                name: Some("example".to_string()),
                enabled: false,
            },
            crate::project_settings::SkillConfig {
                path: Some("/tmp/example/SKILL.md".to_string()),
                name: Some("example".to_string()),
                enabled: false,
            },
        ];
        crate::project_settings::save_project_settings(workspace.path(), &settings).unwrap();
        invalidate_skill_cache(Some(workspace.path()));

        let report = skill_diagnostics_report(workspace.path());

        assert!(!discover_skill_catalog(Some(workspace.path()))
            .skills
            .iter()
            .any(|entry| entry.name.as_deref() == Some("example")));
        assert_eq!(report.disabled_count, 1);
        assert!(report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("not both")));
    }

    #[test]
    fn explicit_invalidation_refreshes_the_throttled_catalog_cache() {
        let workspace = TempDir::new().unwrap();
        write(
            &workspace.path().join(".agents/skills/first/SKILL.md"),
            &skill("first", "First workflow", "Instructions."),
        );
        invalidate_skill_cache(Some(workspace.path()));
        let first = discover_skill_catalog(Some(workspace.path()));
        assert!(first
            .skills
            .iter()
            .any(|entry| entry.name.as_deref() == Some("first")));

        write(
            &workspace.path().join(".agents/skills/second/SKILL.md"),
            &skill("second", "Second workflow", "Instructions."),
        );
        let cached = discover_skill_catalog(Some(workspace.path()));
        assert!(!cached
            .skills
            .iter()
            .any(|entry| entry.name.as_deref() == Some("second")));

        invalidate_skill_cache(Some(workspace.path()));
        let refreshed = discover_skill_catalog(Some(workspace.path()));
        assert!(refreshed
            .skills
            .iter()
            .any(|entry| entry.name.as_deref() == Some("second")));
    }

    #[test]
    fn injected_roots_include_repository_user_and_legacy_skills() {
        let workspace = TempDir::new().unwrap();
        let user = TempDir::new().unwrap();
        let legacy = TempDir::new().unwrap();
        write(
            &workspace.path().join("repo-skill/SKILL.md"),
            &skill("repo-skill", "Repository workflow", "Body"),
        );
        write(
            &user.path().join("user-skill/SKILL.md"),
            &skill("user-skill", "User workflow", "Body"),
        );
        write(
            &legacy.path().join("legacy-skill/SKILL.md"),
            &skill("legacy-skill", "Legacy workflow", "Body"),
        );

        let catalog = discover_catalog_with_roots(DiscoveryRoots {
            workspace: Some(workspace.path().to_path_buf()),
            user: Some(user.path().to_path_buf()),
            legacy_user: Some(legacy.path().to_path_buf()),
        });

        assert_eq!(catalog.skills.len(), 3);
        assert!(catalog
            .skills
            .iter()
            .any(|entry| entry.source == SkillSource::Workspace));
        assert!(catalog
            .skills
            .iter()
            .any(|entry| entry.source == SkillSource::User));
        assert!(catalog
            .skills
            .iter()
            .any(|entry| entry.source == SkillSource::Global));
    }

    #[test]
    fn compatibility_helper_still_combines_workspace_and_legacy_roots() {
        let workspace = TempDir::new().unwrap();
        let global = TempDir::new().unwrap();
        write(
            &workspace.path().join(".agents/skills/repo-skill/SKILL.md"),
            &skill("repo-skill", "Repository workflow", "Body"),
        );
        write(
            &global.path().join("global-skill/SKILL.md"),
            &skill("global-skill", "Global workflow", "Body"),
        );

        assert_eq!(
            discover_available_skills_with_global_root(workspace.path(), global.path()).len(),
            2
        );
    }

    #[test]
    fn standard_user_root_discovers_known_installed_skills_when_present() {
        let Some(user_root) = crate::config::standard_user_skills_dir() else {
            return;
        };
        let expected = [
            "find-skills",
            "next-best-practices",
            "rust-skills",
            "seo-audit",
            "vercel-composition-patterns",
            "vercel-react-best-practices",
        ];
        if !expected
            .iter()
            .all(|name| user_root.join(name).join("SKILL.md").is_file())
        {
            return;
        }

        let catalog = discover_catalog_with_roots(DiscoveryRoots {
            workspace: None,
            user: Some(user_root),
            legacy_user: None,
        });
        let names = catalog
            .skills
            .iter()
            .filter_map(|entry| entry.name.as_deref())
            .collect::<Vec<_>>();

        for expected_name in expected {
            assert!(
                names.contains(&expected_name),
                "missing installed skill {expected_name}; diagnostics: {:?}",
                catalog.diagnostics
            );
        }
    }
}
