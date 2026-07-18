//! Fixture 2 — file-local constant usage vs. lexical shadowing.

const RETRY_LIMIT: usize = 3;

fn direct_use() -> usize {
    RETRY_LIMIT
}

fn call_argument_use() {
    consume(RETRY_LIMIT);
}

fn closure_use() -> usize {
    let add = |extra: usize| RETRY_LIMIT + extra;
    add(1)
}

fn parameter_shadow(RETRY_LIMIT: usize) -> usize {
    RETRY_LIMIT
}

fn let_shadow() -> usize {
    let RETRY_LIMIT = RETRY_LIMIT + 1;
    RETRY_LIMIT
}

fn loop_shadow() {
    for RETRY_LIMIT in 0..3 {
        consume(RETRY_LIMIT);
    }
}

fn match_shadow(value: Option<usize>) -> usize {
    match value {
        Some(RETRY_LIMIT) => RETRY_LIMIT,
        None => 0,
    }
}
