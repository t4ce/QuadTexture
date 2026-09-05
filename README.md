# QuadTexture

The default view and key **1** submit one native `QUAD_LIST` primitive with four
indices. Key **3** switches to two `TRIANGLE_LIST` primitives with six indices;
key **1** switches back. Both modes use the same four position/UV vertices,
Intel Graphics logo texture, sampler, and UV texture shader.

The native quad uses the indexed texture submission's explicit topology field.
It requires the kernel/API update that supports this field; zero still selects
triangle lists for older clients. The app does not expand the quad into a
triangle list.

The host tests exercise key transitions, release handling, native quad indices,
and matching winding and UV coverage between the two modes:

```sh
rustc --edition 2024 --test src/geometry.rs -o /tmp/quadtexture-geometry-tests
/tmp/quadtexture-geometry-tests
```

Hardware comparison: open the app, compare the full logo in the default view
with key **3**, then return with key **1**. Texture orientation and coverage
should remain the same.
