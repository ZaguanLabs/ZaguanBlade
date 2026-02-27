import os

with open("src/index.css", "r") as f:
    content = f.read()

# Replace the simple cursor definitions with robust SVG base64 cursors
svg_ew = 'url("data:image/svg+xml,%3Csvg xmlns=\'http://www.w3.org/2000/svg\' width=\'24\' height=\'24\' viewBox=\'0 0 24 24\'%3E%3Cg stroke=\'black\' stroke-width=\'4\' stroke-linecap=\'round\' stroke-linejoin=\'round\'%3E%3Cpath d=\'m16 8 4 4-4 4\'/%3E%3Cpath d=\'M4 12h16\'/%3E%3Cpath d=\'m8 8-4 4 4 4\'/%3E%3C/g%3E%3Cg stroke=\'white\' stroke-width=\'2\' stroke-linecap=\'round\' stroke-linejoin=\'round\'%3E%3Cpath d=\'m16 8 4 4-4 4\'/%3E%3Cpath d=\'M4 12h16\'/%3E%3Cpath d=\'m8 8-4 4 4 4\'/%3E%3C/g%3E%3C/svg%3E") 12 12, ew-resize, col-resize !important;'

svg_ns = 'url("data:image/svg+xml,%3Csvg xmlns=\'http://www.w3.org/2000/svg\' width=\'24\' height=\'24\' viewBox=\'0 0 24 24\'%3E%3Cg stroke=\'black\' stroke-width=\'4\' stroke-linecap=\'round\' stroke-linejoin=\'round\'%3E%3Cpath d=\'m8 16 4 4 4-4\'/%3E%3Cpath d=\'M12 4v16\'/%3E%3Cpath d=\'m8 8 4-4 4 4\'/%3E%3C/g%3E%3Cg stroke=\'white\' stroke-width=\'2\' stroke-linecap=\'round\' stroke-linejoin=\'round\'%3E%3Cpath d=\'m8 16 4 4 4-4\'/%3E%3Cpath d=\'M12 4v16\'/%3E%3Cpath d=\'m8 8 4-4 4 4\'/%3E%3C/g%3E%3C/svg%3E") 12 12, ns-resize, row-resize !important;'

content = content.replace("cursor: ew-resize !important;", f"cursor: {svg_ew}")
content = content.replace("cursor: ns-resize !important;", f"cursor: {svg_ns}")

with open("src/index.css", "w") as f:
    f.write(content)
