"""Install local npm/wheel artifacts and exercise their actual stdio servers."""

import argparse
import json
import os
from pathlib import Path
from queue import Queue, Empty
import shutil
import subprocess
import sys
import tempfile
from threading import Thread
import venv
import zipfile

from package_registries import ROOT, PLATFORMS, ARCHITECTURES, metadata


def smoke(command, version):
    with tempfile.TemporaryDirectory(prefix="affine-data-") as directory:
        data = Path(directory) / "AFFiNE data with spaces"
        (data / "workspaces" / "local").mkdir(parents=True)
        env = os.environ.copy()
        env.update({"AFFINE_DATA_DIR": str(data), "AFFINE_ENABLE_WRITES": "0", "AFFINE_WORKSPACE_IDS": ""})
        actual = subprocess.check_output([*command, "--version"], text=True, env=env, timeout=20).strip()
        assert actual == version, actual
        check = json.loads(subprocess.check_output([*command, "--data-dir", str(data), "--check"], text=True, env=env, timeout=20))
        assert check == {"workspaces": [], "warnings": [], "writesEnabled": False}, check
        invalid = subprocess.run([*command, "--not-a-real-option"], capture_output=True, text=True, env=env, timeout=20)
        assert invalid.returncode != 0 and invalid.stdout == "" and "Unknown argument" in invalid.stderr, invalid
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
        stdin, stdout, stderr = child.stdin, child.stdout, child.stderr
        assert stdin is not None and stdout is not None and stderr is not None
        messages = Queue()

        def receive():
            for line in stdout:
                messages.put(line)

        Thread(target=receive, daemon=True).start()

        def send(message):
            stdin.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
            stdin.flush()

        def request(identifier, method, params):
            send({"id": identifier, "method": method, "params": params})
            while True:
                try:
                    response = json.loads(messages.get(timeout=20))
                except Empty:
                    raise AssertionError(f"MCP timeout for {method}") from None
                if response.get("id") == identifier:
                    assert "error" not in response, response
                    return response["result"]

        try:
            init = request(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "package-smoke", "version": "1"}})
            assert "tools" in init["capabilities"], init
            send({"method": "notifications/initialized"})
            tools = request(2, "tools/list", {})["tools"]
            assert any(tool["name"] == "affine_list_workspaces" for tool in tools), tools
            result = request(3, "tools/call", {"name": "affine_list_workspaces", "arguments": {}})
            assert result["isError"] is False, result
            assert result["structuredContent"] == check, result
            stdin.close()
            assert child.wait(timeout=20) == 0, stderr.read()
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=20)
            stdin.close()
            stdout.close()
            stderr.close()
        # A wrapper must propagate a signal sent by a supervising MCP client.
        if os.name != "nt":
            child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
            signal_stdin, signal_stdout, signal_stderr = child.stdin, child.stdout, child.stderr
            assert signal_stdin is not None and signal_stdout is not None and signal_stderr is not None
            try:
                # Initialization proves the native child has started before the signal.
                signal_stdin.write((json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "signal-smoke", "version": "1"}}}) + "\n").encode())
                signal_stdin.flush()
                lines = Queue()
                Thread(target=lambda: lines.put(signal_stdout.readline()), daemon=True).start()
                assert json.loads(lines.get(timeout=20))["id"] == 1
                child.terminate()
                assert child.wait(timeout=20) == -15
            finally:
                if child.poll() is None:
                    child.kill()
                    child.wait(timeout=20)
                signal_stdin.close()
                signal_stdout.close()
                signal_stderr.close()
    print(f"Verified {command[0]}: version, arguments, environment, exit errors, MCP initialize/tools/call and shutdown")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True, choices=PLATFORMS)
    parser.add_argument("--arch", required=True, choices=ARCHITECTURES)
    parser.add_argument("--source", action="store_true", help="Also build/install the sdist with Rust")
    args = parser.parse_args()
    version = metadata()["version"]
    dist = ROOT / "dist"
    native = dist / f"affine-desktop-mcp-{PLATFORMS[args.platform]}-{ARCHITECTURES[args.arch]}-{version}.tgz"
    main_package = dist / f"affine-desktop-mcp-{version}.tgz"
    wheels = list(dist.glob("*.whl"))
    assert len(wheels) == 1, f"Expected one native wheel, found {wheels}"
    wheel = wheels[0]
    with zipfile.ZipFile(wheel) as archive:
        names = archive.namelist()
        assert any(name.startswith("affine_desktop_mcp/bin/") for name in names), names
        wheel_metadata = archive.read(next(name for name in names if name.endswith(".dist-info/WHEEL"))).decode()
        assert "Root-Is-Purelib: false" in wheel_metadata and "Tag: py3-none-" in wheel_metadata, wheel_metadata
        assert "-any" not in wheel_metadata, wheel_metadata
        if args.platform == "macos":
            wheel_arch = "x86_64" if args.arch == "amd64" else "arm64"
            assert wheel.name.endswith(f"_{wheel_arch}.whl"), wheel.name
            assert "_universal2" not in wheel_metadata, wheel_metadata
            assert f"_{wheel_arch}\n" in wheel_metadata, wheel_metadata
    with tempfile.TemporaryDirectory(prefix="affine-install-") as directory:
        root = Path(directory)
        npm_root = root / "npm"
        npm = shutil.which("npm")
        assert npm is not None, "npm is required for package smoke tests"
        subprocess.run([npm, "install", "--prefix", str(npm_root), "--offline", "--ignore-scripts", "--no-audit", "--no-fund", str(main_package), str(native)], check=True)
        suffix = ".cmd" if os.name == "nt" else ""
        smoke([str(npm_root / "node_modules" / ".bin" / f"affine-desktop-mcp{suffix}")], version)
        python_root = root / "python"
        venv.create(python_root, with_pip=True)
        scripts = python_root / ("Scripts" if os.name == "nt" else "bin")
        python = scripts / ("python.exe" if os.name == "nt" else "python")
        subprocess.run([str(python), "-m", "pip", "install", "--no-index", "--no-deps", str(wheel)], check=True)
        smoke([str(scripts / ("affine-desktop-mcp.exe" if os.name == "nt" else "affine-desktop-mcp"))], version)
        smoke([str(python), "-m", "affine_desktop_mcp"], version)
        if args.source:
            sources = list(dist.glob("affine_desktop_mcp-*.tar.gz"))
            assert len(sources) == 1, sources
            env = os.environ.copy()
            env.pop("AFFINE_DESKTOP_MCP_BINARY", None)
            # Reuse dependency builds, and exercise a non-default target path.
            cargo = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT, text=True))
            env["CARGO_TARGET_DIR"] = cargo["target_directory"]
            subprocess.run([str(python), "-m", "pip", "install", "--force-reinstall", "--no-deps", str(sources[0])], env=env, check=True)
            smoke([str(scripts / ("affine-desktop-mcp.exe" if os.name == "nt" else "affine-desktop-mcp"))], version)


if __name__ == "__main__":
    main()
