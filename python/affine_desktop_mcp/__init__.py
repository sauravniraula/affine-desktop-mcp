"""Launch the bundled AFFiNE Desktop stdio MCP server."""

import os
from pathlib import Path
import subprocess
import sys


def main():
    executable = "affine-local-mcp.exe" if os.name == "nt" else "affine-local-mcp"
    binary = Path(__file__).resolve().parent / "bin" / executable
    if not binary.is_file():
        print("affine-desktop-mcp: bundled executable is missing; reinstall the package.", file=sys.stderr)
        return 1
    try:
        if os.name != "nt":
            # Replacing the launcher preserves stdio, signals and exit status.
            os.execv(str(binary), [str(binary), *sys.argv[1:]])
        child = subprocess.Popen([str(binary), *sys.argv[1:]])
        try:
            return child.wait()
        except KeyboardInterrupt:
            # Windows console Ctrl-C is also delivered to the native process.
            return child.wait()
    except OSError as error:
        print(f"affine-desktop-mcp: {error}", file=sys.stderr)
        return 1
