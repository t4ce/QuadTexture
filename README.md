# QuadTexture

Startup and key **3** show a 6 × 4 gallery of all 24 GLBs in
`assets/industrial_cyberpunk_tilepack`, with a **16 × 16 floor beneath it**.
The floor repeats the silver X-braced `reinforced_metal_wall2` panel in its
196 interior cells and uses pale `bathroom_tiles_1` panels for the 60 outer
border cells. Tiles meet edge to edge, with their upper surfaces aligned.
The gallery retains all **604 authored triangles**, including slab thickness,
cables, vents, and other raised details. Both floor meshes retain their full
source geometry. Rendering uses Picasso Example's retained PBR triangle path
with a perspective camera and depth testing.

| Control | Action |
| --- | --- |
| WASD | Move the scene camera |
| Middle-button drag | Look around |
| R | Frame the gallery and floor from above in the current window |
| 1 | Show the existing Intel logo native quad probe |
| 3 | Return to the triangle gallery and floor |

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

The assets share compatible opaque material factors. The source GLB images
have been reduced offline to **512 × 512**. Three lossless RGBA8 PNG atlases
preserve all 72 of those images: base color, packed occlusion/roughness/metallic,
and normals. Each atlas is **3096 × 2064**, with two
texels of repeat padding per tile. UVs are remapped into those slots. The packed
atlas is bound to both the occlusion and metallic/roughness roles, sharing one
GPU texture. There are no emissive textures in this pack.

Prepared geometry, instance seeds, atlases, and provenance are stored in Picasso's separate
chunked derived-artifact tables. The database is closed and reopened again;
only the reloaded, hash-verified artifacts are emitted for the app. The build's
`OUT_DIR/tilepack.picasso.redb` retains the sources and derived scene;
`tile_scene.json` records source hashes/revisions, transforms, bindings, bounds,
triangle counts, atlas hashes, and every floor placement. `tile_scene.seeds`
contains the 257 compact TRS records. This is generated build output, not the
Picasso idea/proof seed database.

At runtime, the prepared assets enter Picasso's **ephemeral in-memory redb**
store and are read back before GPU upload. The kernel decodes each atlas once;
geometry and textures remain resident while the camera moves. Runtime changes
are not persisted to the rig filesystem. Runtime database values use 60 KiB
chunks and a bounded cache to avoid oversized allocations during atlas insertion.

The scene uses the additive `RetainedFrameSubmitV3` API: one retained mesh,
three index ranges, and 257 instances (gallery + 196 interior + 60 border).
Only one local geometry copy of each floor tile is appended to the gallery:
**1,616 resident vertices and 2,484 indices**, drawing **42,876 triangles**
across the instances. The GPU writes the instance matrices, compaction indices,
and three indirect draws. The floor shares the existing atlas slots; it adds
no texture uploads or per-tile texture copies. V1/V2 retained submissions keep
their original four inline seeds and unchanged wire layouts.

## Current rendering limits

The gallery uses the existing PBR shader and its lighting approximation. It
samples the base mip level with bilinear filtering; the source mipmap sampler
settings are recorded but mip chains are not generated. All source image
texels from the reduced assets are preserved, so minification can shimmer at
a distance. This update requires a matching kernel with retained-frame V3
support (up to 512 instances and four draw ranges). The three resident atlases
use about **73 MiB** of GPU texture memory, the same as the 512 px gallery.

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
and repeat padding. They also check all 256 placements, the two source meshes,
shared UVs, contiguous instance slots, touching edges, and aligned upper surfaces.
Camera tests cover projection depth, transforms, inverse matrices, resizing,
and the elevated floor reset view. The kernel's `tools/test_retained_scene.py`
checks the real V3 decoder, range validation, unchanged older ABI layouts, and
compaction slot separation; pass a generated `tile_scene.json` to exercise the
persisted app seeds through those kernel helpers. The kernel's
`tools/test_vmedia_image_capacity.py` exercises its image admission and actual
vendored PNG decoder. Busy surface acquisition, fence waits, and publication
resume the same frame; only begin/failed-submit paths restart frame acquisition.

To package locally without publishing:

```sh
cd ../TRUEOS-Blueprints
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 cargo bp QuadTexture
```

The 512 px gallery has been confirmed on hardware. The added floor and V3
handoff still need a hardware run. Deploy the matching kernel and QuadTexture
package, then check the 60-tile pale border and X-braced interior beneath the
gallery, WASD/middle-drag, reset, resizing, and switching between keys 1 and 3.
Building and host checks do not establish a hardware pass.
