#!/usr/bin/env python3
"""Create a lossless-PNG 512px GLB texture trial without altering the source.

QuadTexture's build pipeline deliberately accepts only embedded PNG texture
buffer views.  This utility therefore keeps that contract intact: BaseColor is
resampled in sRGB, ORM data is box-filtered, and normal-map vectors are
renormalized after box filtering.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import struct
import tempfile
from pathlib import Path

from PIL import Image


GLB_HEADER = struct.Struct("<4sII")
CHUNK_HEADER = struct.Struct("<I4s")
ALIGNMENT = 4


def aligned(data: bytes, fill: bytes) -> bytes:
    return data + fill * ((-len(data)) % ALIGNMENT)


def read_glb(path: Path) -> tuple[dict, bytes]:
    raw = path.read_bytes()
    magic, version, total_length = GLB_HEADER.unpack_from(raw)
    if (magic, version, total_length) != (b"glTF", 2, len(raw)):
        raise ValueError(f"{path}: expected a valid glTF 2.0 binary file")
    json_length, chunk_type = CHUNK_HEADER.unpack_from(raw, GLB_HEADER.size)
    if chunk_type != b"JSON":
        raise ValueError(f"{path}: JSON must be the first GLB chunk")
    json_start = GLB_HEADER.size + CHUNK_HEADER.size
    document = json.loads(raw[json_start : json_start + json_length])
    bin_header = json_start + json_length
    bin_length, chunk_type = CHUNK_HEADER.unpack_from(raw, bin_header)
    if chunk_type != b"BIN\0":
        raise ValueError(f"{path}: expected an embedded BIN chunk")
    binary = raw[bin_header + CHUNK_HEADER.size : bin_header + CHUNK_HEADER.size + bin_length]
    return document, binary


def png(image: Image.Image) -> bytes:
    encoded = io.BytesIO()
    image.save(encoded, format="PNG", optimize=True, compress_level=9)
    return encoded.getvalue()


def normal_resize(image: Image.Image, size: int) -> Image.Image:
    """Downsample a tangent-space normal map and normalize each result vector."""
    mode = "RGBA" if "A" in image.getbands() else "RGB"
    image = image.convert(mode).resize((size, size), Image.Resampling.BOX)
    pixels = bytearray(image.tobytes())
    channels = len(mode)
    for i in range(0, len(pixels), channels):
        x = pixels[i] / 127.5 - 1.0
        y = pixels[i + 1] / 127.5 - 1.0
        z = pixels[i + 2] / 127.5 - 1.0
        length = max((x * x + y * y + z * z) ** 0.5, 1e-8)
        pixels[i] = round((x / length + 1.0) * 127.5)
        pixels[i + 1] = round((y / length + 1.0) * 127.5)
        pixels[i + 2] = round((z / length + 1.0) * 127.5)
    return Image.frombytes(mode, (size, size), bytes(pixels))


def image_roles(document: dict) -> dict[int, str]:
    materials = document.get("materials", [])
    if len(materials) != 1:
        raise ValueError("this tool currently expects one material")
    material = materials[0]
    pbr = material["pbrMetallicRoughness"]
    base = pbr["baseColorTexture"]["index"]
    orm = pbr["metallicRoughnessTexture"]["index"]
    if material["occlusionTexture"]["index"] != orm:
        raise ValueError("occlusion and metallic-roughness must share the ORM texture")
    normal = material["normalTexture"]["index"]
    textures = document["textures"]
    roles = {
        textures[base]["source"]: "base-color",
        textures[orm]["source"]: "orm",
        textures[normal]["source"]: "normal",
    }
    if len(roles) != 3:
        raise ValueError("base color, ORM, and normal must use distinct images")
    return roles


def resize(source: Path, destination: Path, size: int) -> None:
    document, binary = read_glb(source)
    roles = image_roles(document)
    views = document["bufferViews"]
    images = document["images"]
    image_views = {images[index]["bufferView"] for index in roles}
    if len(image_views) != len(roles) or any(images[i].get("mimeType") != "image/png" for i in roles):
        raise ValueError("expected three distinct embedded PNG image buffer views")

    first_image = min(views[index].get("byteOffset", 0) for index in image_views)
    non_image_end = max(
        (view.get("byteOffset", 0) + view["byteLength"] for index, view in enumerate(views) if index not in image_views),
        default=0,
    )
    if first_image < non_image_end:
        raise ValueError("image buffer views must follow mesh data")

    rebuilt = bytearray(binary[:first_image])
    for image_index, role in roles.items():
        view = views[images[image_index]["bufferView"]]
        start = view.get("byteOffset", 0)
        raw = binary[start : start + view["byteLength"]]
        with Image.open(io.BytesIO(raw)) as source_image:
            if source_image.size != (1024, 1024):
                raise ValueError(f"{source}: {role} is {source_image.size}, not 1024x1024")
            mode = "RGBA" if "A" in source_image.getbands() else "RGB"
            if role == "base-color":
                converted = source_image.convert(mode).resize((size, size), Image.Resampling.LANCZOS)
            elif role == "normal":
                converted = normal_resize(source_image, size)
            else:
                converted = source_image.convert(mode).resize((size, size), Image.Resampling.BOX)
        rebuilt.extend(b"\0" * ((-len(rebuilt)) % ALIGNMENT))
        view["byteOffset"] = len(rebuilt)
        encoded = png(converted)
        view["byteLength"] = len(encoded)
        rebuilt.extend(encoded)

    document["buffers"][0]["byteLength"] = len(rebuilt)
    json_chunk = aligned(json.dumps(document, separators=(",", ":"), ensure_ascii=False).encode(), b" ")
    bin_chunk = aligned(bytes(rebuilt), b"\0")
    result = bytearray(GLB_HEADER.size)
    result.extend(CHUNK_HEADER.pack(len(json_chunk), b"JSON"))
    result.extend(json_chunk)
    result.extend(CHUNK_HEADER.pack(len(bin_chunk), b"BIN\0"))
    result.extend(bin_chunk)
    GLB_HEADER.pack_into(result, 0, b"glTF", 2, len(result))
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as temporary:
        temporary.write(result)
        temporary_path = Path(temporary.name)
    os.replace(temporary_path, destination)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--size", type=int, default=512, choices=(256, 512))
    args = parser.parse_args()
    resize(args.source, args.destination, args.size)


if __name__ == "__main__":
    main()
