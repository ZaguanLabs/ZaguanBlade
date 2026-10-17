//! RFC 6570 expansion for MCP resource templates, levels 1–3 without value
//! modifiers (`:prefix`, `*explode`). Unsupported syntax fails rather than
//! producing a URI the server did not describe.
use super::RuntimeError;
use std::collections::HashMap;

pub const MAX_TEMPLATE_BYTES: usize = 2048;
pub const MAX_VARIABLES: usize = 16;

/// Variable names in order of first appearance.
pub fn variables(template: &str) -> Result<Vec<String>, RuntimeError> {
    let mut names = Vec::new();
    for expression in expressions(template)? {
        for name in expression.names {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    if names.len() > MAX_VARIABLES {
        return Err(RuntimeError::InvalidArguments);
    }
    Ok(names)
}

/// Missing or empty variables are undefined and omitted, as RFC 6570 specifies.
pub fn expand(template: &str, values: &HashMap<String, String>) -> Result<String, RuntimeError> {
    let mut output = String::new();
    let mut rest = template;
    for expression in expressions(template)? {
        let (literal, tail) = rest.split_at(expression.start - (template.len() - rest.len()));
        output.push_str(&encode(literal, true));
        rest = &tail[expression.len..];
        let (first, separator, named, reserved) = match expression.operator {
            None => ("", ",", false, false),
            Some('+') => ("", ",", false, true),
            Some('#') => ("#", ",", false, true),
            Some('.') => (".", ".", false, false),
            Some('/') => ("/", "/", false, false),
            Some(';') => (";", ";", true, false),
            Some('?') => ("?", "&", true, false),
            Some('&') => ("&", "&", true, false),
            Some(_) => return Err(RuntimeError::InvalidArguments),
        };
        let mut parts = Vec::new();
        for name in &expression.names {
            let Some(value) = values.get(name).filter(|value| !value.is_empty()) else {
                continue;
            };
            let value = encode(value, reserved);
            parts.push(if named {
                format!("{name}={value}")
            } else {
                value
            });
        }
        if !parts.is_empty() {
            output.push_str(first);
            output.push_str(&parts.join(separator));
        }
    }
    output.push_str(&encode(rest, true));
    if output.len() > MAX_TEMPLATE_BYTES {
        return Err(RuntimeError::InvalidArguments);
    }
    Ok(output)
}

struct Expression {
    start: usize,
    len: usize,
    operator: Option<char>,
    names: Vec<String>,
}

fn expressions(template: &str) -> Result<Vec<Expression>, RuntimeError> {
    if template.len() > MAX_TEMPLATE_BYTES {
        return Err(RuntimeError::InvalidArguments);
    }
    let mut found = Vec::new();
    let mut offset = 0;
    while let Some(open) = template[offset..].find(['{', '}']) {
        let start = offset + open;
        if template.as_bytes()[start] == b'}' {
            return Err(RuntimeError::InvalidArguments);
        }
        let close = template[start..]
            .find('}')
            .ok_or(RuntimeError::InvalidArguments)?;
        let body = &template[start + 1..start + close];
        if body.contains('{') || body.is_empty() {
            return Err(RuntimeError::InvalidArguments);
        }
        let (operator, list) = match body.chars().next() {
            Some(c @ ('+' | '#' | '.' | '/' | ';' | '?' | '&')) => (Some(c), &body[1..]),
            Some('=' | ',' | '!' | '@' | '|') => return Err(RuntimeError::InvalidArguments),
            _ => (None, body),
        };
        let names = list
            .split(',')
            .map(|name| {
                let valid = !name.is_empty()
                    && name.split('.').all(|part| {
                        !part.is_empty()
                            && part
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                    });
                valid
                    .then(|| name.to_owned())
                    .ok_or(RuntimeError::InvalidArguments)
            })
            .collect::<Result<Vec<_>, _>>()?;
        found.push(Expression {
            start,
            len: close + 1,
            operator,
            names,
        });
        offset = start + close + 1;
    }
    Ok(found)
}

/// Percent-encode everything outside the unreserved set; `reserved` also keeps
/// RFC 3986 reserved characters and existing percent-encoded triplets.
fn encode(value: &str, reserved: bool) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        let kept = reserved && b":/?#[]@!$&'()*+,;=".contains(&byte);
        let triplet = reserved
            && byte == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit);
        if unreserved || kept {
            output.push(byte as char);
        } else if triplet {
            output.push_str(&value[index..index + 3]);
            index += 2;
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
        index += 1;
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> HashMap<String, String> {
        [
            ("var", "value"),
            ("hello", "Hello World!"),
            ("path", "/foo/bar"),
            ("x", "1024"),
            ("y", "768"),
            ("empty", ""),
        ]
        .into_iter()
        .map(|(name, value)| (name.into(), value.into()))
        .collect()
    }

    #[test]
    fn rfc_6570_level_three_examples_expand() {
        let values = values();
        for (template, expected) in [
            ("{var}", "value"),
            ("{hello}", "Hello%20World%21"),
            ("{+hello}", "Hello%20World!"),
            ("{+path}/here", "/foo/bar/here"),
            ("here?ref={+path}", "here?ref=/foo/bar"),
            ("X{#var}", "X#value"),
            ("map?{x,y}", "map?1024,768"),
            ("{x,hello,y}", "1024,Hello%20World%21,768"),
            ("{+x,hello,y}", "1024,Hello%20World!,768"),
            ("X{.x,y}", "X.1024.768"),
            ("{/var,x}/here", "/value/1024/here"),
            ("{;x,y}", ";x=1024;y=768"),
            ("{?x,y}", "?x=1024&y=768"),
            ("?fixed=yes{&x}", "?fixed=yes&x=1024"),
            ("file:///{path}", "file:///%2Ffoo%2Fbar"),
            ("x{empty}y{?missing}", "xy"),
        ] {
            assert_eq!(expand(template, &values).unwrap(), expected, "{template}");
        }
        assert_eq!(
            variables("{/var,x}/here{?var,y}").unwrap(),
            ["var", "x", "y"]
        );
    }

    #[test]
    fn unsupported_or_malformed_templates_fail() {
        let values = values();
        for template in [
            "{var:3}", "{list*}", "{=var}", "{", "}", "{}", "{a{b}}", "{a-b}", "{a,}",
        ] {
            assert!(expand(template, &values).is_err(), "{template}");
        }
        assert!(expand(&"a".repeat(MAX_TEMPLATE_BYTES + 1), &values).is_err());
        let many = (0..=MAX_VARIABLES)
            .map(|index| format!("{{v{index}}}"))
            .collect::<String>();
        assert!(variables(&many).is_err());
    }
}
