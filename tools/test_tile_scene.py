#!/usr/bin/env python3
"""Test the production host scene bake without linking the Blueprint target."""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import tomllib
ROOT=Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="quadtexture-tile-tests-") as temporary:
    directory=Path(temporary)
    (directory/"lib.rs").write_text('#![allow(dead_code)]\n#[path='+json.dumps(str(ROOT/'build_support/tile_scene.rs'))+']\nmod tile_scene;\n')
    (directory/"Cargo.toml").write_text(
        '[package]\nname="quadtexture-tile-scene-tests"\nversion="0.0.0"\nedition="2024"\n'
        '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\n'
        'png={path='+json.dumps(str(ROOT.parent/'TRUEOS/vendor/png-0.18.1'))+',default-features=false}\n'
        'miniz_oxide={version="0.9",default-features=false,features=["with-alloc"]}\n'
        'crc32fast={version="1.5",default-features=false}\n'
        'gltf={version="=1.4.1",default-features=false,features=["utils"]}\n'
        'bevy_mikktspace="=0.15.3"\nserde_json="1"\nsha2="0.10"\n'
        'trueos-picasso={path='+json.dumps(str(ROOT.parent/'TRUEOS-Picasso'))+',default-features=false,features=["host"]}\n'
        '[patch.crates-io]\ncrc32fast={path='+json.dumps(str(ROOT.parent/'TRUEOS/vendor/crc32fast-1.5.0'))+'}\n'
        'fdeflate={path='+json.dumps(str(ROOT.parent/'TRUEOS/vendor/fdeflate-0.3.7'))+'}\n')
    env=os.environ.copy();env['TILEPACK_ROOT']=str(ROOT/'assets/industrial_cyberpunk_tilepack')
    env['RUSTUP_TOOLCHAIN']=tomllib.loads((ROOT.parent/'TRUEOS-Blueprints/rust-toolchain.toml').read_text())['toolchain']['channel']
    subprocess.run(['cargo','test','--offline','--manifest-path',str(directory/'Cargo.toml'),'--target-dir',str(ROOT/'target/tile-scene-tests'),'--','--nocapture'],cwd=directory,env=env,check=True)
