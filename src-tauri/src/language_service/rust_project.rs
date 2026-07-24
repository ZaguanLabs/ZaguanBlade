use crate::symbol_index::store::{
    RustQualifiedCallRecord, RustQualifiedCallUpdate, SymbolStore, SymbolStoreError,
};
use crate::tree_sitter::{
    call_form, unresolved_reason, Language, Symbol, SymbolType, TreeSitterParser,
};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Bump whenever the derived Rust qualified-call projection changes without
/// requiring a new parser observation shape.
pub(super) const RUST_QUALIFIED_RESOLVER_VERSION: u32 = 1;

#[derive(Debug, Clone, Default)]
pub(super) struct RustQualifiedResolutionStats {
    pub observations: usize,
    pub resolved: usize,
    pub unresolved: usize,
    pub candidates_examined: usize,
    pub candidate_p50: usize,
    pub candidate_p95: usize,
    pub candidate_p99: usize,
    pub max_candidates: usize,
    pub estimated_comparisons_avoided: usize,
    pub duration_ms: u64,
    pub by_form: HashMap<String, usize>,
    pub by_strategy: HashMap<&'static str, usize>,
    pub by_unresolved_reason: HashMap<&'static str, usize>,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    manifest_path: PathBuf,
    targets: Vec<CargoTarget>,
    dependencies: Vec<CargoDependency>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoDependency {
    name: String,
    rename: Option<String>,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ProjectTarget {
    key: String,
    package_id: String,
    crate_name: String,
    root_file: String,
    dependency_packages: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ModuleLocation {
    target_key: String,
    module_path: Vec<String>,
}

#[derive(Debug, Clone)]
struct ImportBinding {
    path: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct BindingSlot {
    exact: Option<ImportBinding>,
    ambiguous: bool,
}

type ModuleBindings = HashMap<(String, Vec<String>), HashMap<String, BindingSlot>>;
type OwnerKey = (String, Vec<String>);

#[derive(Debug)]
struct RustProjectContext {
    targets: HashMap<String, ProjectTarget>,
    file_modules: HashMap<String, Vec<ModuleLocation>>,
    bindings: ModuleBindings,
    symbols: HashMap<String, Symbol>,
    owners: HashMap<OwnerKey, Vec<String>>,
    methods: HashMap<(String, String), Vec<String>>,
    source_impl_owner: HashMap<String, String>,
    trait_impl_sources: HashSet<String>,
    source_impl_trait: HashMap<String, String>,
    ufcs_methods: HashMap<(String, String, String), Vec<String>>,
}

#[derive(Debug)]
struct Resolution {
    target_id: Option<String>,
    strategy: Option<&'static str>,
    unresolved_reason: &'static str,
    candidates_examined: usize,
}

#[derive(Debug)]
struct ExternalModule {
    inline_prefix: Vec<String>,
    name: String,
    path_override: ModulePathOverride,
}

#[derive(Debug)]
enum ModulePathOverride {
    None,
    Literal(String),
    Unsupported,
}

#[derive(Debug)]
struct ParsedUse {
    inline_prefix: Vec<String>,
    bindings: Vec<(String, Vec<String>)>,
}

#[derive(Debug)]
struct ImplBlock {
    file_path: String,
    target_key: String,
    module_path: Vec<String>,
    symbol: Symbol,
    type_path: Vec<String>,
    trait_path: Option<Vec<String>>,
}

pub(super) fn resolve_qualified_calls(
    workspace_root: &Path,
    store: &SymbolStore,
) -> Result<RustQualifiedResolutionStats, SymbolStoreError> {
    let started = std::time::Instant::now();
    let records = store.rust_qualified_call_records()?;
    let mut stats = RustQualifiedResolutionStats {
        observations: records.len(),
        ..RustQualifiedResolutionStats::default()
    };
    if records.is_empty() {
        return Ok(stats);
    }

    let Some(context) = RustProjectContext::build(workspace_root, store)? else {
        let updates = records
            .iter()
            .map(|record| RustQualifiedCallUpdate {
                row_id: record.row_id,
                target_symbol_id: None,
                resolution_strategy: None,
                confidence: None,
                unresolved_reason: unresolved_reason::MISSING_PROJECT_CONTEXT,
            })
            .collect::<Vec<_>>();
        store.apply_rust_qualified_call_updates(&updates)?;
        stats.unresolved = records.len();
        stats
            .by_unresolved_reason
            .insert(unresolved_reason::MISSING_PROJECT_CONTEXT, records.len());
        stats.duration_ms = started.elapsed().as_millis() as u64;
        return Ok(stats);
    };

    let brute_force_comparisons = records.len().saturating_mul(
        context
            .symbols
            .values()
            .filter(|symbol| symbol.symbol_type == SymbolType::Method)
            .count(),
    );
    let mut candidate_counts = Vec::with_capacity(records.len());
    let updates = records
        .iter()
        .map(|record| {
            *stats.by_form.entry(record.call_form.clone()).or_default() += 1;
            let resolution = context.resolve(record);
            candidate_counts.push(resolution.candidates_examined);
            stats.candidates_examined += resolution.candidates_examined;
            stats.max_candidates = stats.max_candidates.max(resolution.candidates_examined);
            if let Some(strategy) = resolution.strategy {
                stats.resolved += 1;
                *stats.by_strategy.entry(strategy).or_default() += 1;
            } else {
                stats.unresolved += 1;
                *stats
                    .by_unresolved_reason
                    .entry(resolution.unresolved_reason)
                    .or_default() += 1;
            }
            RustQualifiedCallUpdate {
                row_id: record.row_id,
                target_symbol_id: resolution.target_id,
                resolution_strategy: resolution.strategy,
                confidence: resolution.strategy.map(|_| 1.0),
                unresolved_reason: resolution.unresolved_reason,
            }
        })
        .collect::<Vec<_>>();
    store.apply_rust_qualified_call_updates(&updates)?;
    candidate_counts.sort_unstable();
    stats.candidate_p50 = percentile(&candidate_counts, 50);
    stats.candidate_p95 = percentile(&candidate_counts, 95);
    stats.candidate_p99 = percentile(&candidate_counts, 99);
    stats.estimated_comparisons_avoided =
        brute_force_comparisons.saturating_sub(stats.candidates_examined);
    stats.duration_ms = started.elapsed().as_millis() as u64;
    Ok(stats)
}

impl RustProjectContext {
    fn build(workspace_root: &Path, store: &SymbolStore) -> Result<Option<Self>, SymbolStoreError> {
        let indexed_files = store.list_all_indexed_files()?;
        let rust_files = indexed_files
            .iter()
            .map(|record| record.file_path.clone())
            .filter(|path| path.to_ascii_lowercase().ends_with(".rs"))
            .collect::<HashSet<_>>();
        if rust_files.is_empty() {
            return Ok(None);
        }

        let manifests = indexed_files
            .iter()
            .map(|record| record.file_path.as_str())
            .filter(|path| {
                Path::new(path)
                    .file_name()
                    .is_some_and(|name| name == "Cargo.toml")
            })
            .map(|path| workspace_root.join(path))
            .collect::<Vec<_>>();
        let packages = cargo_packages(&manifests);
        if packages.is_empty() {
            return Ok(None);
        }

        let package_paths = packages
            .iter()
            .map(|package| {
                (
                    package
                        .manifest_path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_default(),
                    package.id.clone(),
                )
            })
            .collect::<Vec<_>>();
        let mut targets = HashMap::new();
        for package in &packages {
            let dependency_packages = package
                .dependencies
                .iter()
                .filter_map(|dependency| {
                    let dependency_path = dependency.path.as_ref()?;
                    let package_id = package_paths
                        .iter()
                        .find(|(path, _)| same_path(path, dependency_path))
                        .map(|(_, id)| id.clone())?;
                    let binding = dependency
                        .rename
                        .as_deref()
                        .unwrap_or(&dependency.name)
                        .replace('-', "_");
                    Some((binding, package_id))
                })
                .collect::<HashMap<_, _>>();
            for target in &package.targets {
                let Some(root_file) = relative_workspace_path(workspace_root, &target.src_path)
                else {
                    continue;
                };
                if !rust_files.contains(&root_file) {
                    continue;
                }
                let kind = target.kind.first().map(String::as_str).unwrap_or("unknown");
                let key = format!("{}::{}::{}", package.id, kind, target.name);
                targets.insert(
                    key.clone(),
                    ProjectTarget {
                        key,
                        package_id: package.id.clone(),
                        crate_name: target.name.replace('-', "_"),
                        root_file,
                        dependency_packages: dependency_packages.clone(),
                    },
                );
            }
        }
        if targets.is_empty() {
            return Ok(None);
        }

        let mut file_modules = HashMap::<String, Vec<ModuleLocation>>::new();
        for target in targets.values() {
            discover_target_modules(workspace_root, target, &rust_files, &mut file_modules);
        }

        let mut symbols = HashMap::new();
        for file_path in &rust_files {
            for symbol in store.get_symbols_in_file(file_path)? {
                symbols.insert(symbol.id.clone(), symbol);
            }
        }

        let bindings = build_import_bindings(workspace_root, &file_modules);
        let mut context = Self {
            targets,
            file_modules,
            bindings,
            symbols,
            owners: HashMap::new(),
            methods: HashMap::new(),
            source_impl_owner: HashMap::new(),
            trait_impl_sources: HashSet::new(),
            source_impl_trait: HashMap::new(),
            ufcs_methods: HashMap::new(),
        };
        context.build_owner_and_method_indexes();
        Ok(Some(context))
    }

    fn build_owner_and_method_indexes(&mut self) {
        for symbol in self.symbols.values() {
            if !is_owner_symbol(symbol.symbol_type) {
                continue;
            }
            for location in self.symbol_locations(symbol) {
                let mut path = location.module_path;
                path.extend(module_ancestors(symbol, &self.symbols));
                path.push(normalize_identifier(&symbol.name));
                self.owners
                    .entry((location.target_key, path))
                    .or_default()
                    .push(symbol.id.clone());
            }
        }

        let impl_blocks = self.impl_blocks();
        let mut impl_method_ids = HashSet::new();
        for block in impl_blocks {
            let owner_ids = self.resolve_owner_candidates(
                &block.target_key,
                &block.module_path,
                &block.type_path,
                0,
                &mut HashSet::new(),
            );
            if owner_ids.len() != 1 {
                continue;
            }
            let owner_id = owner_ids[0].clone();
            let trait_ids = block
                .trait_path
                .as_ref()
                .map(|path| {
                    self.resolve_owner_candidates(
                        &block.target_key,
                        &block.module_path,
                        path,
                        0,
                        &mut HashSet::new(),
                    )
                })
                .unwrap_or_default();
            let is_trait_impl = block.trait_path.is_some();
            let resolved_trait_id = match trait_ids.as_slice() {
                [trait_id] => Some(trait_id.clone()),
                _ => None,
            };
            for method in self.symbols.values().filter(|symbol| {
                symbol.file_path == block.file_path
                    && symbol.symbol_type == SymbolType::Method
                    && symbol.range.start.line >= block.symbol.range.start.line
                    && symbol.range.end.line <= block.symbol.range.end.line
            }) {
                impl_method_ids.insert(method.id.clone());
                self.source_impl_owner
                    .insert(method.id.clone(), owner_id.clone());
                if is_trait_impl {
                    self.trait_impl_sources.insert(method.id.clone());
                }
                if let Some(trait_id) = resolved_trait_id.as_ref() {
                    self.source_impl_trait
                        .insert(method.id.clone(), trait_id.clone());
                    self.ufcs_methods
                        .entry((
                            owner_id.clone(),
                            trait_id.clone(),
                            normalize_identifier(&method.name),
                        ))
                        .or_default()
                        .push(method.id.clone());
                } else if !is_trait_impl {
                    self.methods
                        .entry((owner_id.clone(), normalize_identifier(&method.name)))
                        .or_default()
                        .push(method.id.clone());
                }
            }
        }

        // Trait-impl methods need trait evidence and are intentionally absent
        // from this generic associated-call map. Methods not owned by an impl
        // block (for example trait declarations) retain their direct owner.
        for method in self.symbols.values().filter(|symbol| {
            symbol.symbol_type == SymbolType::Method && !impl_method_ids.contains(&symbol.id)
        }) {
            if let Some(parent_id) = method.parent_id.as_ref() {
                if self
                    .symbols
                    .get(parent_id)
                    .is_some_and(|parent| is_owner_symbol(parent.symbol_type))
                {
                    self.methods
                        .entry((parent_id.clone(), normalize_identifier(&method.name)))
                        .or_default()
                        .push(method.id.clone());
                }
            }
        }
    }

    fn resolve(&self, record: &RustQualifiedCallRecord) -> Resolution {
        let Some(source) = self.symbols.get(&record.source_symbol_id) else {
            return unresolved(unresolved_reason::UNRESOLVED_OWNER, 0);
        };
        let locations = self.source_locations(source);
        if locations.is_empty() {
            return unresolved(unresolved_reason::MISSING_PROJECT_CONTEXT, 0);
        }

        let mut candidates = Vec::new();
        let mut strategies = Vec::new();
        let mut examined = 0usize;
        let mut saw_owner = false;
        let mut saw_ambiguous_binding = false;
        for location in locations {
            match record.call_form.as_str() {
                call_form::SELF_PATH => {
                    let owner_id = self
                        .source_impl_owner
                        .get(&source.id)
                        .cloned()
                        .or_else(|| enclosing_owner_id(source, &self.symbols));
                    if let Some(owner_id) = owner_id {
                        saw_owner = true;
                        let methods = if self.trait_impl_sources.contains(&source.id) {
                            self.source_impl_trait
                                .get(&source.id)
                                .and_then(|trait_id| {
                                    self.ufcs_methods.get(&(
                                        owner_id,
                                        trait_id.clone(),
                                        record.target_name.clone(),
                                    ))
                                })
                                .cloned()
                                .unwrap_or_default()
                        } else {
                            self.methods
                                .get(&(owner_id, record.target_name.clone()))
                                .cloned()
                                .unwrap_or_default()
                        };
                        examined += methods.len();
                        candidates.extend(methods);
                        strategies.push("rust_self_owner");
                    }
                }
                call_form::UFCS => {
                    if record.qualifier_segments.len() != 2 {
                        continue;
                    }
                    saw_ambiguous_binding |= record.qualifier_segments.iter().any(|segment| {
                        self.binding_is_ambiguous(
                            &location.target_key,
                            &location.module_path,
                            segment,
                        )
                    });
                    let type_ids = self.resolve_owner_candidates(
                        &location.target_key,
                        &location.module_path,
                        &record.qualifier_segments[..1],
                        0,
                        &mut HashSet::new(),
                    );
                    let trait_ids = self.resolve_owner_candidates(
                        &location.target_key,
                        &location.module_path,
                        &record.qualifier_segments[1..],
                        0,
                        &mut HashSet::new(),
                    );
                    saw_owner |= !type_ids.is_empty() && !trait_ids.is_empty();
                    for type_id in &type_ids {
                        for trait_id in &trait_ids {
                            let methods = self
                                .ufcs_methods
                                .get(&(
                                    type_id.clone(),
                                    trait_id.clone(),
                                    record.target_name.clone(),
                                ))
                                .cloned()
                                .unwrap_or_default();
                            examined += methods.len();
                            candidates.extend(methods);
                        }
                    }
                    strategies.push("rust_ufcs");
                }
                call_form::ASSOCIATED | call_form::CRATE_PATH | call_form::MODULE_PATH => {
                    if let Some(first) = record.qualifier_segments.first() {
                        saw_ambiguous_binding |= self.binding_is_ambiguous(
                            &location.target_key,
                            &location.module_path,
                            first,
                        );
                    }
                    let owners = self.resolve_owner_candidates(
                        &location.target_key,
                        &location.module_path,
                        &record.qualifier_segments,
                        0,
                        &mut HashSet::new(),
                    );
                    saw_owner |= !owners.is_empty();
                    for owner_id in owners {
                        let methods = self
                            .methods
                            .get(&(owner_id, record.target_name.clone()))
                            .cloned()
                            .unwrap_or_default();
                        examined += methods.len();
                        candidates.extend(methods);
                    }
                    strategies.push(match record.call_form.as_str() {
                        call_form::CRATE_PATH => "rust_crate_path",
                        call_form::MODULE_PATH => "rust_module_path",
                        _ if self.is_explicit_binding(
                            &location.target_key,
                            &location.module_path,
                            record.qualifier_segments.first().map(String::as_str),
                        ) =>
                        {
                            "rust_use_binding"
                        }
                        _ => "rust_visible_owner",
                    });
                }
                _ => {}
            }
        }

        candidates.sort();
        candidates.dedup();
        strategies.sort_unstable();
        strategies.dedup();
        match candidates.as_slice() {
            [target_id] if strategies.len() == 1 => Resolution {
                target_id: Some(target_id.clone()),
                strategy: strategies.first().copied(),
                unresolved_reason: unresolved_reason::UNRESOLVED_OWNER,
                candidates_examined: examined,
            },
            [] if record.call_form == call_form::UFCS && record.qualifier_segments.len() != 2 => {
                unresolved(unresolved_reason::UNSUPPORTED, examined)
            }
            [] if record.call_form == call_form::SELF_PATH && !saw_owner => {
                unresolved(unresolved_reason::SELF_WITHOUT_OWNER, examined)
            }
            [] if saw_ambiguous_binding => unresolved(unresolved_reason::AMBIGUOUS, examined),
            [] if saw_owner => unresolved(unresolved_reason::UNRESOLVED_METHOD, examined),
            [] => unresolved(unresolved_reason::UNRESOLVED_OWNER, examined),
            _ => unresolved(unresolved_reason::AMBIGUOUS, examined),
        }
    }

    fn source_locations(&self, source: &Symbol) -> Vec<ModuleLocation> {
        self.symbol_locations(source)
            .into_iter()
            .map(|mut location| {
                location
                    .module_path
                    .extend(module_ancestors(source, &self.symbols));
                location
            })
            .collect()
    }

    fn symbol_locations(&self, symbol: &Symbol) -> Vec<ModuleLocation> {
        self.file_modules
            .get(&symbol.file_path)
            .cloned()
            .unwrap_or_default()
    }

    fn is_explicit_binding(
        &self,
        target_key: &str,
        module_path: &[String],
        local: Option<&str>,
    ) -> bool {
        let Some(local) = local else {
            return false;
        };
        self.bindings
            .get(&(target_key.to_string(), module_path.to_vec()))
            .and_then(|bindings| bindings.get(local))
            .is_some_and(|slot| slot.exact.is_some() && !slot.ambiguous)
    }

    fn binding_is_ambiguous(&self, target_key: &str, module_path: &[String], local: &str) -> bool {
        self.bindings
            .get(&(target_key.to_string(), module_path.to_vec()))
            .and_then(|bindings| bindings.get(local))
            .is_some_and(|slot| slot.ambiguous)
    }

    fn resolve_owner_candidates(
        &self,
        target_key: &str,
        source_module: &[String],
        observed_path: &[String],
        depth: usize,
        visited: &mut HashSet<(String, Vec<String>)>,
    ) -> Vec<String> {
        if observed_path.is_empty() || depth > 32 {
            return Vec::new();
        }
        let Some(target) = self.targets.get(target_key) else {
            return Vec::new();
        };
        let mut path = observed_path.to_vec();
        let first = path.first().map(String::as_str).unwrap_or_default();

        if first == "crate" || first == target.crate_name {
            path.remove(0);
            return self.resolve_absolute_owner_path(target_key, &path, depth, visited);
        }
        if first == "self" {
            path.remove(0);
            let mut absolute = source_module.to_vec();
            absolute.extend(path);
            return self.resolve_absolute_owner_path(target_key, &absolute, depth, visited);
        }
        if first == "super" {
            let mut absolute = source_module.to_vec();
            while path.first().is_some_and(|segment| segment == "super") {
                path.remove(0);
                if absolute.pop().is_none() {
                    return Vec::new();
                }
            }
            absolute.extend(path);
            return self.resolve_absolute_owner_path(target_key, &absolute, depth, visited);
        }
        if let Some(package_id) = target.dependency_packages.get(first) {
            path.remove(0);
            return self
                .targets
                .values()
                .filter(|candidate| candidate.package_id == *package_id)
                .flat_map(|candidate| {
                    self.resolve_absolute_owner_path(&candidate.key, &path, depth + 1, visited)
                })
                .collect();
        }

        let visit_key = (target_key.to_string(), observed_path.to_vec());
        if !visited.insert(visit_key) {
            return Vec::new();
        }
        if let Some(slot) = self
            .bindings
            .get(&(target_key.to_string(), source_module.to_vec()))
            .and_then(|bindings| bindings.get(first))
        {
            if slot.ambiguous {
                return Vec::new();
            }
            if let Some(binding) = slot.exact.as_ref() {
                let mut expanded = binding.path.clone();
                expanded.extend(path.into_iter().skip(1));
                return self.resolve_owner_candidates(
                    target_key,
                    source_module,
                    &expanded,
                    depth + 1,
                    visited,
                );
            }
        }

        let mut module_relative = source_module.to_vec();
        module_relative.extend(path.clone());
        let mut candidates =
            self.resolve_absolute_owner_path(target_key, &module_relative, depth, visited);
        if source_module.is_empty() {
            return candidates;
        }
        candidates.extend(self.resolve_absolute_owner_path(target_key, &path, depth, visited));
        candidates.sort();
        candidates.dedup();
        candidates
    }

    fn resolve_absolute_owner_path(
        &self,
        target_key: &str,
        path: &[String],
        depth: usize,
        visited: &mut HashSet<(String, Vec<String>)>,
    ) -> Vec<String> {
        let direct = self.owner_ids(target_key, path);
        if !direct.is_empty() || path.is_empty() || depth > 32 {
            return direct;
        }
        let module_path = path[..path.len() - 1].to_vec();
        let local = &path[path.len() - 1];
        let Some(slot) = self
            .bindings
            .get(&(target_key.to_string(), module_path.clone()))
            .and_then(|bindings| bindings.get(local))
        else {
            return direct;
        };
        if slot.ambiguous {
            return Vec::new();
        }
        let Some(binding) = slot.exact.as_ref() else {
            return Vec::new();
        };
        self.resolve_owner_candidates(target_key, &module_path, &binding.path, depth + 1, visited)
    }

    fn owner_ids(&self, target_key: &str, path: &[String]) -> Vec<String> {
        self.owners
            .get(&(target_key.to_string(), path.to_vec()))
            .cloned()
            .unwrap_or_default()
    }

    fn impl_blocks(&self) -> Vec<ImplBlock> {
        let mut blocks = Vec::new();
        for symbol in self
            .symbols
            .values()
            .filter(|symbol| symbol.symbol_type == SymbolType::Impl)
        {
            let Some((trait_path, type_path)) = parse_impl_name(&symbol.name) else {
                continue;
            };
            for location in self.source_locations(symbol) {
                blocks.push(ImplBlock {
                    file_path: symbol.file_path.clone(),
                    target_key: location.target_key,
                    module_path: location.module_path,
                    symbol: symbol.clone(),
                    type_path: type_path.clone(),
                    trait_path: trait_path.clone(),
                });
            }
        }
        blocks
    }
}

fn unresolved(reason: &'static str, candidates_examined: usize) -> Resolution {
    Resolution {
        target_id: None,
        strategy: None,
        unresolved_reason: reason,
        candidates_examined,
    }
}

fn percentile(values: &[usize], percentile: usize) -> usize {
    if values.is_empty() {
        return 0;
    }
    let index = values.len().saturating_mul(percentile).saturating_add(99) / 100;
    values[index.saturating_sub(1).min(values.len() - 1)]
}

fn is_owner_symbol(symbol_type: SymbolType) -> bool {
    matches!(
        symbol_type,
        SymbolType::Struct
            | SymbolType::Class
            | SymbolType::Trait
            | SymbolType::Type
            | SymbolType::Enum
    )
}

fn enclosing_owner_id(source: &Symbol, symbols: &HashMap<String, Symbol>) -> Option<String> {
    let mut parent_id = source.parent_id.as_ref()?;
    loop {
        let parent = symbols.get(parent_id)?;
        if is_owner_symbol(parent.symbol_type) {
            return Some(parent.id.clone());
        }
        parent_id = parent.parent_id.as_ref()?;
    }
}

fn module_ancestors(symbol: &Symbol, symbols: &HashMap<String, Symbol>) -> Vec<String> {
    let mut names = Vec::new();
    let mut parent_id = symbol.parent_id.as_ref();
    while let Some(id) = parent_id {
        let Some(parent) = symbols.get(id) else {
            break;
        };
        if parent.symbol_type == SymbolType::Module && parent.qualified_name != "__file__" {
            names.push(normalize_identifier(&parent.name));
        }
        parent_id = parent.parent_id.as_ref();
    }
    names.reverse();
    names
}

fn cargo_packages(manifests: &[PathBuf]) -> Vec<CargoPackage> {
    let mut packages = HashMap::<String, CargoPackage>::new();
    let mut manifests = manifests.to_vec();
    manifests.sort_by_key(|path| path.components().count());
    let mut covered_manifests = HashSet::new();
    for manifest in &manifests {
        let manifest_key = std::fs::canonicalize(manifest).unwrap_or_else(|_| manifest.clone());
        if covered_manifests.contains(&manifest_key) {
            continue;
        }
        let output = cargo_metadata_output(manifest);
        let Ok(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let Ok(metadata) = serde_json::from_slice::<CargoMetadata>(&output.stdout) else {
            continue;
        };
        for package in &metadata.packages {
            covered_manifests.insert(
                std::fs::canonicalize(&package.manifest_path)
                    .unwrap_or_else(|_| package.manifest_path.clone()),
            );
        }
        let members = metadata
            .workspace_members
            .into_iter()
            .collect::<HashSet<_>>();
        for package in metadata
            .packages
            .into_iter()
            .filter(|package| members.contains(&package.id))
        {
            packages.entry(package.id.clone()).or_insert(package);
        }
    }
    packages.into_values().collect()
}

/// Cargo metadata is an intentional blocking probe in the service's existing
/// synchronous indexing/reconciliation phase. It never runs on an async
/// runtime task.
#[allow(clippy::disallowed_methods, clippy::disallowed_types)]
fn cargo_metadata_output(manifest: &Path) -> std::io::Result<std::process::Output> {
    std::process::Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
}

fn discover_target_modules(
    workspace_root: &Path,
    target: &ProjectTarget,
    rust_files: &HashSet<String>,
    file_modules: &mut HashMap<String, Vec<ModuleLocation>>,
) {
    let mut pending = vec![(target.root_file.clone(), Vec::<String>::new())];
    let mut visited = HashSet::new();
    while let Some((file_path, module_path)) = pending.pop() {
        if !visited.insert((file_path.clone(), module_path.clone())) {
            continue;
        }
        file_modules
            .entry(file_path.clone())
            .or_default()
            .push(ModuleLocation {
                target_key: target.key.clone(),
                module_path: module_path.clone(),
            });
        let absolute = workspace_root.join(&file_path);
        let Ok(content) = std::fs::read_to_string(&absolute) else {
            continue;
        };
        for declaration in external_modules(&content) {
            if matches!(&declaration.path_override, ModulePathOverride::Unsupported) {
                continue;
            }
            let mut child_module = module_path.clone();
            child_module.extend(declaration.inline_prefix.iter().cloned());
            child_module.push(declaration.name.clone());
            let candidates = module_file_candidates(
                &absolute,
                &declaration.inline_prefix,
                &declaration.name,
                match &declaration.path_override {
                    ModulePathOverride::Literal(path) => Some(path.as_str()),
                    ModulePathOverride::None | ModulePathOverride::Unsupported => None,
                },
            )
            .into_iter()
            .filter_map(|path| relative_workspace_path(workspace_root, &path))
            .filter(|path| rust_files.contains(path))
            .collect::<Vec<_>>();
            if let [child_file] = candidates.as_slice() {
                pending.push((child_file.clone(), child_module));
            }
        }
    }
    for locations in file_modules.values_mut() {
        locations.sort_by(|left, right| {
            left.target_key
                .cmp(&right.target_key)
                .then_with(|| left.module_path.cmp(&right.module_path))
        });
        locations.dedup();
    }
}

fn external_modules(content: &str) -> Vec<ExternalModule> {
    let Ok(mut parser) = TreeSitterParser::new() else {
        return Vec::new();
    };
    let Ok(tree) = parser.parse(content, Language::Rust) else {
        return Vec::new();
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }
    let mut modules = Vec::new();
    collect_rust_nodes(
        tree.root_node(),
        content,
        &mut Vec::new(),
        &mut modules,
        &mut Vec::new(),
    );
    modules
}

fn build_import_bindings(
    workspace_root: &Path,
    file_modules: &HashMap<String, Vec<ModuleLocation>>,
) -> ModuleBindings {
    let mut bindings = ModuleBindings::new();
    for (file_path, locations) in file_modules {
        let Ok(content) = std::fs::read_to_string(workspace_root.join(file_path)) else {
            continue;
        };
        for parsed in parsed_uses(&content) {
            for location in locations {
                let mut module_path = location.module_path.clone();
                module_path.extend(parsed.inline_prefix.iter().cloned());
                let module_bindings = bindings
                    .entry((location.target_key.clone(), module_path))
                    .or_default();
                for (local, path) in &parsed.bindings {
                    let slot = module_bindings.entry(local.clone()).or_default();
                    if slot
                        .exact
                        .as_ref()
                        .is_some_and(|existing| existing.path != *path)
                    {
                        slot.ambiguous = true;
                        slot.exact = None;
                    } else if !slot.ambiguous {
                        slot.exact = Some(ImportBinding { path: path.clone() });
                    }
                }
            }
        }
    }
    bindings
}

fn parsed_uses(content: &str) -> Vec<ParsedUse> {
    let Ok(mut parser) = TreeSitterParser::new() else {
        return Vec::new();
    };
    let Ok(tree) = parser.parse(content, Language::Rust) else {
        return Vec::new();
    };
    if tree.root_node().has_error() {
        return Vec::new();
    }
    let mut uses = Vec::new();
    collect_rust_nodes(
        tree.root_node(),
        content,
        &mut Vec::new(),
        &mut Vec::new(),
        &mut uses,
    );
    uses
}

fn collect_rust_nodes(
    node: tree_sitter::Node<'_>,
    content: &str,
    inline_prefix: &mut Vec<String>,
    modules: &mut Vec<ExternalModule>,
    uses: &mut Vec<ParsedUse>,
) {
    if node.kind() == "mod_item" {
        let name = node
            .child_by_field_name("name")
            .and_then(|child| child.utf8_text(content.as_bytes()).ok())
            .map(normalize_identifier);
        let body = node.child_by_field_name("body");
        if let Some(name) = name {
            if let Some(body) = body {
                inline_prefix.push(name);
                let mut cursor = body.walk();
                for child in body.named_children(&mut cursor) {
                    collect_rust_nodes(child, content, inline_prefix, modules, uses);
                }
                inline_prefix.pop();
            } else {
                modules.push(ExternalModule {
                    inline_prefix: inline_prefix.clone(),
                    name,
                    path_override: module_path_override(node, content),
                });
            }
        }
        return;
    }
    if node.kind() == "use_declaration" {
        if let Ok(text) = node.utf8_text(content.as_bytes()) {
            let Some(use_start) = text.find("use ") else {
                return;
            };
            let tree = text[use_start + 4..].trim_end_matches(';').trim();
            let mut expanded = Vec::new();
            expand_use_tree(&[], tree, &mut expanded);
            uses.push(ParsedUse {
                inline_prefix: inline_prefix.clone(),
                bindings: expanded,
            });
        }
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        collect_rust_nodes(child, content, inline_prefix, modules, uses);
    }
}

fn module_path_override(node: tree_sitter::Node<'_>, content: &str) -> ModulePathOverride {
    let Some(sibling) = node.prev_named_sibling() else {
        return ModulePathOverride::None;
    };
    if sibling.kind() != "attribute_item" {
        return ModulePathOverride::None;
    }
    let Ok(text) = sibling.utf8_text(content.as_bytes()) else {
        return ModulePathOverride::Unsupported;
    };
    let Some(body) = text
        .trim()
        .strip_prefix("#[")
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
    else {
        return ModulePathOverride::None;
    };
    let Some(value) = body
        .strip_prefix("path")
        .map(str::trim)
        .and_then(|value| value.strip_prefix('='))
        .map(str::trim)
    else {
        return ModulePathOverride::None;
    };
    let Some(value) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return ModulePathOverride::Unsupported;
    };
    if value.is_empty() {
        ModulePathOverride::Unsupported
    } else {
        ModulePathOverride::Literal(value.to_string())
    }
}

fn module_file_candidates(
    declaring_file: &Path,
    inline_prefix: &[String],
    module_name: &str,
    literal_path: Option<&str>,
) -> Vec<PathBuf> {
    let parent = declaring_file.parent().unwrap_or_else(|| Path::new(""));
    if let Some(path) = literal_path {
        return vec![parent.join(path)];
    }
    let stem = declaring_file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let mut base = if matches!(stem, "lib" | "main" | "mod" | "build") {
        parent.to_path_buf()
    } else {
        parent.join(stem)
    };
    for segment in inline_prefix {
        base.push(segment);
    }
    vec![
        base.join(format!("{module_name}.rs")),
        base.join(module_name).join("mod.rs"),
    ]
}

fn expand_use_tree(prefix: &[String], tree: &str, out: &mut Vec<(String, Vec<String>)>) {
    let tree = tree.trim();
    if tree.is_empty() || tree == "*" {
        return;
    }
    if let Some((head, body)) = top_level_group(tree) {
        let mut base = prefix.to_vec();
        base.extend(path_segments(head.trim_end_matches("::")));
        for entry in split_top_level(body) {
            expand_use_tree(&base, entry, out);
        }
        return;
    }

    let (path_text, alias) = split_alias(tree);
    let mut path = prefix.to_vec();
    let mut suffix = path_segments(path_text);
    if suffix.last().is_some_and(|segment| segment == "*") {
        return;
    }
    if suffix.last().is_some_and(|segment| segment == "self") {
        suffix.pop();
        path.extend(suffix);
        if let Some(local) = alias
            .map(normalize_identifier)
            .or_else(|| path.last().cloned())
        {
            out.push((local, path));
        }
        return;
    }
    path.extend(suffix);
    let Some(local) = alias
        .map(normalize_identifier)
        .or_else(|| path.last().cloned())
    else {
        return;
    };
    out.push((local, path));
}

fn top_level_group(tree: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut start = None;
    for (index, ch) in tree.char_indices() {
        match ch {
            '{' if depth == 0 => {
                start = Some(index);
                depth = 1;
            }
            '{' => depth += 1,
            '}' if depth == 1 => {
                let start = start?;
                if tree[index + ch.len_utf8()..].trim().is_empty() {
                    return Some((&tree[..start], &tree[start + 1..index]));
                }
                return None;
            }
            '}' if depth > 1 => depth -= 1,
            _ => {}
        }
    }
    None
}

fn split_top_level(value: &str) -> Vec<&str> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut parts = Vec::new();
    for (index, ch) in value.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    parts.push(value[start..].trim());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

fn split_alias(value: &str) -> (&str, Option<&str>) {
    value
        .rsplit_once(" as ")
        .map(|(path, alias)| (path.trim(), Some(alias.trim())))
        .unwrap_or((value.trim(), None))
}

fn path_segments(path: &str) -> Vec<String> {
    path.split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(normalize_identifier)
        .collect()
}

fn normalize_identifier(identifier: &str) -> String {
    identifier
        .trim()
        .strip_prefix("r#")
        .unwrap_or(identifier.trim())
        .to_string()
}

fn parse_impl_name(name: &str) -> Option<(Option<Vec<String>>, Vec<String>)> {
    let body = name.strip_prefix("impl ")?.trim();
    let body = strip_generics(body);
    if let Some((trait_path, type_path)) = body.rsplit_once(" for ") {
        return Some((
            Some(path_segments(trait_path.trim())),
            path_segments(type_path.trim()),
        ));
    }
    Some((None, path_segments(body.trim())))
}

fn strip_generics(value: &str) -> String {
    let mut depth = 0usize;
    value
        .chars()
        .filter(|ch| match ch {
            '<' => {
                depth += 1;
                false
            }
            '>' => {
                depth = depth.saturating_sub(1);
                false
            }
            _ => depth == 0,
        })
        .collect()
}

fn relative_workspace_path(workspace_root: &Path, path: &Path) -> Option<String> {
    let absolute = std::fs::canonicalize(path).ok()?;
    let root = std::fs::canonicalize(workspace_root).ok()?;
    let relative = absolute.strip_prefix(root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language_service::LanguageService;
    use crate::symbol_index::SymbolStore;
    use crate::tree_sitter::SymbolRelationshipType;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn grouped_use_expansion_preserves_aliases_and_self_entries() {
        let mut bindings = Vec::new();
        expand_use_tree(
            &[],
            "crate::store::{self, Store as PublicStore, nested::{Thing, Other as Renamed}}",
            &mut bindings,
        );
        assert!(bindings.contains(&(
            "store".to_string(),
            vec!["crate".to_string(), "store".to_string()]
        )));
        assert!(bindings.contains(&(
            "PublicStore".to_string(),
            vec![
                "crate".to_string(),
                "store".to_string(),
                "Store".to_string()
            ]
        )));
        assert!(bindings.contains(&(
            "Renamed".to_string(),
            vec![
                "crate".to_string(),
                "store".to_string(),
                "nested".to_string(),
                "Other".to_string()
            ]
        )));
    }

    #[test]
    fn glob_use_does_not_create_exact_binding() {
        let mut bindings = Vec::new();
        expand_use_tree(&[], "crate::store::*", &mut bindings);
        assert!(bindings.is_empty());
    }

    #[test]
    fn cargo_workspace_resolves_aliases_cross_file_impls_ufcs_and_same_line_callers() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/rust-qualified-workspace");
        let database_dir = TempDir::new().expect("temporary symbol database");
        let store = Arc::new(
            SymbolStore::new(&database_dir.path().join("symbols.db"))
                .expect("create symbol database"),
        );
        let service = LanguageService::new(fixture.clone(), Arc::clone(&store))
            .expect("create language service");

        service
            .index_directory("")
            .expect("index qualified Rust workspace");

        let impl_symbols = service
            .get_file_symbols("store-core/src/impls.rs")
            .expect("impl symbols");
        let new_method = impl_symbols
            .iter()
            .find(|symbol| {
                symbol.symbol_type == SymbolType::Method && symbol.qualified_name == "Store::new"
            })
            .expect("Store::new method");
        let make_method = impl_symbols
            .iter()
            .find(|symbol| symbol.symbol_type == SymbolType::Method && symbol.name == "make")
            .expect("Maker::make implementation");

        let new_callers = service
            .find_references_to_symbol(new_method, 20)
            .expect("incoming Store::new callers");
        let create_both_calls = new_callers
            .iter()
            .filter(|reference| reference.source_symbol.name == "create_both")
            .collect::<Vec<_>>();
        assert_eq!(
            create_both_calls.len(),
            2,
            "both same-line qualified calls must survive incoming-reference dedup"
        );
        assert_ne!(
            create_both_calls[0].byte_offset,
            create_both_calls[1].byte_offset
        );
        assert!(create_both_calls.iter().all(|reference| {
            matches!(
                reference.resolution_strategy.as_deref(),
                Some("rust_use_binding") | Some("rust_crate_path") | Some("rust_visible_owner")
            ) && reference.resolution_confidence == Some(1.0)
        }));

        let make_callers = service
            .find_references_to_symbol(make_method, 20)
            .expect("incoming UFCS callers");
        assert!(make_callers.iter().any(|reference| {
            reference.source_symbol.name == "create_with_ufcs"
                && reference.resolution_strategy.as_deref() == Some("rust_ufcs")
                && reference.resolution_confidence == Some(1.0)
        }));
        for (qualified_name, caller_name) in [
            ("Generic::new", "create_with_turbofish"),
            ("Raw::r#build", "create_with_raw_identifiers"),
        ] {
            let method = impl_symbols
                .iter()
                .find(|symbol| {
                    symbol.symbol_type == SymbolType::Method
                        && symbol.qualified_name == qualified_name
                })
                .expect("qualified fixture method");
            let references = service
                .find_references_to_symbol(method, 20)
                .expect("qualified fixture callers");
            assert!(
                references
                    .iter()
                    .any(|reference| reference.source_symbol.name == caller_name),
                "{qualified_name} must have caller {caller_name}"
            );
        }
        for expected_file in [
            "app/build.rs",
            "app/tests/integration.rs",
            "app/examples/demo.rs",
            "app/benches/index.rs",
        ] {
            assert!(
                new_callers
                    .iter()
                    .any(|reference| reference.source_symbol.file_path == expected_file),
                "{expected_file} target must participate in the Cargo project graph"
            );
        }

        let app_symbols = service
            .get_file_symbols("app/src/main.rs")
            .expect("app symbols");
        let unknown = app_symbols
            .iter()
            .find(|symbol| symbol.name == "create_unknown")
            .expect("unknown caller");
        let graph = service
            .get_symbol_graph(unknown, SymbolRelationshipType::Call, 20)
            .expect("unknown caller graph");
        let unknown_call = graph
            .outgoing
            .iter()
            .find(|edge| edge.target_name == "new")
            .expect("Unknown::new observation");
        assert!(unknown_call.target_symbol_id.is_none());
        assert_ne!(
            unknown_call.resolution_strategy.as_deref(),
            Some("global_unique"),
            "a failed qualifier must never fall back to global terminal-name resolution"
        );

        let ambiguity_module = app_symbols
            .iter()
            .find(|symbol| symbol.symbol_type == SymbolType::Module && symbol.name == "ambiguity")
            .expect("ambiguity module");
        let ambiguous_caller = app_symbols
            .iter()
            .find(|symbol| {
                symbol.name == "create"
                    && symbol.parent_id.as_deref() == Some(ambiguity_module.id.as_str())
            })
            .expect("ambiguous binding caller");
        let ambiguous_graph = service
            .get_symbol_graph(ambiguous_caller, SymbolRelationshipType::Call, 20)
            .expect("ambiguous caller graph");
        let ambiguous_call = ambiguous_graph
            .outgoing
            .iter()
            .find(|edge| edge.target_name == "new")
            .expect("ambiguous Choice::new observation");
        assert!(ambiguous_call.target_symbol_id.is_none());
        assert_eq!(
            ambiguous_call.unresolved_reason.as_deref(),
            Some(unresolved_reason::AMBIGUOUS)
        );

        let glob_module = app_symbols
            .iter()
            .find(|symbol| symbol.symbol_type == SymbolType::Module && symbol.name == "glob_only")
            .expect("glob-only module");
        let glob_caller = app_symbols
            .iter()
            .find(|symbol| {
                symbol.name == "create"
                    && symbol.parent_id.as_deref() == Some(glob_module.id.as_str())
            })
            .expect("glob-only caller");
        let glob_graph = service
            .get_symbol_graph(glob_caller, SymbolRelationshipType::Call, 20)
            .expect("glob-only caller graph");
        let glob_call = glob_graph
            .outgoing
            .iter()
            .find(|edge| edge.target_name == "new")
            .expect("glob-only PublicStore::new observation");
        assert!(glob_call.target_symbol_id.is_none());

        for caller_name in ["create_missing_dependency", "create_reexport_cycle"] {
            let caller = app_symbols
                .iter()
                .find(|symbol| symbol.name == caller_name)
                .expect("negative fixture caller");
            let graph = service
                .get_symbol_graph(caller, SymbolRelationshipType::Call, 20)
                .expect("negative fixture graph");
            let call = graph
                .outgoing
                .iter()
                .find(|edge| edge.target_name == "new")
                .expect("negative qualified observation");
            assert!(
                call.target_symbol_id.is_none(),
                "{caller_name} must remain unresolved"
            );
        }
    }

    #[test]
    fn cargo_manifest_rename_incrementally_invalidates_qualified_targets() {
        let workspace = TempDir::new().expect("temporary Cargo workspace");
        let root = workspace.path();
        std::fs::create_dir_all(root.join("core/src")).unwrap();
        std::fs::create_dir_all(root.join("app/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"core\"]\nresolver = \"2\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("core/Cargo.toml"),
            "[package]\nname = \"qualified-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("core/src/lib.rs"),
            "pub struct Store;\nimpl Store { pub fn new() -> Self { Self } }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("app/Cargo.toml"),
            "[package]\nname = \"qualified-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nold_alias = { package = \"qualified-core\", path = \"../core\" }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("app/src/main.rs"),
            "fn create() { old_alias::Store::new(); }\nfn main() { create(); }\n",
        )
        .unwrap();

        let database_dir = TempDir::new().expect("temporary symbol database");
        let store = Arc::new(
            SymbolStore::new(&database_dir.path().join("symbols.db"))
                .expect("create symbol database"),
        );
        let service =
            LanguageService::new(root.to_path_buf(), store).expect("create language service");
        service.index_directory("").expect("index Cargo workspace");

        let store_method = service
            .get_file_symbols("core/src/lib.rs")
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.symbol_type == SymbolType::Method && symbol.name == "new")
            .expect("Store::new");
        assert!(service
            .find_references_to_symbol(&store_method, 10)
            .unwrap()
            .iter()
            .any(|reference| reference.source_symbol.name == "create"));

        std::fs::write(
            root.join("app/Cargo.toml"),
            "[package]\nname = \"qualified-app\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nnew_alias = { package = \"qualified-core\", path = \"../core\" }\n",
        )
        .unwrap();
        service
            .index_file("app/Cargo.toml")
            .expect("reindex changed Cargo manifest");

        let caller = service
            .get_file_symbols("app/src/main.rs")
            .unwrap()
            .into_iter()
            .find(|symbol| symbol.name == "create")
            .expect("create caller");
        let call = service
            .get_symbol_graph(&caller, SymbolRelationshipType::Call, 10)
            .unwrap()
            .outgoing
            .into_iter()
            .find(|edge| edge.target_name == "new")
            .expect("old_alias::Store::new observation");
        assert!(
            call.target_symbol_id.is_none(),
            "removing a Cargo dependency binding must clear its derived target"
        );
        assert_eq!(
            call.unresolved_reason.as_deref(),
            Some(unresolved_reason::UNRESOLVED_OWNER)
        );
    }
}
