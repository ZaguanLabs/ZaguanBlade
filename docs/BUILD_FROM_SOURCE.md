# Building Zaguán Blade from Source

This guide provides instructions on how to build Zaguán Blade from source.

## Prerequisites

Before you begin, ensure you have the following installed:

*   **Node.js** (v18+)
*   **Rust** (v1.75+)
*   **pnpm** (install via `npm install -g pnpm`)
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
    pnpm install
    ```

## Development

To start the application in development mode with hot-reloading:

```bash
pnpm tauri dev
```

This command will start the Vite frontend server and launch the Tauri application window.

## Building for Release

To build the optimized production application:

```bash
pnpm tauri build
```

The build artifacts (e.g., AppImage, Deb, RPM, MSI, DMG) will be located in `src-tauri/target/release/bundle/`.

## Troubleshooting

If you encounter issues during the build process, ensure your generic Rust and Node environments are correctly set up and that you have all necessary platform-specific build tools installed.
