# Zaguán Blade

> **The AI Editor (The Body)**

Zaguán Blade (`zblade`) is the graphical frontend for the Zaguán AI system. It serves as the "Body" to the "Brain" ([zcoderd](#zcoderd-the-brain)).

Built with **Tauri v2** and **Next.js**, it provides a modern, high-performance interface for AI-assisted coding, featuring a custom CodeMirror 6 editor and deep integration with the Blade Protocol.

## Architecture

The system follows a strict Body/Brain separation:

*   **Zaguán Blade (The Body)**: This repository. A lightweight GUI client that handles user input, file rendering, and editor visualizations. It possesses no AI logic itself.
*   **Zcoderd (The Brain)**: An external, high-performance Go server that manages state, executes tools, performs web research, and drives the AI models.
*   **Blade Protocol v2**: The communication layer between Body and Brain, allowing the AI to "pilot" the editor.

## Project Status: Alpha

> [!IMPORTANT]
> **External Dependency Required**: This project is the *client only*. To function, it requires a running instance of `zcoderd`.
>
> currently, `zcoderd` is:
> 1.  **Mandatory**: The client cannot function without it.
> 2.  **Hardcoded**: The client expects the server at a specific localhost address.
> 3.  **Private**: The `zcoderd` repository is currently private ("The Secret Sauce").
>
> Future versions aim to make `zcoderd`.

## Key Features

*   **Visual Editor**: A heavily customized CodeMirror 6 implementation with "Vertical Diff Blocks" for AI code generation.
*   **Web Tools Visualization**: The client visualizes the server's research context, showing what the AI is reading and thinking.
*   **Blade Protocol Integration**: seamless, real-time sync between the editor state and the AI's context.

## Getting Started

### Prerequisites

*   Node.js (v18+)
*   Rust (v1.70+)
*   pnpm
*   **A running instance of `zcoderd`**

### Installation

1.  Clone the repository:
    ```bash
    git clone https://github.com/ZaguanLabs/ZaguanBlade.git
    cd zaguan-blade
    ```

2.  Install dependencies:
    ```bash
    pnpm install
    ```

### Development

Start the Taurus development window:

```bash
pnpm tauri dev
```

This will spin up the Next.js frontend and the Tauri Rust, backend.

### Building

To build the application for release:

```bash
pnpm tauri build
```

## License

MIT
