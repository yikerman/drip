#!/usr/bin/env python3
"""Compile real GUI bindings, including rejected ones; run after cargo test.

The GUI is a binary crate, so rustdoc cannot exercise its private binding API.
Compile probes inside a temporary source copy instead of exposing that API or
reimplementing it in a mock. Cargo shares the existing dependency build cache.
"""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[3]
CASES = {
    "valid": None,
    "wrong_identity": "E0308",
    "wrong_inputs": "E0308",
    "wrong_presentation": "E0308",
    "private_binding": "E0451",
}


def check(workspace, name, expected):
    gui = workspace / "crates/drip-gui"
    shutil.copyfile(gui / "tests/bindings" / f"{name}.rs", gui / "src/binding_probe.rs")
    result = subprocess.run(
        [
            os.environ.get("CARGO", "cargo"), "check", "--offline", "--locked",
            "--manifest-path", str(workspace / "Cargo.toml"),
            "--target-dir", str(Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")).resolve()),
            "--package", "drip-gui", "--bin", "drip-gui", "--color", "never",
            "--message-format=json",
        ],
        cwd=ROOT, capture_output=True, text=True, check=False,
    )
    diagnostics = [
        message["message"] for line in result.stdout.splitlines()
        if (message := json.loads(line)).get("reason") == "compiler-message"
    ]
    errors = [message for message in diagnostics if message["level"] == "error"]
    if expected is None:
        passed = result.returncode == 0
    else:
        passed = result.returncode != 0 and bool(errors) and all(
            message.get("code") and message["code"]["code"] == expected
            and any(
                span["is_primary"] and Path(span["file_name"]).name == "binding_probe.rs"
                for span in message["spans"]
            )
            for message in errors
        )
    if not passed:
        rendered = "\n".join(message.get("rendered") or message["message"] for message in diagnostics)
        raise AssertionError(f"{name}: expected {expected or 'successful compilation'}\n{result.stderr}\n{rendered}")
    print(f"{name}: {expected or 'compiles'}", flush=True)


def main():
    with tempfile.TemporaryDirectory(prefix="drip-binding-tests-") as directory:
        workspace = Path(directory)
        for name in ("Cargo.toml", "Cargo.lock"):
            shutil.copyfile(ROOT / name, workspace / name)
        shutil.copytree(ROOT / "crates", workspace / "crates", ignore=shutil.ignore_patterns("target", "__pycache__"))
        main_rs = workspace / "crates/drip-gui/src/main.rs"
        main_rs.write_text(main_rs.read_text() + "\n#[allow(dead_code)]\nmod binding_probe;\n")
        for name, expected in CASES.items():
            check(workspace, name, expected)


if __name__ == "__main__":
    main()
