# QuadTexture

Startup and key **3** show a 6 × 4 gallery of all 24 GLBs in
`assets/industrial_cyberpunk_tilepack`. The scene retains all **604 authored
triangles**, including slab thickness, cables, vents, and other raised details.
It uses Picasso Example's retained PBR triangle renderer with a perspective
camera and depth testing.

| Control | Action |
| --- | --- |
| WASD | Move the gallery camera |
| Middle-button drag | Look around |
| R | Refit the gallery camera to the current window |
| 1 | Show the existing Intel logo native quad probe |
| 3 | Return to the triangle gallery |

Key 1 still submits four indices as one native `QUAD_LIST` primitive using its
original position/UV shader. The gallery is an independent `TRIANGLE_LIST`
mesh; it does not reconstruct quads from the GLBs.

## Assets and persistence

The build first inspects GLB JSON metadata, imports each exact source into a
Picasso database, closes that database, and reopens it. Preparation reads the
persisted source blobs, composes each default scene's node transforms, then
applies an explicit gallery placement. Source corner order, positions, normals,
and UVs are checked before placement. The three assets without tangents receive
MikkTSpace tangents, splitting vertices where necessary without changing their
triangles. Authored tangent directions are retained; the manifest records
24 authored tangent frames parallel to their normals.

The assets share compatible opaque material factors. Three lossless RGBA8 PNG
atlases preserve all 72 original 1024 × 1024 images: base color, packed
occlusion/roughness/metallic, and normals. Each atlas is 6168 × 4112, with two
texels of repeat padding per tile. UVs are remapped into those slots. The packed
atlas is bound to both the occlusion and metallic/roughness roles, sharing one
GPU texture. There are no emissive textures in this pack.

Prepared geometry, atlases, and provenance are stored in Picasso's separate
chunked derived-artifact tables. The database is closed and reopened again;
only the reloaded, hash-verified artifacts are emitted for the app. The build's
`OUT_DIR/tilepack.picasso.redb` retains the sources and derived scene;
`tile_scene.json` records source hashes/revisions, transforms, bindings, bounds,
triangle counts, and atlas hashes. This is generated build output, not the
Picasso idea/proof seed database.

At runtime, the prepared assets enter Picasso's **ephemeral in-memory redb**
store and are read back before GPU upload. The kernel decodes each atlas once;
geometry and textures remain resident while the camera moves. Runtime changes
are not persisted to the rig filesystem. Runtime database values use 60 KiB
chunks and a bounded cache to avoid oversized allocations during atlas insertion.

## Current rendering limits

The gallery uses the existing PBR shader and its lighting approximation. It
samples the base mip level with bilinear filtering; the source mipmap sampler
settings are recorded but mip chains are not generated. All source image
texels are preserved, so minification can shimmer at a distance. This update
requires the kernel's 64 MiB encoded-image admission limit; decoded image and
dimension limits remain 128 MiB and 8192. The three resident atlases use about
290 MiB of GPU texture memory.

## Local checks and build

```sh
python3 tools/test_tile_scene.py
python3 tools/test_camera.py
rustc --edition 2024 --test src/geometry.rs -o /tmp/quadtexture-geometry-tests
/tmp/quadtexture-geometry-tests
rustc --edition 2024 --test src/frame_retry.rs -o /tmp/quadtexture-frame-retry-tests
/tmp/quadtexture-frame-retry-tests
```

The scene tests reopen the database, verify exact source bytes and triangle
corners, and decode the prepared atlases to compare every original image pixel
and repeat padding. Camera tests cover projection depth, transforms, inverse
matrices, and resizing. The kernel's
`tools/test_vmedia_image_capacity.py` exercises its image admission and actual
vendored PNG decoder. Busy surface acquisition, fence waits, and publication
resume the same frame; only begin/failed-submit paths restart frame acquisition.

To package locally without publishing:

```sh
cd ../TRUEOS-Blueprints
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp QuadTexture
```

Hardware validation is pending. Deploy the matching kernel and QuadTexture
package, then check all 24 tiles, WASD/middle-drag, reset, resizing, and switching
between keys 1 and 3. Building and host checks do not establish a hardware pass.
