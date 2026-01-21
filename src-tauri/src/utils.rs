use std::path::Path;

/// Parse @command syntax and extract tool name and query
/// Returns (actual_message, Option<(tool_name, query)>)
pub fn parse_command(message: &str) -> (String, Option<(String, String)>) {
    let trimmed = message.trim();

    // Check if message starts with @command
    if trimmed.starts_with("@research ") {
        let query = trimmed.strip_prefix("@research ").unwrap().to_string();
        return (message.to_string(), Some(("research".to_string(), query)));
    } else if trimmed.starts_with("@search ") {
        let query = trimmed.strip_prefix("@search ").unwrap().to_string();
        return (message.to_string(), Some(("search".to_string(), query)));
    } else if trimmed.starts_with("@web ") {
        let query = trimmed.strip_prefix("@web ").unwrap().to_string();
        return (message.to_string(), Some(("fetch_url".to_string(), query)));
    }

    // No command found, return original message
    (message.to_string(), None)
}

pub fn extract_root_command(command: &str) -> Option<String> {
    let first_segment = command
        .split(|c| c == '|' || c == ';')
        .next()
        .unwrap_or(command);
    let first_segment = first_segment.split("&&").next().unwrap_or(first_segment);
    let first_segment = first_segment.split("||").next().unwrap_or(first_segment);

    let mut it = first_segment.split_whitespace().peekable();
    while let Some(tok) = it.peek().copied() {
        if tok == "sudo" || tok == "env" || tok == "command" || tok == "time" {
            it.next();
            continue;
        }
        if tok.contains('=') && !tok.starts_with("./") && !tok.contains('/') {
            it.next();
            continue;
        }
        break;
    }
    it.next().map(|s| s.to_string())
}

pub fn is_cwd_outside_workspace(ws_root: Option<&str>, cwd: Option<&str>) -> Option<bool> {
    let ws_root = ws_root?;
    let cwd = cwd?;
    let ws = std::fs::canonicalize(Path::new(ws_root)).ok()?;
    let p = Path::new(cwd);
    let candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        ws.join(p)
    };
    let candidate = std::fs::canonicalize(&candidate).ok()?;
    Some(!candidate.starts_with(&ws))
}
