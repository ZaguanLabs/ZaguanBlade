# Building Zaguán Blade from Source

This guide provides instructions on how to build Zaguán Blade from source.

## Prerequisites

Before you begin, ensure you have the following installed:

*   **Bun** (v1.3+)
*   **Node.js** (v20.19+ or v22.12+, only if you run Vite tooling directly without Bun)
*   **Rust** (v1.75+)
*   **System Dependencies** (Linux only):
    *   `libwebkit2gtk-4.1-dev`
    *   `build-essential`
    *   `curl`
    *   `wget`
    *   `file`
    *   `libssl-dev`
    *   `libgtk-3-dev`
    *   `libayatana-appindicator3-dev`
    *   `librsvg2-dev`

## Installation

1.  **Clone the repository:**
    ```bash
    git clone https://github.com/ZaguanLabs/ZaguanBlade.git
    cd ZaguanBlade
    ```

2.  **Install dependencies:**
    ```bash
    bun install
    ```

## Development

To start the application in development mode with hot-reloading:

```bash
bun run tauri dev
```

This command will start the Vite frontend server and launch the Tauri application window.

## Building for Release

To build the optimized production application:

```bash
bun run tauri build
```

The build artifacts (e.g., AppImage, Deb, RPM, MSI, DMG) will be located in `src-tauri/target/release/bundle/`.

## Troubleshooting

If you encounter issues during the build process, ensure your Bun and Rust environments are correctly set up, and if you are invoking Vite directly without Bun, verify that your Node.js version meets Vite 8 requirements.
