use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use walkdir::{DirEntry, WalkDir};

use super::frontmatter::read_catalog_metadata;
use super::model::{
    SkillCatalog, SkillCatalogEntry, SkillDiagnosticsReport, SkillDiscoveryDiagnostic, SkillSource,
};
use super::snapshot::stable_host_skill_id;

const AGENTS_SKILLS_DIR: &str = ".agents/skills";
const SKILL_FILE_NAME: &str = "SKILL.md";
const MAX_SCAN_DEPTH: usize = 6;
const MAX_DIRECTORIES_PER_ROOT: usize = 2_000;
const MAX_ENTRIES_PER_ROOT: usize = 20_000;
const MAX_SKILLS_PER_SNAPSHOT: usize = 1_000;
const MAX_DIAGNOSTICS: usize = 2_000;
const DISCOVERY_CACHE_TTL: Duration = Duration::from_secs(2);
const MAX_CACHED_WORKSPACES: usize = 32;

#[derive(Clone)]
struct CachedCatalog {
    refreshed_at: Instant,
    catalog: SkillCatalog,
}

static DISCOVERY_CACHE: OnceLock<Mutex<HashMap<Option<PathBuf>, CachedCatalog>>> = OnceLock::new();

#[derive(Debug)]
pub(crate) struct DiscoveryRoots {
    pub workspace: Option<PathBuf>,
    pub user: Option<PathBuf>,
    pub legacy_user: Option<PathBuf>,
}

impl DiscoveryRoots {
    fn standard(workspace_root: Option<&Path>) -> Self {
        Self {
            workspace: workspace_root.map(|root| root.join(AGENTS_SKILLS_DIR)),
            user: crate::config::standard_user_skills_dir(),
            legacy_user: Some(crate::config::global_skills_dir()),
        }
    }
}

pub fn discover_workspace_skills(workspace_root: &Path) -> Vec<SkillCatalogEntry> {
    discover_catalog_with_roots(DiscoveryRoots {
        workspace: Some(workspace_root.join(AGENTS_SKILLS_DIR)),
        user: None,
        legacy_user: None,
    })
    .skills
}

pub fn discover_available_skills(workspace_root: &Path) -> Vec<SkillCatalogEntry> {
    discover_skill_catalog(Some(workspace_root)).skills
}

pub fn discover_skill_catalog(workspace_root: Option<&Path>) -> SkillCatalog {
    let key = workspace_root.map(|root| {
        fs::canonicalize(root)
            .unwrap_or_else(|_| root.to_path_buf())
            .to_path_buf()
    });
    let cache = DISCOVERY_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let cache = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.get(&key) {
            if cached.refreshed_at.elapsed() < DISCOVERY_CACHE_TTL {
                return cached.catalog.clone();
            }
        }
    }

    let started_at = Instant::now();
    let mut catalog = discover_catalog_with_roots(DiscoveryRoots::standard(workspace_root));
    apply_skill_config(workspace_root, &mut catalog);
    eprintln!(
        "[SKILLS][discovery] skill_count={} disabled_count={} diagnostic_count={} truncated={} elapsed_ms={}",
        catalog.skills.len(),
        catalog.disabled_count,
        catalog.diagnostics.len(),
        catalog.truncated,
        started_at.elapsed().as_millis()
    );

    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.len() >= MAX_CACHED_WORKSPACES && !cache.contains_key(&key) {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, cached)| cached.refreshed_at)
            .map(|(key, _)| key.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(
        key,
        CachedCatalog {
            refreshed_at: Instant::now(),
            catalog: catalog.clone(),
        },
    );
    catalog
}

pub fn invalidate_skill_cache(workspace_root: Option<&Path>) {
    let Some(cache) = DISCOVERY_CACHE.get() else {
        return;
    };
    let mut cache = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(workspace_root) = workspace_root {
        let key = fs::canonicalize(workspace_root)
            .unwrap_or_else(|_| workspace_root.to_path_buf())
            .to_path_buf();
        cache.remove(&Some(key));
    } else {
        cache.clear();
    }
}

pub fn skill_diagnostics_report(workspace_root: &Path) -> SkillDiagnosticsReport {
    let catalog = discover_skill_catalog(Some(workspace_root));
    SkillDiagnosticsReport {
        skill_count: catalog.skills.len(),
        disabled_count: catalog.disabled_count,
        truncated: catalog.truncated,
        diagnostics: catalog.diagnostics,
    }
}

pub fn authorized_skill_directories(workspace_root: &Path) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = discover_skill_catalog(Some(workspace_root))
        .skills
        .into_iter()
        .filter_map(|skill| skill.canonical_path.parent().map(Path::to_path_buf))
        .collect();
    directories.sort();
    directories.dedup();
    directories
}

pub fn is_path_in_authorized_skill_directory(workspace_root: &Path, path: &Path) -> bool {
    authorized_skill_directories(workspace_root)
        .iter()
        .any(|directory| path.starts_with(directory))
}

pub(crate) fn discover_available_skills_with_global_root(
    workspace_root: &Path,
    global_skills_root: &Path,
) -> Vec<SkillCatalogEntry> {
    discover_catalog_with_roots(DiscoveryRoots {
        workspace: Some(workspace_root.join(AGENTS_SKILLS_DIR)),
        user: None,
        legacy_user: Some(global_skills_root.to_path_buf()),
    })
    .skills
}

pub(crate) fn discover_global_skills_from_root(skills_root: &Path) -> Vec<SkillCatalogEntry> {
    discover_catalog_with_roots(DiscoveryRoots {
        workspace: None,
        user: None,
        legacy_user: Some(skills_root.to_path_buf()),
    })
    .skills
}

pub(crate) fn discover_catalog_with_roots(roots: DiscoveryRoots) -> SkillCatalog {
    let mut catalog = SkillCatalog::default();
    let mut canonical_paths = HashSet::new();

    if let Some(root) = roots.workspace {
        discover_root(
            &root,
            SkillSource::Workspace,
            true,
            &mut canonical_paths,
            &mut catalog,
        );
    }
    if let Some(root) = roots.user {
        discover_root(
            &root,
            SkillSource::User,
            true,
            &mut canonical_paths,
            &mut catalog,
        );
    }
    if let Some(root) = roots.legacy_user {
        discover_root(
            &root,
            SkillSource::Global,
            false,
            &mut canonical_paths,
            &mut catalog,
        );
    }

    catalog
        .skills
        .sort_by(|left, right| left.skill_id.cmp(&right.skill_id));
    catalog
}

fn apply_skill_config(workspace_root: Option<&Path>, catalog: &mut SkillCatalog) {
    let Some(workspace_root) = workspace_root else {
        return;
    };
    let settings = match crate::project_settings::load_project_settings(workspace_root) {
        Ok(settings) => settings,
        Err(error) if error == "Settings file does not exist" => return,
        Err(error) => {
            push_diagnostic(
                catalog,
                &crate::project_settings::get_settings_path(workspace_root),
                format!("skills configuration could not be loaded: {error}"),
            );
            return;
        }
    };
    if settings.skills.config.is_empty() {
        return;
    }

    let config_path = crate::project_settings::get_settings_path(workspace_root);
    let mut disabled_paths = HashSet::new();
    for rule in settings.skills.config {
        let matching_paths: Vec<PathBuf> = match (rule.path.as_deref(), rule.name.as_deref()) {
            (Some(raw_path), None) => {
                let selector = Path::new(raw_path.trim());
                if !selector.is_absolute() {
                    push_diagnostic(
                        catalog,
                        &config_path,
                        "skill path selectors must be absolute".to_string(),
                    );
                    continue;
                }
                let selector =
                    fs::canonicalize(selector).unwrap_or_else(|_| selector.to_path_buf());
                catalog
                    .skills
                    .iter()
                    .filter(|skill| skill.canonical_path == selector)
                    .map(|skill| skill.canonical_path.clone())
                    .collect()
            }
            (None, Some(name)) if !name.trim().is_empty() => catalog
                .skills
                .iter()
                .filter(|skill| skill.name.as_deref() == Some(name.trim()))
                .map(|skill| skill.canonical_path.clone())
                .collect(),
            (Some(_), Some(_)) => {
                push_diagnostic(
                    catalog,
                    &config_path,
                    "skill configuration entries must select by path or name, not both".to_string(),
                );
                continue;
            }
            _ => {
                push_diagnostic(
                    catalog,
                    &config_path,
                    "skill configuration entries require a non-empty path or name".to_string(),
                );
                continue;
            }
        };

        if matching_paths.is_empty() {
            push_diagnostic(
                catalog,
                &config_path,
                "skill configuration selector matched no discovered skills".to_string(),
            );
        }
        for path in matching_paths {
            if rule.enabled {
                disabled_paths.remove(&path);
            } else {
                disabled_paths.insert(path);
            }
        }
    }

    catalog.disabled_count = disabled_paths.len();
    catalog
        .skills
        .retain(|skill| !disabled_paths.contains(&skill.canonical_path));
}

fn discover_root(
    skills_root: &Path,
    source: SkillSource,
    follow_directory_links: bool,
    canonical_paths: &mut HashSet<PathBuf>,
    catalog: &mut SkillCatalog,
) {
    if !skills_root.is_dir() {
        return;
    }

    let mut directory_count = 0usize;
    let mut entry_count = 0usize;
    let walker = WalkDir::new(skills_root)
        .follow_links(follow_directory_links)
        .max_depth(MAX_SCAN_DEPTH)
        .into_iter()
        .filter_entry(should_descend);

    for result in walker {
        if entry_count >= MAX_ENTRIES_PER_ROOT {
            catalog.truncated = true;
            push_diagnostic(
                catalog,
                skills_root,
                format!("root exceeded {MAX_ENTRIES_PER_ROOT} filesystem entries"),
            );
            break;
        }
        entry_count += 1;

        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                push_diagnostic(catalog, skills_root, format!("traversal error: {error}"));
                continue;
            }
        };
        if entry.file_type().is_dir() {
            directory_count += 1;
            if directory_count > MAX_DIRECTORIES_PER_ROOT {
                catalog.truncated = true;
                push_diagnostic(
                    catalog,
                    skills_root,
                    format!("root exceeded {MAX_DIRECTORIES_PER_ROOT} directories"),
                );
                break;
            }
            continue;
        }
        if !is_skill_file(&entry) {
            continue;
        }
        if catalog.skills.len() >= MAX_SKILLS_PER_SNAPSHOT {
            catalog.truncated = true;
            break;
        }

        let discovered_path = entry.path();
        let canonical_path = match fs::canonicalize(discovered_path) {
            Ok(path) => path,
            Err(error) => {
                push_diagnostic(
                    catalog,
                    discovered_path,
                    format!("canonicalize skill path: {error}"),
                );
                continue;
            }
        };
        if !canonical_paths.insert(canonical_path.clone()) {
            continue;
        }

        match catalog_entry_from_path(skills_root, discovered_path, canonical_path, source) {
            Ok((skill, recovered_name)) => {
                if recovered_name {
                    push_diagnostic(
                        catalog,
                        discovered_path,
                        "missing name recovered from parent directory".to_string(),
                    );
                }
                catalog.skills.push(skill);
            }
            Err(error) => push_diagnostic(catalog, discovered_path, error),
        }
    }
}

fn catalog_entry_from_path(
    skills_root: &Path,
    discovered_path: &Path,
    canonical_path: PathBuf,
    source: SkillSource,
) -> Result<(SkillCatalogEntry, bool), String> {
    let directory_name = discovered_path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .ok_or_else(|| "skill directory name is not valid UTF-8".to_string())?;
    let parsed = read_catalog_metadata(discovered_path, directory_name)?;
    let metadata =
        fs::metadata(&canonical_path).map_err(|error| format!("read skill metadata: {error}"))?;
    let display_locator = display_locator(skills_root, discovered_path, source);
    let skill_id = stable_host_skill_id(source, &canonical_path);
    let recovered_name = parsed.recovered_name;

    Ok((
        SkillCatalogEntry {
            skill_id,
            source,
            name: Some(parsed.name),
            description: Some(parsed.description),
            triggers: parsed.triggers,
            short_description: parsed.short_description,
            path: display_locator,
            license: parsed.license,
            compatibility: parsed.compatibility,
            metadata: parsed.metadata,
            allowed_tools: parsed.allowed_tools,
            canonical_path,
            file_size: metadata.len(),
            modified: metadata.modified().ok(),
        },
        recovered_name,
    ))
}

fn display_locator(skills_root: &Path, path: &Path, source: SkillSource) -> String {
    let relative = path.strip_prefix(skills_root).unwrap_or(path);
    match source {
        SkillSource::Workspace => {
            normalize_path(Path::new(AGENTS_SKILLS_DIR).join(relative).as_path())
        }
        SkillSource::User => {
            let suffix = normalize_path(relative);
            if suffix.is_empty() {
                "~/.agents/skills".to_string()
            } else {
                format!("~/.agents/skills/{suffix}")
            }
        }
        SkillSource::Global => {
            let suffix = normalize_path(relative);
            if suffix.is_empty() {
                "legacy-skills".to_string()
            } else {
                format!("legacy-skills/{suffix}")
            }
        }
    }
}

fn should_descend(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return true;
    }
    entry
        .file_name()
        .to_str()
        .map(|name| !name.starts_with('.'))
        .unwrap_or(false)
}

fn is_skill_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file() && entry.file_name() == SKILL_FILE_NAME
}

fn push_diagnostic(catalog: &mut SkillCatalog, path: &Path, message: String) {
    if catalog.diagnostics.len() >= MAX_DIAGNOSTICS {
        return;
    }
    catalog.diagnostics.push(SkillDiscoveryDiagnostic {
        path: path.to_string_lossy().to_string(),
        message,
    });
}

fn normalize_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
