# Zaguán Blade

**The AI-Native Code Editor.**

Zaguán Blade (`zblade`) is a high-performance, AI-first code editor built for the future of development. It serves as the graphical interface (the "Body") that connects to the Zaguán AI system (the "Brain").

> [!WARNING]
> **Alpha Release (v0.0.4-alpha)**
> This is an early Alpha release. It is functional and "good enough" for daily use, but please expect bugs and incomplete features.
> *   Current Limitation: **Diff views are currently non-functional.**
> *   Stability: Experimental but usable.

## Why build another AI editor?

Zaguán Blade isn't just another AI editor. It's a combination of a code editor and an AI system backend. Together they create a whole and I had two goals in mind when I started this project:

1. **AI-Native Workflow**: Deeply integrated AI that understands your project context.
2. **Save Money**: Vibe Coding sends a lot of data to the providers and they charge a lot for it. The server I created does its best to send only what is relevant while also making sure that the model has enough context to understand your project.

These 2 systems work together to create a whole that is much more than the sum of its parts. I spent a lot of time getting the server to work well and borrowed a lot of knowledge from many other open source projects like Cline, Roo-Code, OpenCode, Codex, Gemini-CLI, Qwen-Code, and many others.

### Active Development

Zaguán Blade is currently in active development. We are working on many new features and improvements and things may be unstable at times while I update the server. I will try my very best to keep the updates regular and give a heads up, but that's not a guarantee during this phase of development.

There are many things that I've planned for both Zaguán Blade and the server too numerous to list here.

The server and the system prompts are relatively opinionated tailored to my preferences and the way I like to work.

I'm also planning on updating the GUI that emphasizes more the AI-first approach and workflow. I was mostly inspired by the many VSCode forks out there like Windsurf, Cursor et al during the initial development just to get something working.

## Requirement: Zaguán AI Subscription

Zaguán Blade is powered by our hosted AI backend. To use the AI features (Chat, Code Generation, Auto-fix), you **must have an active subscription**.

👉 **[Get a Subscription at ZaguanAI.com](https://zaguanai.com/pricing)**

Without a subscription and a valid API Key, Zaguán Blade functions as a standard (albeit very nice) text editor.

## Key Features

*   **AI-Native Workflow**: Deeply integrated AI that understands your project context.
*   **Performance**: Built with **Rust** (Tauri v2) and **React**, offering near-native performance with the flexibility of web technologies.
*   **Blade Protocol**: Utilizes our custom BP (Blade Protocol) for high-fidelity communication between the editor and the AI. *Note: The Blade Protocol specification is currently internal.*

## Installation

We provide pre-built binaries for Windows (`.msi`, `.exe`), macOS (`.dmg`, `.app`), and Linux (`.AppImage`, `.deb`, `.rpm`).

Check the **[Releases](https://github.com/ZaguanLabs/ZaguanBlade/releases)** page for the latest version.

### Building from Source

If you prefer to build the editor yourself, please refer to our **[Build Guide](docs/BUILD_FROM_SOURCE.md)**.

## Feedback & Contributions

We welcome feedback, bug reports, and Pull Requests!

*   **Found a bug?** Please open an issue on our [GitHub Issue Tracker](https://github.com/ZaguanLabs/ZaguanBlade/issues).
*   **Have an idea?** Start a discussion or submit a PR.
*   **Community:** Join us in building the next generation of coding tools.

## License

MIT
