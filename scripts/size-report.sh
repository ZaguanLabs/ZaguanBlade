#!/usr/bin/env bash
# scripts/size-report.sh — P0 size reporting for the optimization plan.
#
# Reports executable, stripped copy, installer (if present), frontend dist
# total, initial preload graph, and largest chunks.
#
# Usage:
#   scripts/size-report.sh [--exe PATH] [--dist PATH]
#
# Defaults:
#   --exe  src-tauri/target/release/zblade
#   --dist dist
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

EXE="${ROOT_DIR}/src-tauri/target/release/zblade"
DIST="${ROOT_DIR}/dist"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --exe)  EXE="$2";  shift 2 ;;
        --dist) DIST="$2"; shift 2 ;;
        *) echo "Unknown option: $1" >&2; exit 1 ;;
    esac
done

bytes_fmt() {
    local b="$1"
    if [[ "$b" -ge 1048576 ]]; then
        awk -v b="$b" 'BEGIN { printf "%.2f MB", b / 1048576 }'
    elif [[ "$b" -ge 1024 ]]; then
        awk -v b="$b" 'BEGIN { printf "%.2f KB", b / 1024 }'
    else
        echo "${b} bytes"
    fi
}

gzip_size() {
    local file="$1"
    local tmp
    tmp="$(mktemp)"
    gzip -9 -c "$file" > "$tmp"
    local sz
    sz="$(stat -c %s "$tmp" 2>/dev/null || stat -f %z "$tmp" 2>/dev/null)"
    rm -f "$tmp"
    echo "$sz"
}

echo "=== Zaguán Blade Size Report ==="
echo "Date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo ""

# --- Native executable ---
if [[ -f "$EXE" ]]; then
    exe_size="$(stat -c %s "$EXE" 2>/dev/null || stat -f %z "$EXE" 2>/dev/null)"
    exe_gzip="$(gzip_size "$EXE")"

    stripped="${EXE}.stripped"
    if command -v strip &>/dev/null; then
        cp "$EXE" "$stripped"
        strip --strip-unneeded "$stripped" 2>/dev/null || true
        stripped_size="$(stat -c %s "$stripped" 2>/dev/null || stat -f %z "$stripped" 2>/dev/null)"
        stripped_gzip="$(gzip_size "$stripped")"
        rm -f "$stripped"
    else
        stripped_size="N/A (strip not found)"
        stripped_gzip="N/A"
    fi

    echo "--- Native executable ---"
    echo "  Executable:          $(bytes_fmt "$exe_size") ($exe_size bytes)"
    echo "  Executable gzip -9:  $(bytes_fmt "$exe_gzip") ($exe_gzip bytes)"
    if [[ "$stripped_size" != N/A* ]]; then
        echo "  Stripped:            $(bytes_fmt "$stripped_size") ($stripped_size bytes)"
        echo "  Stripped gzip -9:    $(bytes_fmt "$stripped_gzip") ($stripped_gzip bytes)"
        local_savings=$((exe_size - stripped_size))
        echo "  Strip savings:       $(bytes_fmt "$local_savings") ($local_savings bytes)"
    else
        echo "  Stripped:            $stripped_size"
    fi
else
    echo "--- Native executable ---"
    echo "  Not found at $EXE (run cargo build --release first)"
fi
echo ""

# --- Installers ---
echo "--- Installers ---"
for ext in deb rpm AppImage msi dmg app; do
    found="$(find "${ROOT_DIR}/src-tauri/target/release/bundle" -name "*.${ext}" 2>/dev/null | head -1 || true)"
    if [[ -n "$found" ]]; then
        sz="$(stat -c %s "$found" 2>/dev/null || stat -f %z "$found" 2>/dev/null)"
        echo "  ${ext}: $(bytes_fmt "$sz") ($sz bytes) — $(basename "$found")"
    fi
done
echo ""

# --- Frontend dist ---
if [[ -d "$DIST" ]]; then
    dist_total="$(du -sb "$DIST" 2>/dev/null | cut -f1 || du -sk "$DIST" | awk '{print $1 * 1024}')"
    echo "--- Frontend distribution ---"
    echo "  Total dist: $(bytes_fmt "$dist_total") ($dist_total bytes)"
    echo ""

    # Initial preload graph from index.html
    html="${DIST}/index.html"
    if [[ -f "$html" ]]; then
        echo "--- Initial preload graph ---"
        preload_js=0
        preload_js_gzip=0
        preload_css=0
        preload_files=()
        preload_seen=""
        while IFS= read -r line; do
            href="$(echo "$line" | grep -oP 'href="\K[^"]+' || true)"
            if [[ -n "$href" ]]; then
                file="${DIST}/${href}"
                if [[ -f "$file" ]]; then
                    sz="$(stat -c %s "$file" 2>/dev/null || stat -f %z "$file" 2>/dev/null)"
                    if [[ "$href" == *.js && " $preload_seen " != *" $href "* ]]; then
                        preload_seen="$preload_seen $href"
                        preload_js=$((preload_js + sz))
                        preload_js_gzip=$((preload_js_gzip + $(gzip_size "$file")))
                        preload_files+=("$(basename "$href")")
                    elif [[ "$href" == *.css ]]; then
                        preload_css=$((preload_css + sz))
                    fi
                fi
            fi
            src="$(echo "$line" | grep -oP 'src="\K[^"]+' || true)"
            if [[ -n "$src" ]]; then
                file="${DIST}/${src}"
                if [[ -f "$file" && " $preload_seen " != *" $src "* ]]; then
                    preload_seen="$preload_seen $src"
                    sz="$(stat -c %s "$file" 2>/dev/null || stat -f %z "$file" 2>/dev/null)"
                    preload_js=$((preload_js + sz))
                    preload_js_gzip=$((preload_js_gzip + $(gzip_size "$file")))
                    preload_files+=("$(basename "$src")")
                fi
            fi
        done < <(grep -E 'modulepreload|module.*src=|stylesheet' "$html")

        echo "  Preloaded JS (raw):   $(bytes_fmt "$preload_js") ($preload_js bytes)"
        echo "  Preloaded JS (gzip):  $(bytes_fmt "$preload_js_gzip") ($preload_js_gzip bytes)"
        echo "  Preloaded CSS (raw):  $(bytes_fmt "$preload_css") ($preload_css bytes)"
        echo "  Preload files: ${preload_files[*]}"
    fi
    echo ""

    # Largest assets
    echo "--- Largest dist assets (top 15) ---"
    find "$DIST" -type f -not -path "*/locales/*" -exec stat -c '%s %n' {} \; 2>/dev/null \
        | sort -rn | head -15 | while read -r sz path; do
            rel="${path#${DIST}/}"
            echo "  $(bytes_fmt "$sz") — $rel"
        done
else
    echo "--- Frontend distribution ---"
    echo "  dist not found at $DIST (run bun run build first)"
fi
echo ""
echo "=== End report ==="
