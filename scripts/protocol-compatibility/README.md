# Protocol compatibility fixtures

Run from the repository root:

```sh
bun run test:protocols
```

This standalone test crate checks `rmcp = 3.5.1` and
`agent-client-protocol = 3.3.0`, matching Blade's application SDK versions.
Its independent lockfile makes the checks reproducible. ACP's unstable v2
feature is disabled; SDK version 3.3.0 is tested with stable wire protocol v1.

Raw JSON peers exercise MCP discovery for 2026-07-28, automatic fallback to
2025-11-25 after rejected discovery, tool discovery and structured tool results.
The ACP fixture creates a session, sends a prompt, declines a permission request,
and receives a message update before prompt completion. Each conversation has a
ten-second timeout and runs over in-memory byte streams without model accounts.

These fixtures check SDK wire interoperability. They do not qualify
subprocess supervision, packaged executable lookup, Streamable HTTP, OAuth, MCP
elicitation, real Atlas Scout workspace selection, or real ACP agents. The
broader child-process, HTTP, authentication and elicitation feature sets are
enabled here to check compilation separately from the application's narrower
SDK feature sets.
