#!/usr/bin/env python3
"""Run the gallery's production camera code with the real Picasso math types."""
from pathlib import Path
import json
import os
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BLUEPRINTS = ROOT.parent / "TRUEOS-Blueprints"


def main() -> None:
    api = (BLUEPRINTS / "crates/trueos-v/src/vgpu.rs").read_text()
    camera = re.search(r"(?m)^pub struct RetainedCamera \{.*?^\}", api, re.DOTALL)
    if camera is None:
        raise ValueError("RetainedCamera ABI declaration missing")
    with tempfile.TemporaryDirectory(prefix="quadtexture-camera-tests-") as temporary:
        directory = Path(temporary)
        (directory / "lib.rs").write_text(
            "#![allow(dead_code)]\nextern crate self as trueos;\n"
            "pub mod vgpu {\n" + camera.group() + "\n}\n"
            "#[path = " + json.dumps(str(ROOT / "src/camera.rs")) + "]\nmod camera;\n"
        )
        (directory / "Cargo.toml").write_text(
            '[package]\nname="quadtexture-camera-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nlibm="0.2"\n'
            'trueos-picasso={path=' + json.dumps(str(ROOT.parent / "TRUEOS-Picasso"))
            + ',default-features=false}\n'
        )
        env = os.environ.copy()
        env["RUSTUP_TOOLCHAIN"] = tomllib.loads((BLUEPRINTS / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        subprocess.run([
            "cargo", "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target-dir", str(ROOT / "target/camera-host-tests"),
        ], cwd=directory, env=env, check=True)


if __name__ == "__main__":
    main()
