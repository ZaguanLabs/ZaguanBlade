//! File-local constant usage: ordinary value references emit resolved Usage
//! edges; lexical shadows (param, let, loop binding, match arm) suppress them.

const WORKFLOW_TEXT: &str = "workflow";
const RETRY_LIMIT: usize = 3;

fn consume(_text: &str, _limit: usize) {}

/// Direct value reference — Usage edge to WORKFLOW_TEXT.
fn direct_use() -> &'static str {
    WORKFLOW_TEXT
}

/// Call-argument references — Usage edges to both constants.
fn call_argument_use() {
    consume(WORKFLOW_TEXT, RETRY_LIMIT);
}

/// Reference inside a closure body — Usage edge to RETRY_LIMIT.
fn closure_use() -> usize {
    let add = |extra: usize| RETRY_LIMIT + extra;
    add(1)
}

/// Parameter shadow: the parameter rebinds the constant name, so the body
/// reference must NOT emit a Usage edge.
fn param_shadow(WORKFLOW_TEXT: &str) -> &str {
    WORKFLOW_TEXT
}

/// `let` initializer shadow: the initializer still sees the outer constant
/// (Usage edge), the reference after the `let` does not.
fn let_shadow() -> usize {
    let RETRY_LIMIT = RETRY_LIMIT + 1;
    RETRY_LIMIT
}

/// Loop-binding shadow: the body reference is the loop variable, not the
/// constant.
fn loop_binding_shadow() -> usize {
    let mut total = 0;
    for RETRY_LIMIT in 0..2 {
        total += RETRY_LIMIT;
    }
    total
}

/// Match-arm binding shadow: the arm pattern rebinds the name; the arm body
/// reference must NOT resolve to the constant. The scrutinee still does.
fn match_arm_shadow(value: usize) -> usize {
    match value + RETRY_LIMIT {
        WORKFLOW_TEXT => WORKFLOW_TEXT,
        other => other,
    }
}
