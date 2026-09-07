# Protocol compatibility fixtures

Run from the repository root:

```sh
bun run test:protocols
```

This standalone test crate evaluates `rmcp = 3.2.0` and
`agent-client-protocol = 2.1.0` without adding either SDK to Blade's application
dependency graph. Its lockfile makes the checks reproducible. ACP's unstable v2
feature is disabled; SDK version 2.1.0 is tested with stable wire protocol v1.

Raw JSON peers exercise MCP discovery for 2026-07-28, automatic fallback to
2025-11-25 after rejected discovery, tool discovery and structured tool results.
The ACP fixture creates a session, sends a prompt, declines a permission request,
and receives a message update before prompt completion. Each conversation has a
ten-second timeout and runs over in-memory byte streams without model accounts.

These checks establish initial SDK interoperability only. They do not qualify
subprocess supervision, packaged executable lookup, Streamable HTTP, OAuth, MCP
elicitation, real Atlas Scout workspace selection, or real ACP agents. The
intended child-process, HTTP, authentication and elicitation feature sets are
enabled here so their dependencies are compiled before runtime adoption.
