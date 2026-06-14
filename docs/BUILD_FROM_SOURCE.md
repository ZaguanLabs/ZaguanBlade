# Building Zaguán Blade from Source

This guide describes the current source build for Zaguán Blade `0.8.2`.

## Prerequisites

Install these tools before building:

- **Bun** `1.3+`
- **Rust** with Cargo
- **Node.js** `20.19+` or `22.12+` only if you run Vite tooling directly instead of through Bun

Linux builds also need the Tauri/WebKitGTK system packages. On Debian or Ubuntu:

```bash
sudo apt update
sudo apt install -y \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev
```

## Install

```bash
git clone https://github.com/ZaguanLabs/ZaguanBlade.git
cd ZaguanBlade
bun install
```

## Development

Run the desktop app with the Vite dev server and Tauri shell:

```bash
bun run tauri dev
```

The Tauri config starts Vite on `http://localhost:1420` and opens the desktop window.

## Frontend-Only Commands

These are useful when you only need the React/Vite side:

```bash
bun run dev
bun run build
bun run preview
bun run lint
bun run test
```

## Release Build

```bash
bun run tauri build
```

Release bundles are written under `src-tauri/target/release/bundle/`.

The current Tauri bundle targets are:

- Linux: AppImage, `.deb`, `.rpm`
- Windows: NSIS, MSI
- macOS: `.app`, DMG

On Linux, the AppImage target is configured with `bundleMediaFramework: false`, so runtime WebKit/media dependencies may still need to be present on the target system.

## Troubleshooting

- If Vite fails before Tauri launches, verify Bun is installed and that any direct Node.js usage meets the Vite 8 Node requirement.
- If Rust compilation fails, update your Rust toolchain with `rustup update`.
- If Linux linking fails, recheck the WebKitGTK and GTK development packages above.
