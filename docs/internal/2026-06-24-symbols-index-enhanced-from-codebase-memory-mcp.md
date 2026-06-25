# Symbols Index Enhancement Plan Inspired by codebase-memory-mcp

**Date**: 2026-06-24  
**Status**: Active implementation tracker  
**Last updated**: 2026-06-25  
**Scope**: Zaguan Blade Symbols Index, language extraction, search quality, indexing performance, and model-facing code intelligence tools  
**Reference project**: `../inspiration/codebase-memory-mcp`

## Summary

The current Symbols Index is a strong foundation, but its language support and query surface are still narrow compared with broader code-intelligence tools. The present language set is mostly based on immediate project needs:

- TypeScript
- TSX
- Astro, projected through TSX
- JavaScript
- JSX
- Python
- Rust
- Go
- Markdown headings
- limited anchor-only JSON/YAML translation resources

That is not enough for a broad developer audience. It also creates concrete model failures: when a model tries `symbol_search` for CSS selectors, variables, or other non-supported files, the tool returns empty results even when the content exists. The failure is currently silent and easy for a model to misinterpret as "not found" instead of "not indexed by this tool."

The inspiration project shows three important directions worth adapting:

1. **Broad parser coverage**: many languages can be indexed structurally, even if the first pass only extracts definitions, imports, and obvious relationships.
2. **Fast, indexed query primitives**: BM25 FTS, camelCase/snake_case aware search, degree filters, pagination, graph traversal, and schema introspection make the index useful as a navigation layer.
3. **Multi-pass enrichment**: start with syntax, then enrich relationships, routes, config, tests, impact, similarity, and semantic anchors in later phases.

This document proposes a staged path that keeps our Rust/Tauri architecture, expands language support pragmatically, and ports the highest-value ideas without attempting to clone `codebase-memory-mcp` wholesale. The most important constraint is intentionality: broad language support should come from a smaller, clearer, faster indexing core, not from layering special cases onto an already complex path.

## Tracker Discipline

This document is now the source of truth for the Symbols Index refactor. After each successful milestone completion:

1. Update **Implementation Progress** with the completion date, exact scope, verification commands, and any known limitations.
2. Update the relevant milestone status.
3. Add newly discovered follow-up work only if it directly supports the plan goals.
4. Do not start the next milestone until the current milestone's acceptance criteria are either complete or explicitly deferred with a reason.

This is intended to prevent drift. New ideas from `codebase-memory-mcp` or later research should be triaged against the goals and non-goals below before implementation.

## Implementation Progress

### Completed

#### Milestone 0: Capability Truthfulness and First CSS Slice

**Completed**: 2026-06-25  
**Status**: Complete, pending broader fixture hardening in Milestone 1.

Completed scope:

- Added `AGENTS.md` to `.gitignore`.
- Centralized language capability metadata in `src-tauri/src/tree_sitter/parser.rs`.
- Added support-level metadata for full, partial, projection, scanner, markdown-heading, and anchor-only style support.
- Exposed language-support metadata in model-facing code-intelligence tool responses.
- Updated model-facing tool descriptions and `docs/TOOL_CALLS.md` so empty symbol results are not treated as trustworthy without language/index context.
- Updated built-in model guidance in `src-tauri/src/config.rs`.
- Added partial scanner support for CSS, SCSS, Sass, Less, HTML, Vue, Svelte, JSON, YAML, and TOML.
- Added CSS-style symbol types for selectors, custom properties, keyframes, at-rules, layers, and font-face data.
- Added CSS selector, id selector, custom property, keyframes, at-rule, layer, and font-face extraction.
- Added markup `class` and `id` scanner extraction for HTML/Vue/Svelte.
- Added config key-path extraction for JSON/YAML/TOML with bounded symbol counts.
- Added `Usage` relationships for CSS module/class usage and CSS custom property usage where resolution is conservative enough.
- Added FTS/BM25-backed symbol search with LIKE fallback.
- Added `symbol_search` pagination metadata using offset/limit/has-more/lower-bound semantics.
- Added indexing timing and discovery snapshots.
- Batched directory indexing writes more aggressively than the previous per-file path.
- Added focused tests for language capability metadata, scanner families, CSS/config/markup extraction, FTS identifier terms, and styling relationships.

Verification completed:

- `cargo test tools::tests::language_support_metadata_marks_markup_variants_partial_support --lib`
- `cargo test tools::tests::language_support_metadata_marks_config_variants_partial_support --lib`
- `cargo test tools::tests::language_support_meta_includes_file_and_supported_languages --lib`
- `cargo check --lib`
- `git diff --check`

Known limitations carried forward:

- The current scanner support is intentionally partial. It is useful for navigation but not equivalent to full tree-sitter parsing for those languages.
- Vue and Svelte support currently indexes shallow template/style-ish signals, not full script projection.
- JSON/YAML/TOML support is broad key-path extraction, not yet file-specific semantic extraction for package scripts, workflow jobs, Docker Compose services, Kubernetes resources, or tsconfig aliases.
- `symbol_search` pagination currently reports a lower-bound style count rather than an exact total.
- The plan still lacks fixture-backed cross-language snapshots and repeatable indexing measurements.

#### Milestone 1: Hardening Current Support

**Completed**: 2026-06-25  
**Status**: Complete.

Completed scope:

- Added fixture-backed scanner-language coverage under `src-tauri/tests/fixtures/symbol_index_languages/`.
- Covered CSS, CSS modules, SCSS, Sass, Less, HTML, Vue, Svelte, JSON, YAML, and TOML with expected-symbol assertions.
- Added `test_symbol_index_language_fixtures_cover_scanner_languages`.
- Added `test_symbol_index_language_fixtures_record_indexing_measurement`.
- Added unsupported-file and shallow-support diagnostics tests for tool response helpers.
- Extended `fast_context` validation to assert active-file language-support metadata and a compact metadata size bound.
- Verified scanner fixture indexing reports supported-file counts, language counts, extracted symbols, and timing snapshots.

Verification completed:

- `cargo test language_service::service::tests::test_symbol_index_language_fixtures --lib`
- `cargo test tools::tests::language_support --lib`
- `cargo test tools::tests::symbol_outline_diagnostics_report --lib`
- `cargo test tools::tests::fast_context_tool_returns_context_pack_payload --lib`
- `cargo check --lib`
- `git diff --check`

Known limitations carried forward:

- Fixture coverage proves current scanner behavior, but scanner languages remain partial by design.
- The measurement is a repeatable unit-level indexing measurement, not a full Criterion benchmark.
- Deeper Vue/Svelte script projection and file-specific config semantics remain deferred to Milestone 6.

### Active

#### Milestone 2: Complete `symbol_search` Contract

**Status**: Not started  
**Primary goal**: Finish the model-facing search contract promised by this plan.

Tasks:

- Add `file_pattern`.
- Add `name_pattern`.
- Add `qualified_name_pattern`.
- Add `include_connected` only if it can reuse existing relationship queries without noisy output.
- Decide whether exact `total` is worth the query cost; otherwise document and keep lower-bound pagination semantics.
- Add tests for file/path filtering, identifier matching, pagination, and FTS fallback behavior.
- Update this document with exact scope and verification commands after completion.

Non-goals:

- Do not add new languages.
- Do not add `symbol_trace`.
- Do not add a full graph search DSL.
- Do not add expensive exact total counts if they hurt latency.

Acceptance criteria:

- `symbol_search` has the intended filters or documented intentional omissions.
- Tool docs match implementation exactly.
- Search remains fast and bounded.
- Existing CSS/config/markup symbol search behavior remains intact.

## Focused Milestone Roadmap

These milestones supersede the original phase ordering for implementation work. The original phase notes remain below as research and detail, but execution should follow this focused sequence.

### Milestone 1: Hardening Current Support

**Goal**: Make the work already landed reliable, tested, and measurable.

**Non-goals**:

- No new language families.
- No new model-facing tool names.
- No deep parser dependency additions.
- No broad schema migration.

**Exit**: Fixtures, diagnostics tests, `fast_context` validation, and scanner indexing measurement are complete.

### Milestone 2: Complete `symbol_search` Contract

**Goal**: Finish the model-facing search contract promised by this plan.

Tasks:

- Add `file_pattern`.
- Add `name_pattern`.
- Add `qualified_name_pattern`.
- Add `include_connected` only if it can reuse existing relationship queries without noisy output.
- Decide whether exact `total` is worth the query cost; otherwise document and keep lower-bound pagination semantics.
- Add tests for file/path filtering, identifier matching, pagination, and FTS fallback behavior.

**Non-goals**:

- No new languages.
- No `symbol_trace`.
- No full graph search DSL.
- No expensive exact total count if it hurts latency.

**Exit**: `symbol_search` has the intended filters or documented intentional omissions, and tool docs match implementation exactly.

### Milestone 3: Index Schema Introspection

**Goal**: Let models inspect index coverage before trusting search results.

Tasks:

- Add `symbol_schema` or an equivalent model-facing schema/coverage response.
- Report symbol type counts.
- Report relationship type counts.
- Report indexed file counts by extension/language.
- Report support-level counts.
- Report unresolved relationship stats and top unresolved targets.
- Feed compact coverage summaries into `fast_context` only when useful.

**Non-goals**:

- No multi-hop traversal.
- No new relationship extraction.
- No rich metadata schema migration unless strictly required for counts.

**Exit**: Models can ask what the index contains and what support level applies before relying on symbol results.

### Milestone 4: Safe Reindexing and Extractor Versioning

**Goal**: Prevent stale shallow/unsupported rows from surviving when extractor behavior changes.

Tasks:

- Add index schema/extractor version metadata.
- Track per-language or per-extractor version where practical.
- Trigger reindex for files whose extractor version changed.
- Keep old rows readable.
- Avoid destructive database resets.

**Non-goals**:

- No broad metadata enrichment.
- No full database redesign.
- No delayed-index rebuild strategy unless measurements show it is needed.

**Exit**: Parser/scanner upgrades do not require users to manually clear the database.

### Milestone 5: Bounded Graph Traversal

**Goal**: Expose the graph data already stored without pretending it is type-aware.

Tasks:

- Add `symbol_trace` for bounded inbound/outbound/both traversal.
- Include visited symbols, edges, hop distance, truncation status, and unresolved edge count.
- Keep traversal limits strict.
- Add tests for direct and multi-hop traversal.

**Non-goals**:

- No type-aware call graph promise.
- No low-confidence call edge expansion for scanner languages.
- No broad graph query language.

**Exit**: Models can ask "what depends on this up to depth N?" with bounded, explainable results.

### Milestone 6: File-Specific Web and Config Enrichment

**Goal**: Improve high-value web/config files with precise extractors instead of broad key-path noise.

Tasks:

- Add JSONC support if dependency and parser cost are acceptable.
- Add package.json scripts/dependencies extraction.
- Add tsconfig path alias extraction.
- Add GitHub Actions workflow job extraction.
- Add Docker Compose service extraction.
- Add route/config-like anchors where confidence is high.
- Add Vue/Svelte script/style projection only after fixtures exist.

**Non-goals**:

- No arbitrary "every JSON key is important" behavior.
- No backend language expansion.
- No framework-specific route graph unless stored as anchors first.

**Exit**: Common config/web files produce compact, useful symbols or anchors with shallow-support diagnostics.

### Milestone 7: Broad Language Baseline

**Goal**: Add backend and systems languages only after the reusable path is proven.

Candidate order:

1. PHP
2. Java
3. C#
4. Kotlin
5. Ruby
6. C/C++
7. Shell
8. Dockerfile
9. SQL
10. Make/CMake

**Non-goals**:

- No semantic/type-aware guarantees in the first pass.
- No language is added without fixtures.
- No parser crate is added without checking build/binary impact.

**Exit**: Each added language has detection, parser/scanner setup, fixture tests, useful definitions/imports where possible, and clear support metadata.

### Milestone 8: Rich Metadata and Optional Similarity

**Goal**: Add metadata and similarity only after structural correctness is stable.

Tasks:

- Evaluate `properties_json` or normalized metadata fields.
- Add relationship confidence/resolution strategy only where it improves model behavior.
- Add route/service anchors as semantic anchors first.
- Explore lexical similarity before embeddings.

**Non-goals**:

- No embedding dependency by default.
- No metadata that bloats normal tool output.
- No schema churn without a ranking/context benefit.

**Exit**: Metadata improves ranking or context selection without making tool output noisy.

## Initial Findings

The findings below describe the state before the refactor started. Some items are now complete and are tracked in **Implementation Progress**.

### Current Supported Languages

The parser gate is `src-tauri/src/tree_sitter/parser.rs`.

`Language::from_path` currently recognizes:

- `.ts`
- `.tsx`
- `.astro`
- `.js`
- `.jsx`
- `.mjs`
- `.cjs`
- `.py`
- `.rs`
- `.go`
- `.md`
- `.markdown`

The tree-sitter parser manager initializes parsers for:

- TypeScript
- TSX
- JavaScript
- JSX
- Python
- Rust
- Go

Markdown is handled separately as heading extraction rather than a tree-sitter parser.

### CSS Is Not Symbol Indexed

CSS is not a supported language in `Language::from_path`.

`is_supported_index_file` in `src-tauri/src/language_service/service.rs` only accepts:

- files with `Language::from_path(file_path).is_some()`
- anchor-only translation resources accepted by `is_anchor_only_index_file`

`is_anchor_only_index_file` only accepts translation-style JSON/YAML resources when the file is not otherwise supported.

Therefore plain `.css` files are not indexed by `symbol_search`.

There is a related but narrower feature: `extract_semantic_anchors` can extract CSS custom property-looking tokens such as `--accent-ai` from files that are already indexed or anchor-indexed. This is why CSS tokens can appear when embedded in TypeScript or config-like files, but not from standalone CSS files.

### Tool UX Problem

The model-facing `symbol_search` tool currently does not clearly tell the model:

- which languages are supported
- which file types are unsupported
- whether an empty result is due to lack of indexed symbols, stale index state, or unsupported language
- when to use `semantic_anchor_search`, `codebase_search`, or `grep_search` instead

This is a correctness problem. Empty symbol search results should be trusted only when the target language and index state are known.

### Current Strengths

The existing implementation is not weak. It already has:

- a persistent SQLite symbol store
- stable-ish symbol IDs based on path, qualified name, and kind
- symbols with file path, ranges, byte offsets, docstrings, signatures, parent IDs, content hashes, and timestamps
- indexed file metadata with hash, size, line count, and modified time
- symbol relationships with source symbol, target name, optional resolved target symbol ID, relationship type, and line
- semantic anchors for route-like strings, commands, translation keys, config-like values, and CSS-looking tokens
- relationship integrity stats
- self-healing search behavior in the language service
- file freshness checks
- symbol references
- related symbols
- one-hop graph lookups
- edit-impact style tooling
- structured fast context integration

The best strategy is to extend this system, not replace it.

## Goals

1. Expand Symbols Index language coverage beyond the current narrow set.
2. Make unsupported language behavior explicit and model-safe.
3. Improve search quality with FTS/BM25, identifier tokenization, pagination, and structural boosts.
4. Add graph-style query capabilities that expose data we already store.
5. Improve indexing throughput for larger repositories.
6. Add richer extraction metadata where it directly improves AI navigation.
7. Keep the implementation incremental and testable.
8. Optimize and simplify the existing implementation before adding broad new surface area.

## Non-Goals

- Do not port `codebase-memory-mcp` wholesale.
- Do not rewrite the index in C.
- Do not add heavyweight language server processes as a hard dependency.
- Do not promise perfect type-aware call graph resolution across all languages in the first expansion.
- Do not make symbol search a replacement for text search.
- Do not index generated/vendor directories just to claim broader coverage.

## Guiding Principles

### 1. Broad Syntax First, Deep Semantics Later

For many languages, a shallow index is still useful:

- definitions
- imports/includes
- exports
- selectors/routes/config keys
- basic containment
- file/module symbols

Deep call resolution and type-aware relationships can come later per language.

### 2. Make Capability Explicit

Every symbol/search response should communicate:

- whether the file type is supported
- whether the relevant file is indexed
- whether the index is fresh, stale, partial, indexing, or unknown
- whether the result came from symbol definitions, semantic anchors, or text fallback

### 3. Prefer Unified Query Shapes

Model-facing tools should guide the model toward good first moves:

- `fast_context` for task orientation
- `symbol_search` for supported structural symbols
- `semantic_anchor_search` for literals, routes, config keys, CSS tokens, translation keys
- `symbol_references` and future `symbol_trace` for impact and relationships
- `codebase_search` or `grep_search` for unsupported files and arbitrary text

### 4. Keep Storage Stable, Add Columns Carefully

The current SQLite store is already useful. Prefer additive schema changes:

- language field
- exported/public flag
- properties JSON for language-specific metadata
- FTS content optimized for identifier search
- optional edge properties for confidence and strategy

Avoid schema churn unless it removes real limitations.

### 5. Optimize Before Expanding

Every expansion should either simplify the code, improve measured behavior, or unlock a clearly valuable capability. Do not add language support by copying a full extractor per language unless the duplication is temporary and scheduled for removal.

Before adding each language batch:

- identify the shared extractor path it will use
- identify the smallest set of symbol kinds worth extracting
- define the support level: full, partial, anchor-only, or unsupported
- measure index time and search latency before and after
- verify binary size and dependency impact
- add fixtures that prevent regressions

This is a better fit for the current codebase than porting the inspiration project's breadth directly. The useful idea to port is not "many parsers"; it is "a compact indexing core that can host many parsers cheaply."

### 6. Prefer Code Shape Improvements Over Feature Count

The implementation should reduce long-term maintenance cost while expanding coverage. Good changes include:

- centralizing language capability metadata instead of scattering extension checks
- making parser registration data-driven
- sharing tree-sitter traversal helpers across languages
- separating extraction into definitions, anchors, relationships, and enrichment passes
- making each pass independently measurable
- replacing ad hoc search ranking with one optimized FTS/query pipeline
- reducing duplicate symbol post-processing logic

Avoid changes that merely increase the number of files, crates, enum variants, or tool options without improving model behavior.

## Optimization-First Decision Gates

Each candidate change should pass these gates before implementation:

1. **User value**: What model failure or developer workflow does this fix?
2. **Reuse**: Can it use the existing store, parser manager, semantic anchor flow, or search API?
3. **Complexity budget**: What new branches, dependencies, schema fields, and tests does it require?
4. **Performance budget**: What is the expected impact on cold index time, incremental index time, DB size, and search latency?
5. **Fallback behavior**: If extraction is partial, how will tools communicate that clearly?
6. **Removal plan**: Does the change replace or simplify an existing path, or only add another path?

Use this as the default review checklist for Symbols Index work.

## Optimization Track

This track should run before and alongside language expansion.

### A. Make Language Capabilities Data-Driven

Replace scattered language assumptions with one capability registry:

```rust
struct LanguageCapability {
    language: Language,
    extensions: &'static [&'static str],
    parser: ParserKind,
    support: SupportLevel,
    extractor_version: u32,
    extracts: ExtractionCapabilities,
}
```

The point is not this exact type. The point is to make support visible, testable, and cheap to extend.

Expected benefits:

- fewer branches in `Language::from_path`
- easier tool diagnostics
- simpler parser registration
- easier language support reporting
- lower risk when adding new extensions

### B. Split Extraction Into Reusable Passes

Current extraction should evolve toward reusable passes:

- file/module identity
- definitions
- containment
- imports/exports
- semantic anchors
- relationships
- language-specific enrichment

New languages should opt into passes instead of getting custom end-to-end extractors by default.

Expected benefits:

- CSS can add selector and custom property extraction without inheriting irrelevant call graph behavior
- shallow languages stay honest
- relationship extraction can be improved independently
- tests can target each pass

### C. Optimize Search Once

The inspiration project's strongest optimization lesson is its search path, not just its parser list. We should avoid adding separate search behavior per language.

Build one optimized path for:

- normalized identifier tokens
- camelCase and snake_case splits
- qualified names
- file path constraints
- symbol kind boosts
- exact/prefix/fuzzy-ish matching
- pagination
- bounded candidate sets before joins

Expected benefits:

- better results for existing languages immediately
- CSS and future languages inherit ranking improvements
- fewer model retries and fallback searches
- less duplicated query code

### D. Measure Indexing Hotspots Before Parallelizing

Do not add concurrency first. Add lightweight counters first:

- files discovered
- files skipped by reason
- files parsed by language
- parse time by language
- extraction time by language
- DB write time
- symbols/anchors/relationships inserted
- stale file checks

Then optimize the measured bottleneck.

Likely optimizations:

- batch DB writes more aggressively
- avoid reparsing unchanged files
- defer enrichment passes
- cap expensive relationship work for large files
- parallelize parse/extract while keeping DB writes serialized or batched

### E. Keep Language Expansion Batch-Sized

Each language batch should have a narrow reason:

- CSS first because it is a known model failure.
- HTML/Vue/Svelte/Astro follow because they are adjacent web-surface languages and share selector/anchor needs.
- PHP/Ruby/C#/Java/Kotlin/Swift follow only after the reusable extractor path is proven.

The target is not "support every language"; the target is "make each supported language cheap, truthful, and useful."

## Phase 0: Capability Diagnostics and Tool Truthfulness

**Goal**: Stop silent false negatives before expanding language support.

### Tasks

1. Add a centralized language capability registry.

   Suggested module:

   - `src-tauri/src/tree_sitter/language_registry.rs`

   Suggested data:

   ```rust
   pub struct LanguageCapability {
       pub id: &'static str,
       pub display_name: &'static str,
       pub extensions: &'static [&'static str],
       pub parser: ParserSupport,
       pub symbol_support: SymbolSupport,
       pub relationship_support: RelationshipSupport,
       pub anchor_support: AnchorSupport,
   }
   ```

2. Add `Language::from_path` tests that assert unsupported common files are intentionally unsupported until implemented:

   - `.css`
   - `.scss`
   - `.html`
   - `.vue`
   - `.svelte`
   - `.php`
   - `.java`
   - `.cpp`
   - `.c`
   - `.yaml`
   - `.json`

3. Add a tool-visible `supported_languages` metadata block to:

   - `symbol_search`
   - `symbol_resolve`
   - `symbol_outline`
   - `symbol_references`
   - `semantic_anchor_search`
   - `fast_context`

4. Add explicit unsupported-file diagnostics.

   Example `symbol_search` scoped to `src/styles/theme.css`:

   ```json
   {
     "results": [],
     "_meta": {
       "tool": "symbol_search",
       "language_support": {
         "path": "src/styles/theme.css",
         "supported": false,
         "reason": "css is not symbol-indexed yet",
         "recommended_tools": ["semantic_anchor_search", "codebase_search", "grep_search"]
       }
     }
   }
   ```

5. Update tool descriptions in:

   - `src-tauri/src/ai_workflow/tool_defs.rs`
   - `src-tauri/src/config.rs`
   - `docs/TOOL_CALLS.md`

6. Add a warning to `symbol_search` empty-result diagnostics when the query appears CSS-like:

   - starts with `.`
   - starts with `#`
   - starts with `--`
   - contains selector combinators like `>` or `:hover`

### Acceptance Criteria

- A model cannot silently mistake CSS unsupported status for "symbol not found."
- Tool output recommends the next correct tool.
- Existing supported-language searches still work.
- Tests cover unsupported file diagnostics.

## Phase 1: CSS and Web Styling Support

**Goal**: Make CSS useful in Symbols Index because it is a visible gap and a common developer need.

### Target File Types

Start with:

- `.css`

Then add:

- `.scss`
- `.sass`
- `.less`
- CSS modules: `.module.css`, `.module.scss`

### Parser Options

Preferred:

- add `tree-sitter-css`

Then evaluate:

- `tree-sitter-scss`

Fallback:

- implement a conservative scanner for CSS selectors and custom properties while parser work lands.

### Symbols to Extract

For `.css`:

- class selectors: `.button`
- id selectors: `#app`
- custom properties: `--color-primary`
- keyframes: `@keyframes fadeIn`
- media query blocks as anchors: `@media (...)`
- container query blocks as anchors: `@container (...)`
- layer names: `@layer components`
- font-face family names: `@font-face { font-family: ... }`

Suggested symbol types:

- add `CssClass`
- add `CssId`
- add `CssVariable`
- add `CssKeyframes`
- add `CssLayer`

Alternative if we want fewer enum variants:

- reuse `SymbolType::Property` for CSS variables
- reuse `SymbolType::Class` for CSS classes
- reuse `SymbolType::Constant` for keyframes/layers

Recommendation: add explicit CSS symbol types. They make results clearer to models and users.

### Relationship Ideas

Phase 1 should not attempt full CSS usage resolution. Add simple relationships only:

- CSS file module contains selectors and variables.
- CSS variable references within CSS declarations can create `uses` or future `usage` edges.

Later:

- connect TSX/JSX `className="..."` and CSS modules `styles.foo` to CSS selectors.
- connect `var(--token)` usage to custom property definitions.

### Tool Behavior

`symbol_search("accent")` should find `--accent-ai` in `.css`.

`symbol_search("button")` should find:

- `.button`
- `.buttonPrimary`
- `button` keyframe/layer/class names where applicable

`semantic_anchor_search("--accent-ai")` should continue working, but CSS definitions should become symbols, not only anchors.

### Tests

Add tests in `src-tauri/src/tree_sitter/symbol.rs` and `src-tauri/src/language_service/service.rs`:

- indexes `.css`
- extracts class selectors
- extracts id selectors
- extracts custom properties
- extracts keyframes
- symbol search finds CSS symbols
- symbol outline returns CSS symbols
- unsupported diagnostic is removed for `.css`
- CSS modules path variants are supported

### Acceptance Criteria

- `.css` is supported by `Language::from_path`.
- `.css` files enter `indexed_files`.
- CSS symbols are persisted with ranges.
- `symbol_search` finds CSS definitions.
- Existing semantic anchor CSS-token extraction still works.

## Phase 2: High-Value Web Framework Languages

**Goal**: Cover the file types common in modern frontend and full-stack web projects.

### Target Languages

Tier 2A:

- HTML
- SCSS
- Vue
- Svelte

Tier 2B:

- JSON
- JSONC
- YAML
- TOML

### HTML Extraction

Parser:

- `tree-sitter-html`

Symbols/anchors:

- ids
- class attributes
- custom elements
- templates
- script/style blocks where practical
- `href`/`src` route-like anchors

Relationships:

- HTML references to CSS classes and ids can later connect to CSS definitions.

### Vue and Svelte Extraction

Parser options:

- tree-sitter Vue/Svelte grammars if mature enough.
- projection approach if parser integration is expensive:
  - extract `<script>` as TS/JS
  - extract `<style>` as CSS
  - extract template ids/classes/components as anchors/symbols

This matches the existing Astro projection strategy and keeps implementation manageable.

Symbols:

- component module symbol
- exported props
- functions/classes/types from script block
- CSS selectors from style block
- template ids/classes as semantic anchors

### JSON/YAML/TOML Extraction

Current JSON/YAML support is anchor-only and translation-resource constrained.

Expand to config-aware indexing:

- package names
- script names from `package.json`
- dependency names
- tsconfig path aliases
- Docker Compose service names
- GitHub Actions job names
- Kubernetes resource names
- environment variable keys
- route/config-like string values

Do not treat all arbitrary JSON keys as high-confidence symbols. Use file-specific extractors.

### Tests

- HTML id/class extraction
- Vue/Svelte script projection symbols
- Vue/Svelte style CSS selectors
- package.json script/dependency anchors
- YAML workflow job anchors
- non-translation JSON no longer throws "not symbol-indexed yet" when config extractor applies

### Acceptance Criteria

- Common frontend files produce useful symbols or anchors.
- `fast_context` can surface Vue/Svelte/HTML/CSS files when tasks mention selectors, components, routes, or config.
- Models get diagnostics when support is shallow rather than full.

## Phase 3: Backend and Broad Developer Language Expansion

**Goal**: Move from "our project language set" to a broad developer baseline.

### Tier 3A: Common Backend and Enterprise

- PHP
- Java
- Kotlin
- C#
- Ruby

### Tier 3B: Systems and Native

- C
- C++
- Header files: `.h`, `.hpp`, `.hh`
- Swift
- Objective-C
- Zig

### Tier 3C: Shell and Build

- Bash
- Zsh
- Fish
- Makefile
- CMake
- Dockerfile
- SQL

### Extraction Minimum Per Language

Each new language should initially provide:

- file/module root symbol
- function/method definitions
- class/type/interface/struct definitions where applicable
- imports/includes/requires where applicable
- parent/child containment
- ranges and byte offsets
- signatures when straightforward
- export/public hints where straightforward

Relationships can start shallow:

- contains
- import/include
- export/public
- direct textual calls only when low-noise

### Language Addition Template

For every new parser:

1. Add dependency in `src-tauri/Cargo.toml`.
2. Add enum variant in `src-tauri/src/tree_sitter/parser.rs`.
3. Add extension mapping in `Language::from_path`.
4. Initialize parser in `TreeSitterParser::new`.
5. Add display name.
6. Add extraction branch in `SymbolExtractor::node_to_symbol`.
7. Add relationship extraction if safe.
8. Add tests:
   - language detection
   - parser initialization
   - symbol extraction
   - indexing and search
   - unsupported diagnostics no longer fire
9. Update docs and tool metadata.

### Acceptance Criteria

- Each language can index a representative fixture with at least definitions and imports.
- Search results are useful enough for model navigation.
- Low-confidence relationship extraction is gated or marked as unresolved.
- No broad false-positive call graph explosion.

## Phase 4: Search Quality and FTS Optimization

**Goal**: Port the best `codebase-memory-mcp` search ideas into the Rust SQLite store.

### Current Gap

The store has FTS5, but the active `execute_search` path uses `search_by_name_like`. This means symbol search does not get the full benefit of FTS ranking.

The inspiration project does several useful things:

- contentless FTS table
- camelCase splitting at index time
- sanitized query tokenization
- BM25 ranking
- structural label boosts
- pagination with `total` and `has_more`
- file pattern filters
- avoids expensive FTS joins by querying top FTS candidates first, then joining/filtering

### Tasks

1. Add normalized FTS columns.

   Options:

   - keep current `symbols_fts`, add `qualified_name`, `symbol_type`, `file_path`, and normalized identifier text
   - or create a new `symbols_search_fts` contentless table

2. Add Rust implementation of identifier token expansion.

   Input examples:

   - `updateCloudClient`
   - `XMLParser`
   - `auth_user`
   - `GitCommitMessage`

   Indexed terms should include both original and split tokens.

3. Add a BM25 search path.

   Suggested API:

   ```rust
   pub struct SymbolSearchParams {
       pub text: Option<String>,
       pub name_pattern: Option<String>,
       pub qualified_name_pattern: Option<String>,
       pub file_pattern: Option<String>,
       pub symbol_types: Vec<SymbolType>,
       pub relationship_type: Option<SymbolRelationshipType>,
       pub min_degree: Option<usize>,
       pub max_degree: Option<usize>,
       pub offset: usize,
       pub limit: usize,
       pub include_connected: bool,
   }
   ```

4. Return pagination metadata.

   ```json
   {
     "total": 431,
     "has_more": true,
     "offset": 0,
     "limit": 50
   }
   ```

5. Add structural boosts.

   Suggested boosts:

   - function/method/component-like symbols: high
   - class/interface/type/struct/trait/enum: medium-high
   - CSS selectors/custom properties: medium
   - imports/modules/headings: lower unless scoped

6. Add file and directory contextual boosts from the existing search layer.

7. Keep current LIKE search as fallback.

### Tests

- camelCase query finds split identifiers.
- snake_case query finds split identifiers.
- multi-word query ranks relevant symbols.
- file pattern filters work.
- pagination total/has_more are correct.
- unsupported FTS syntax cannot be injected through query text.
- LIKE fallback works when FTS query has no usable tokens.

### Acceptance Criteria

- `symbol_search` is better than broad LIKE for natural language-ish symbol discovery.
- Search remains fast on large symbol tables.
- The model can page rather than silently miss results.

## Phase 5: Graph Query and Traversal Tools

**Goal**: Expose graph capabilities already latent in the store.

### Add `symbol_trace`

Current `get_symbol_graph` is one-hop. Add bounded traversal:

```json
{
  "symbol_id": "...",
  "direction": "inbound | outbound | both",
  "relationships": ["call", "import", "export", "extends", "implements"],
  "max_depth": 3,
  "limit": 100
}
```

Output:

- root symbol
- visited symbols with hop distance
- edges
- truncated flag
- unresolved edge count
- confidence metadata

Implementation:

- start with in-memory BFS using existing store methods
- add SQL helpers later if needed

### Add `symbol_schema`

Return:

- symbol type counts
- relationship type counts
- supported language counts
- indexed file counts by extension/language
- unresolved relationship stats
- top unresolved targets

This is inspired by `get_graph_schema` but tailored to the current store.

### Add `symbol_graph_search`

This can be an extension of `symbol_search`, not necessarily a new tool.

Filters:

- symbol type
- file pattern
- relationship type
- min inbound degree
- min outbound degree
- max degree
- include connected names

### Acceptance Criteria

- Models can ask "what calls X up to depth 3?"
- Models can inspect index coverage before trusting searches.
- Fast Context can include graph-level summaries without dumping source code.

## Phase 6: Diff Impact Analysis

**Goal**: Adapt `detect_changes` style impact mapping using our existing symbol and git infrastructure.

### Inputs

- working tree changes
- staged changes
- specific files
- optional line ranges

### Process

1. Map changed line ranges to symbols by file overlap.
2. Add changed file module symbols.
3. Traverse inbound call/import/export relationships for affected symbols.
4. Include related tests and docs.
5. Classify risk by hop distance and relationship type.

### Risk Heuristic

Suggested:

- critical: edited exported symbol with inbound references or module export changes
- high: direct callers/importers
- medium: second-hop dependents
- low: same-file/internal-only symbols or isolated anchors

### Output

```json
{
  "changed_symbols": [],
  "impacted_symbols": [],
  "impacted_files": [],
  "likely_tests": [],
  "risk": {
    "level": "medium",
    "reasons": []
  }
}
```

### Acceptance Criteria

- Editing a function with callers reports those callers.
- Editing exported module surface reports importers.
- Editing CSS custom properties reports local CSS references first, and JSX/className links later when implemented.
- The output recommends next tool calls when impact is uncertain.

## Phase 7: Indexing Performance and Scalability

**Goal**: Make broader language support affordable on real projects.

### Baseline Metrics First

Add debug/perf counters for:

- discovered files
- supported files
- indexed files
- skipped files by reason
- parse time
- extraction time
- DB write time
- relationship resolution time
- semantic anchor extraction time
- total indexing duration
- memory high-water approximation where feasible

Emit these in dev logs and internal debug metadata.

### Parallel Staging

The current batch path already stages file indexes before committing. Expand this into a clearer pipeline:

1. discover files
2. parse/extract in parallel
3. bulk write symbols/anchors/file metadata
4. resolve relationships after all symbols are present
5. bulk write relationships
6. audit graph integrity

Avoid holding the SQLite mutex during parse/extract.

### SQLite Bulk Write Improvements

Evaluate:

- `PRAGMA journal_mode=WAL`
- `PRAGMA synchronous=NORMAL`
- larger cache during bulk indexing
- prepared statement reuse
- explicit transaction boundaries
- delayed index creation only for full rebuilds if needed

Do not compromise crash safety for normal incremental updates.

### File Discovery

Use `ignore` consistently:

- `.gitignore`
- global ignore defaults
- generated/vendor folders
- project setting for allow gitignored files

Add a local ignore mechanism later if needed:

- `.zbladeignore`
- or extend project settings

### Incremental Reindexing

Keep current hash/metadata checks, but add:

- changed extension/language capability invalidation when parser support changes
- schema version and extractor version
- per-language extractor version in indexed metadata

This prevents old "unsupported" or shallow indexed data from persisting after new language support lands.

### Acceptance Criteria

- Indexing remains responsive after adding CSS/HTML/config support.
- Bulk workspace indexing reports useful timings.
- File changes reindex only affected files where possible.
- Parser additions do not require users to manually clear the database.

## Phase 8: Richer Metadata and Semantic Anchors

**Goal**: Add higher-value extracted properties without overfitting to one language.

### Candidate Symbol Properties

Add a `properties_json` column or equivalent normalized fields for:

- language
- exported/public
- async
- test symbol
- entry point
- route path
- route method
- decorators/annotations
- parameters
- return type
- receiver type
- complexity score
- line count
- CSS selector specificity
- CSS module export name

### Candidate Relationship Properties

Add properties for:

- confidence
- resolution strategy
- unresolved reason
- source line/column
- call argument literals

### Route and Service Anchors

Borrow the inspiration project's idea that route nodes are first-class.

Initial route support:

- Express/Fastify route calls in TS/JS
- Next.js app router files
- Python Flask/FastAPI decorators
- Rust Tauri command attributes where relevant
- Go HTTP handler registrations

Store as semantic anchors first; promote to symbols/relationships when reliable.

### Acceptance Criteria

- Rich metadata improves ranking and context selection.
- Missing metadata does not break older rows.
- Tool output remains compact by default.

## Phase 9: Optional Semantic and Similarity Search

**Goal**: Explore meaning-aware retrieval after structural foundations are stronger.

The inspiration project includes local embeddings and similarity edges. That is useful, but it is not the first thing to port.

Potential path:

1. Add lightweight lexical similarity first:
   - identifier token overlap
   - MinHash for function bodies
   - near-duplicate detection
2. Add semantic embedding support behind a feature flag:
   - local-only
   - optional
   - no API dependency
3. Store semantic results separately from symbol search results.

Acceptance criteria:

- Semantic search improves vocabulary mismatch cases.
- It does not slow initial indexing by default.
- It is clearly labeled as semantic, not structural truth.

## Language Support Roadmap

### Immediate

1. CSS
2. SCSS
3. HTML
4. JSON/YAML/TOML config anchors

### Next

5. Vue
6. Svelte
7. PHP
8. Java
9. C#
10. Kotlin

### Then

11. C
12. C++
13. Bash/Zsh/Fish
14. Dockerfile
15. SQL
16. Make/CMake

### Selection Criteria

Prioritize languages by:

- common use in developer projects
- model navigation value
- parser crate maturity
- extraction complexity
- expected false-positive risk
- whether Zaguan Blade editor already has syntax support

## Required Test Fixtures

Create a fixture directory:

- `src-tauri/tests/fixtures/symbol_index_languages/`

Suggested fixtures:

- `css/basic.css`
- `css/modules/button.module.css`
- `html/basic.html`
- `vue/component.vue`
- `svelte/component.svelte`
- `config/package.json`
- `config/github-action.yml`
- `php/basic.php`
- `java/Service.java`
- `cpp/service.cpp`
- `shell/script.sh`

Each fixture should have expected symbols in a small snapshot-like assertion.

## Tool Contract Changes

### `symbol_search`

Add:

- `offset`
- `file_pattern`
- `name_pattern`
- `qualified_name_pattern`
- `include_connected`
- language support metadata
- total/has_more metadata

### `symbol_outline`

Add:

- unsupported file diagnostics
- shallow support diagnostics
- language support metadata

### `semantic_anchor_search`

Clarify:

- this is the correct tool for strings, routes, translation keys, config keys, and CSS-like tokens when structural support is absent or shallow

### New `symbol_trace`

Add after graph traversal exists.

### New `symbol_schema`

Add after schema counts and language coverage metadata exist.

## Migration and Compatibility

1. Keep existing `SymbolType` variants stable where possible.
2. Add new variants with serde compatibility.
3. Add schema migrations through `ensure_column` style helpers.
4. Add an index schema/extractor version.
5. Trigger reindex for files whose extractor version changed.
6. Keep old rows readable.
7. Avoid deleting existing DB data unless a hard integrity issue is found.

## Risks

### Parser Dependency Bloat

Adding many tree-sitter crates increases build time and binary size.

Mitigation:

- add languages in batches
- track binary size
- feature-gate less common languages if needed

### False Positive Relationships

Shallow call extraction can create misleading graph edges.

Mitigation:

- start with definitions/imports/contains for new languages
- add call edges only when tests show acceptable precision
- store confidence and resolution strategy

### Slow Indexing

Broad language support can increase discovered file count significantly.

Mitigation:

- strong ignore rules
- per-language extractor versions
- parallel extraction
- batched DB writes
- perf counters

### Model Misuse

Models may over-trust symbols in shallow languages.

Mitigation:

- tool metadata must include support depth
- diagnostics must recommend source reads before edits
- `fast_context` should report confidence and next steps

## Definition of Done

This plan is complete when:

- CSS is symbol-indexed and searchable.
- Unsupported language results are explicit and actionable.
- Symbol search uses improved FTS/BM25 ranking with identifier tokenization.
- Search results include pagination metadata.
- At least one graph traversal tool supports bounded multi-hop impact exploration.
- The index can report language coverage and schema/relationship counts.
- Fast Context uses the enhanced capabilities without increasing token noise.
- New language support follows a repeatable template with fixtures and tests.

## Recommended First Implementation Slice

**Status**: Complete as Milestone 0 on 2026-06-25.

The first slice was intentionally small, visible, and optimization-led:

1. Add language capability metadata and unsupported diagnostics.
2. Centralize extension/language support rules so future languages do not add scattered checks.
3. Add basic index timing counters for discovery, parse, extraction, and DB writes.
4. Update tool descriptions to expose support depth and fallback behavior.
5. Add FTS/BM25 search path for symbol names and qualified names.
6. Add pagination metadata to `symbol_search`.
7. Add CSS parser/scanner support through the shared capability/extraction path.
8. Extract CSS custom properties, class selectors, id selectors, and keyframes.
9. Add CSS tests and a small fixture-backed benchmark.

This directly addressed the observed CSS failure, improved existing-language search, and created the optimization scaffolding needed for broader support without destabilizing the graph layer.

Future work should now follow the **Focused Milestone Roadmap** near the top of this document instead of treating this first slice as pending.
