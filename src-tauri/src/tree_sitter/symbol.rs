//! Symbol extraction from parsed AST trees
//!
//! Extracts semantic symbols (functions, classes, methods, etc.) from
//! tree-sitter AST trees for indexing and context assembly.

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Tree};
use uuid::Uuid;

use super::parser::Language;

/// Types of symbols we extract from code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolType {
    Function,
    Method,
    Class,
    Struct,
    Interface,
    Type,
    Enum,
    EnumMember,
    Constant,
    Variable,
    Property,
    Module,
    Namespace,
    Import,
    Export,
    Trait,
    Impl,
}

impl std::fmt::Display for SymbolType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SymbolType::Function => "function",
            SymbolType::Method => "method",
            SymbolType::Class => "class",
            SymbolType::Struct => "struct",
            SymbolType::Interface => "interface",
            SymbolType::Type => "type",
            SymbolType::Enum => "enum",
            SymbolType::EnumMember => "enum_member",
            SymbolType::Constant => "constant",
            SymbolType::Variable => "variable",
            SymbolType::Property => "property",
            SymbolType::Module => "module",
            SymbolType::Namespace => "namespace",
            SymbolType::Import => "import",
            SymbolType::Export => "export",
            SymbolType::Trait => "trait",
            SymbolType::Impl => "impl",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for SymbolType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "function" => Ok(SymbolType::Function),
            "method" => Ok(SymbolType::Method),
            "class" => Ok(SymbolType::Class),
            "struct" => Ok(SymbolType::Struct),
            "interface" => Ok(SymbolType::Interface),
            "type" => Ok(SymbolType::Type),
            "enum" => Ok(SymbolType::Enum),
            "enum_member" => Ok(SymbolType::EnumMember),
            "constant" => Ok(SymbolType::Constant),
            "variable" => Ok(SymbolType::Variable),
            "property" => Ok(SymbolType::Property),
            "module" => Ok(SymbolType::Module),
            "namespace" => Ok(SymbolType::Namespace),
            "import" => Ok(SymbolType::Import),
            "export" => Ok(SymbolType::Export),
            "trait" => Ok(SymbolType::Trait),
            "impl" => Ok(SymbolType::Impl),
            _ => Err(format!("Unknown symbol type: {}", s)),
        }
    }
}

/// Position in source code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Range in source code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

impl Range {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    pub fn from_node(node: &Node) -> Self {
        let start = node.start_position();
        let end = node.end_position();
        Self {
            start: Position::new(start.row as u32, start.column as u32),
            end: Position::new(end.row as u32, end.column as u32),
        }
    }
}

/// A symbol extracted from source code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    /// Unique identifier (UUID v4)
    pub id: String,
    /// Symbol name (e.g., function name, class name)
    pub name: String,
    /// Type of symbol
    pub symbol_type: SymbolType,
    /// File path where symbol is defined
    pub file_path: String,
    /// Range in source code
    pub range: Range,
    /// Parent symbol ID (for methods inside classes, etc.)
    pub parent_id: Option<String>,
    /// Documentation string if present
    pub docstring: Option<String>,
    /// Type signature (for functions: parameters and return type)
    pub signature: Option<String>,
}

impl Symbol {
    pub fn new(name: String, symbol_type: SymbolType, file_path: String, range: Range) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            symbol_type,
            file_path,
            range,
            parent_id: None,
            docstring: None,
            signature: None,
        }
    }

    pub fn with_parent(mut self, parent_id: String) -> Self {
        self.parent_id = Some(parent_id);
        self
    }

    pub fn with_docstring(mut self, docstring: String) -> Self {
        self.docstring = Some(docstring);
        self
    }

    pub fn with_signature(mut self, signature: String) -> Self {
        self.signature = Some(signature);
        self
    }
}

/// Symbol extractor for extracting symbols from AST trees
pub struct SymbolExtractor {
    file_path: String,
}

impl SymbolExtractor {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }

    /// Extract all symbols from a tree
    pub fn extract(&self, tree: &Tree, source: &str, language: Language) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        self.extract_from_node(tree.root_node(), source, language, None, &mut symbols);
        symbols
    }

    fn extract_from_node(
        &self,
        node: Node,
        source: &str,
        language: Language,
        parent_id: Option<&str>,
        symbols: &mut Vec<Symbol>,
    ) {
        // Extract symbol from this node if applicable
        if let Some(mut symbol) = self.node_to_symbol(&node, source, language) {
            if let Some(pid) = parent_id {
                symbol.parent_id = Some(pid.to_string());
            }

            // Try to extract docstring
            if let Some(doc) = self.extract_docstring(&node, source, language) {
                symbol.docstring = Some(doc);
            }

            // Try to extract signature
            if let Some(sig) = self.extract_signature(&node, source, language) {
                symbol.signature = Some(sig);
            }

            let symbol_id = symbol.id.clone();
            symbols.push(symbol);

            // Process children with this symbol as parent
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    self.extract_from_node(child, source, language, Some(&symbol_id), symbols);
                }
            }
        } else {
            // No symbol at this node, process children with same parent
            for i in 0..node.child_count() {
                if let Some(child) = node.child(i) {
                    self.extract_from_node(child, source, language, parent_id, symbols);
                }
            }
        }
    }

    fn node_to_symbol(&self, node: &Node, source: &str, language: Language) -> Option<Symbol> {
        match language {
            Language::TypeScript | Language::Tsx => self.typescript_node_to_symbol(node, source),
            Language::JavaScript | Language::Jsx => self.javascript_node_to_symbol(node, source),
            Language::Python => self.python_node_to_symbol(node, source),
            Language::Rust => self.rust_node_to_symbol(node, source),
        }
    }

    fn typescript_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "function_declaration" | "function" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Function,
                    self.file_path.clone(),
                    range,
                ))
            }
            "method_definition" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Method,
                    self.file_path.clone(),
                    range,
                ))
            }
            "class_declaration" | "class" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Class,
                    self.file_path.clone(),
                    range,
                ))
            }
            "interface_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Interface,
                    self.file_path.clone(),
                    range,
                ))
            }
            "type_alias_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Type,
                    self.file_path.clone(),
                    range,
                ))
            }
            "enum_declaration" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Enum,
                    self.file_path.clone(),
                    range,
                ))
            }
            "arrow_function" => {
                // Arrow functions assigned to variables/constants
                if let Some(parent) = node.parent() {
                    if parent.kind() == "variable_declarator" {
                        if let Some(name_node) = parent.child_by_field_name("name") {
                            let name = name_node.utf8_text(source.as_bytes()).ok()?;
                            return Some(Symbol::new(
                                name.to_string(),
                                SymbolType::Function,
                                self.file_path.clone(),
                                Range::from_node(&parent),
                            ));
                        }
                    }
                }
                None
            }
            "lexical_declaration" => {
                // const/let declarations - check if it's a function expression
                if let Some(declarator) = node.child_by_field_name("declarator") {
                    if let Some(value) = declarator.child_by_field_name("value") {
                        if value.kind() == "arrow_function" || value.kind() == "function" {
                            // Already handled by arrow_function case
                            return None;
                        }
                    }
                    // Regular constant
                    let name = self.get_child_text(&declarator, "name", source)?;
                    let is_const = node.utf8_text(source.as_bytes()).ok()?.starts_with("const");
                    Some(Symbol::new(
                        name,
                        if is_const {
                            SymbolType::Constant
                        } else {
                            SymbolType::Variable
                        },
                        self.file_path.clone(),
                        range,
                    ))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn javascript_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        // JavaScript uses similar structure to TypeScript
        self.typescript_node_to_symbol(node, source)
    }

    fn python_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "function_definition" => {
                let name = self.get_child_text(node, "name", source)?;
                // Check if it's a method (inside a class)
                let is_method = node
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|gp| gp.kind() == "class_definition")
                    .unwrap_or(false);
                Some(Symbol::new(
                    name,
                    if is_method {
                        SymbolType::Method
                    } else {
                        SymbolType::Function
                    },
                    self.file_path.clone(),
                    range,
                ))
            }
            "class_definition" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Class,
                    self.file_path.clone(),
                    range,
                ))
            }
            _ => None,
        }
    }

    fn rust_node_to_symbol(&self, node: &Node, source: &str) -> Option<Symbol> {
        let kind = node.kind();
        let range = Range::from_node(node);

        match kind {
            "function_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Function,
                    self.file_path.clone(),
                    range,
                ))
            }
            "struct_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Struct,
                    self.file_path.clone(),
                    range,
                ))
            }
            "enum_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Enum,
                    self.file_path.clone(),
                    range,
                ))
            }
            "trait_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Trait,
                    self.file_path.clone(),
                    range,
                ))
            }
            "impl_item" => {
                // Get the type being implemented
                if let Some(type_node) = node.child_by_field_name("type") {
                    let name = type_node.utf8_text(source.as_bytes()).ok()?;
                    Some(Symbol::new(
                        format!("impl {}", name),
                        SymbolType::Impl,
                        self.file_path.clone(),
                        range,
                    ))
                } else {
                    None
                }
            }
            "type_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Type,
                    self.file_path.clone(),
                    range,
                ))
            }
            "const_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Constant,
                    self.file_path.clone(),
                    range,
                ))
            }
            "mod_item" => {
                let name = self.get_child_text(node, "name", source)?;
                Some(Symbol::new(
                    name,
                    SymbolType::Module,
                    self.file_path.clone(),
                    range,
                ))
            }
            _ => None,
        }
    }

    fn get_child_text(&self, node: &Node, field_name: &str, source: &str) -> Option<String> {
        node.child_by_field_name(field_name)
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string())
    }

    fn extract_docstring(&self, node: &Node, source: &str, _language: Language) -> Option<String> {
        // Look for comment node immediately before this node
        if let Some(prev) = node.prev_sibling() {
            let kind = prev.kind();
            if kind == "comment" || kind == "block_comment" || kind == "line_comment" {
                return prev.utf8_text(source.as_bytes()).ok().map(|s| {
                    // Clean up comment markers
                    let s = s.trim();
                    let s = s.strip_prefix("///").unwrap_or(s);
                    let s = s.strip_prefix("//").unwrap_or(s);
                    let s = s.strip_prefix("/*").unwrap_or(s);
                    let s = s.strip_suffix("*/").unwrap_or(s);
                    let s = s.strip_prefix("#").unwrap_or(s);
                    let s = s.strip_prefix("\"\"\"").unwrap_or(s);
                    let s = s.strip_suffix("\"\"\"").unwrap_or(s);
                    s.trim().to_string()
                });
            }
        }
        None
    }

    fn extract_signature(&self, node: &Node, source: &str, language: Language) -> Option<String> {
        match language {
            Language::TypeScript | Language::Tsx | Language::JavaScript | Language::Jsx => {
                // For functions, extract parameters
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.utf8_text(source.as_bytes()).ok()?;
                    // Try to get return type
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| format!(" {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Python => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.utf8_text(source.as_bytes()).ok()?;
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| format!(" -> {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
            Language::Rust => {
                if let Some(params) = node.child_by_field_name("parameters") {
                    let params_text = params.utf8_text(source.as_bytes()).ok()?;
                    let return_type = node
                        .child_by_field_name("return_type")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| format!(" {}", s))
                        .unwrap_or_default();
                    return Some(format!("{}{}", params_text, return_type));
                }
            }
        }
        None
    }
}

/// Convenience function to extract symbols from source code
pub fn extract_symbols(
    tree: &Tree,
    source: &str,
    language: Language,
    file_path: &str,
) -> Vec<Symbol> {
    let extractor = SymbolExtractor::new(file_path.to_string());
    extractor.extract(tree, source, language)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_sitter::TreeSitterParser;

    #[test]
    fn test_extract_typescript_function() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "function authenticate(token: string): boolean { return true; }";
        let tree = parser.parse(code, Language::TypeScript).unwrap();

        let symbols = extract_symbols(&tree, code, Language::TypeScript, "test.ts");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "authenticate");
        assert_eq!(symbols[0].symbol_type, SymbolType::Function);
    }

    #[test]
    fn test_extract_typescript_class() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
class UserService {
    private users: User[] = [];

    getUser(id: string): User | undefined {
        return this.users.find(u => u.id === id);
    }
}
"#;
        let tree = parser.parse(code, Language::TypeScript).unwrap();
        let symbols = extract_symbols(&tree, code, Language::TypeScript, "service.ts");

        // Should have class and method
        assert!(symbols
            .iter()
            .any(|s| s.name == "UserService" && s.symbol_type == SymbolType::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "getUser" && s.symbol_type == SymbolType::Method));
    }

    #[test]
    fn test_extract_rust_function() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = "fn greet(name: &str) -> String { format!(\"Hello, {}!\", name) }";
        let tree = parser.parse(code, Language::Rust).unwrap();

        let symbols = extract_symbols(&tree, code, Language::Rust, "lib.rs");

        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "greet");
        assert_eq!(symbols[0].symbol_type, SymbolType::Function);
    }

    #[test]
    fn test_extract_python_class() {
        let mut parser = TreeSitterParser::new().unwrap();
        let code = r#"
class Calculator:
    def add(self, a: int, b: int) -> int:
        return a + b

    def subtract(self, a: int, b: int) -> int:
        return a - b
"#;
        let tree = parser.parse(code, Language::Python).unwrap();
        let symbols = extract_symbols(&tree, code, Language::Python, "calc.py");

        assert!(symbols
            .iter()
            .any(|s| s.name == "Calculator" && s.symbol_type == SymbolType::Class));
        assert!(symbols
            .iter()
            .any(|s| s.name == "add" && s.symbol_type == SymbolType::Method));
        assert!(symbols
            .iter()
            .any(|s| s.name == "subtract" && s.symbol_type == SymbolType::Method));
    }
}
