//! Host-only, triangle-preserving tilepack preparation through Picasso persistence.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use sha2::{Digest, Sha256};
use serde_json::{Value, json};
use trueos_picasso::Store;

pub const COLS: usize = 6;
pub const ROWS: usize = 4;
pub const TILE: usize = 1024;
pub const GUTTER: usize = 2;
pub const SLOT: usize = TILE + GUTTER * 2;
pub const ATLAS_WIDTH: usize = COLS * SLOT;
pub const ATLAS_HEIGHT: usize = ROWS * SLOT;
const SPACING: f32 = 2.5;
type Matrix = [[f32; 4]; 4]; // glTF column-major.

pub struct PreparedScene {
    pub vertices: Vec<u8>,
    pub indices: Vec<u8>,
    pub atlases: [Vec<u8>; 3],
    pub metadata: Value,
}

pub fn sha(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn identity() -> Matrix { core::array::from_fn(|c| core::array::from_fn(|r| if c == r { 1.0 } else { 0.0 })) }
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    core::array::from_fn(|c| core::array::from_fn(|r| (0..4).map(|k| a[k][r] * b[c][k]).sum()))
}
fn point(m: Matrix, p: [f32; 3]) -> [f32; 3] {
    core::array::from_fn(|r| m[0][r]*p[0] + m[1][r]*p[1] + m[2][r]*p[2] + m[3][r])
}
fn vector(m: Matrix, p: [f32; 3]) -> [f32; 3] {
    core::array::from_fn(|r| m[0][r]*p[0] + m[1][r]*p[1] + m[2][r]*p[2])
}
fn cross(a: [f32;3], b: [f32;3]) -> [f32;3] { [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]] }
fn dot(a: [f32;3], b: [f32;3]) -> f32 { a.into_iter().zip(b).map(|(a,b)| a*b).sum() }
fn unit(v: [f32;3]) -> [f32;3] {
    let len = dot(v,v).sqrt(); assert!(len.is_finite() && len > 1e-15, "invalid direction");
    v.map(|v| v/len)
}
fn determinant(m: Matrix) -> f32 { dot([m[0][0],m[0][1],m[0][2]], cross([m[1][0],m[1][1],m[1][2]], [m[2][0],m[2][1],m[2][2]])) }
fn normal(m: Matrix, n: [f32;3]) -> [f32;3] {
    let a=[m[0][0],m[0][1],m[0][2]]; let b=[m[1][0],m[1][1],m[1][2]]; let c=[m[2][0],m[2][1],m[2][2]];
    let co=[cross(b,c),cross(c,a),cross(a,b)]; let d=determinant(m);
    assert!(d.abs()>1e-15);
    unit(core::array::from_fn(|r| (co[0][r]*n[0]+co[1][r]*n[1]+co[2][r]*n[2])/d))
}
fn bounds(positions: &[[f32;3]]) -> ([f32;3],[f32;3]) {
    assert!(!positions.is_empty());
    let min=core::array::from_fn(|c| positions.iter().map(|p|p[c]).fold(f32::INFINITY,f32::min));
    let max=core::array::from_fn(|c| positions.iter().map(|p|p[c]).fold(f32::NEG_INFINITY,f32::max));
    (min,max)
}

pub fn asset_paths(root: &Path) -> Vec<PathBuf> {
    fn visit(dir:&Path, found:&mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("tilepack directory") {
            let path=entry.unwrap().path();
            if path.is_dir() { visit(&path,found); }
            else if path.extension().is_some_and(|e|e=="glb") { found.push(path); }
        }
    }
    let mut paths=Vec::new(); visit(root,&mut paths); paths.sort();
    assert_eq!(paths.len(),COLS*ROWS,"the gallery catalog must contain all 24 assets"); paths
}

pub fn decode_png(bytes:&[u8]) -> (usize,usize,Vec<u8>) {
    let mut decoder=png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader=decoder.read_info().expect("embedded PNG header");
    assert_eq!(reader.info().bit_depth,png::BitDepth::Eight,"lossless atlas requires authored 8-bit images");
    let mut decoded=vec![0;reader.output_buffer_size().unwrap()];
    let info=reader.next_frame(&mut decoded).expect("embedded PNG pixels");
    let channels=match info.color_type { png::ColorType::Rgb=>3,png::ColorType::Rgba=>4,png::ColorType::Grayscale=>1,png::ColorType::GrayscaleAlpha=>2,_=>panic!("unexpanded PNG") };
    let mut rgba=Vec::with_capacity(info.width as usize*info.height as usize*4);
    for p in decoded[..info.buffer_size()].chunks_exact(channels) {
        match channels {1=>rgba.extend([p[0],p[0],p[0],255]),2=>rgba.extend([p[0],p[0],p[0],p[1]]),3=>rgba.extend([p[0],p[1],p[2],255]),4=>rgba.extend_from_slice(p),_=>unreachable!()}
    }
    (info.width as usize,info.height as usize,rgba)
}
fn encode_png(rgba:&[u8]) -> Vec<u8> {
    let mut out=Vec::new();
    {
        let mut encoder=png::Encoder::new(&mut out,ATLAS_WIDTH as u32,ATLAS_HEIGHT as u32);
        encoder.set_color(png::ColorType::Rgba); encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        encoder.write_header().unwrap().write_image_data(rgba).unwrap();
    }
    assert!(out.len()<=64*1024*1024,"atlas exceeds encoded texture admission"); out
}
fn blit_repeat(atlas:&mut [u8], tile:&[u8], slot:usize) {
    assert_eq!(tile.len(),TILE*TILE*4);
    let ox=(slot%COLS)*SLOT; let oy=(slot/COLS)*SLOT;
    for y in 0..SLOT {
        let sy=(y+TILE-GUTTER)%TILE;
        for x in 0..SLOT {
            let sx=(x+TILE-GUTTER)%TILE;
            let dst=((oy+y)*ATLAS_WIDTH+ox+x)*4; let src=(sy*TILE+sx)*4;
            atlas[dst..dst+4].copy_from_slice(&tile[src..src+4]);
        }
    }
}
fn atlas_uv(uv:[f32;2], slot:usize)->[f32;2] {
    assert!(uv.iter().all(|v|v.is_finite() && (0.0..=1.0).contains(v)),"atlas requires source UV0 within [0,1]");
    [((slot%COLS)*SLOT+GUTTER) as f32 / ATLAS_WIDTH as f32 + uv[0]*TILE as f32/ATLAS_WIDTH as f32,
     ((slot/COLS)*SLOT+GUTTER) as f32 / ATLAS_HEIGHT as f32 + uv[1]*TILE as f32/ATLAS_HEIGHT as f32]
}
fn image_bytes<'a>(texture:gltf::Texture<'_>, binary:&'a [u8])->&'a [u8] {
    let sampler=texture.sampler();
    assert_eq!(sampler.wrap_s(),gltf::texture::WrappingMode::Repeat);
    assert_eq!(sampler.wrap_t(),gltf::texture::WrappingMode::Repeat);
    assert_eq!(sampler.mag_filter(),Some(gltf::texture::MagFilter::Linear));
    assert!(matches!(sampler.min_filter(),Some(gltf::texture::MinFilter::LinearMipmapNearest | gltf::texture::MinFilter::LinearMipmapLinear)));
    match texture.source().source() {
        gltf::image::Source::View{view,mime_type} => { assert_eq!(mime_type,"image/png"); assert_eq!(view.buffer().index(),0); &binary[view.offset()..view.offset()+view.length()] }
        _=>panic!("tilepack images must be embedded PNG buffer views")
    }
}

#[derive(Default)]
struct Geometry { positions:Vec<[f32;3]>,normals:Vec<[f32;3]>,uvs:Vec<[f32;2]>,tangents:Vec<[f32;4]>,indices:Vec<u32>,nodes:Vec<Value>,source_corners:Vec<([f32;3],[f32;3],[f32;2])> }
fn visit_node(node:gltf::Node<'_>, parent:Matrix, binary:&[u8], geo:&mut Geometry, generated:&mut usize) {
    let world=multiply(parent,node.transform().matrix());
    // No winding rewrite: this catalog has positive-determinant source nodes.
    assert!(determinant(world)>0.0,"mirrored source nodes require an explicit winding policy");
    if let Some(mesh)=node.mesh() {
        for prim in mesh.primitives() {
            assert_eq!(prim.mode(),gltf::mesh::Mode::Triangles,"retain original triangle lists");
            assert_eq!(prim.material().index(),Some(0),"catalog has one material per asset");
            let reader=prim.reader(|buf|{assert_eq!(buf.index(),0);Some(binary)});
            let mut p:Vec<_>=reader.read_positions().expect("POSITION").collect();
            let mut n:Vec<_>=reader.read_normals().expect("NORMAL").collect();
            let mut uv:Vec<_>=reader.read_tex_coords(0).expect("TEXCOORD_0").into_f32().collect();
            let mut inds:Vec<u32>=reader.read_indices().expect("indexed source").into_u32().collect();
            assert_eq!(p.len(),n.len());assert_eq!(p.len(),uv.len());assert!(inds.len().is_multiple_of(3));
            for &idx in &inds { let i=idx as usize; geo.source_corners.push((point(world,p[i]),normal(world,n[i]),uv[i])); }
            let t=if let Some(t)=reader.read_tangents(){t.collect::<Vec<_>>()}else{*generated+=1;generate_tangents(&mut p,&mut n,&mut uv,&mut inds)};
            assert_eq!(p.len(),t.len());
            let first_vertex=geo.positions.len() as u32; let first_index=geo.indices.len();
            for (((p,n),uv),t) in p.into_iter().zip(n).zip(uv).zip(t) {
                assert!(p.iter().chain(n.iter()).chain(uv.iter()).chain(t.iter()).all(|v|v.is_finite()));
                assert!(t[3]==1.0||t[3]==-1.0);
                let wn=normal(world,n); let wt=vector(world,[t[0],t[1],t[2]]);
                let wt=unit(core::array::from_fn(|c|wt[c]-wn[c]*dot(wn,wt)));
                geo.positions.push(point(world,p));geo.normals.push(wn);geo.uvs.push(uv);geo.tangents.push([wt[0],wt[1],wt[2],t[3]*determinant(world).signum()]);
            }
            geo.indices.extend(inds.into_iter().map(|i|i+first_vertex));
            geo.nodes.push(json!({"node":node.index(),"mesh":mesh.index(),"primitive":prim.index(),"material":prim.material().index(),"world_matrix":world,"first_index":first_index,"index_count":geo.indices.len()-first_index}));
        }
    }
    for child in node.children() {visit_node(child,world,binary,geo,generated);}
}

pub fn prepare(sources:&[(String,u64,Vec<u8>)])->PreparedScene {
    assert_eq!(sources.len(),COLS*ROWS);
    let mut atlases:[Vec<u8>;3]=core::array::from_fn(|_|vec![0;ATLAS_WIDTH*ATLAS_HEIGHT*4]);
    let mut vertices=Vec::new();let mut indices=Vec::new();let mut all_positions=Vec::new();let mut assets=Vec::new();let mut source_triangles=0;let mut generated_count=0;
    for (slot,(name,revision,source)) in sources.iter().enumerate() {
        let parsed=gltf::Gltf::from_slice(source).expect("reload admitted GLB");
        assert_eq!(parsed.meshes().count(),1);assert_eq!(parsed.materials().count(),1);assert_eq!(parsed.buffers().count(),1);
        assert_eq!(parsed.animations().count(),0);assert_eq!(parsed.skins().count(),0);assert_eq!(parsed.extensions_required().count(),0);
        let binary=parsed.blob.as_deref().expect("GLB BIN");
        let material=parsed.materials().next().unwrap();let pbr=material.pbr_metallic_roughness();
        assert_eq!(material.alpha_mode(),gltf::material::AlphaMode::Opaque);assert!(!material.double_sided());
        assert_eq!(pbr.base_color_factor(),[1.0;4]);assert_eq!(pbr.metallic_factor(),1.0);assert_eq!(pbr.roughness_factor(),1.0);
        assert_eq!(material.emissive_factor(),[0.0;3]);assert!(material.emissive_texture().is_none());
        let base=pbr.base_color_texture().expect("base map");let mr=pbr.metallic_roughness_texture().expect("ORM");
        let ao=material.occlusion_texture().expect("AO");let nm=material.normal_texture().expect("normal");
        assert_eq!(base.tex_coord(),0);assert_eq!(mr.tex_coord(),0);assert_eq!(ao.tex_coord(),0);assert_eq!(nm.tex_coord(),0);
        assert_eq!(ao.strength(),1.0);assert_eq!(nm.scale(),1.0);assert_eq!(mr.texture().source().index(),ao.texture().source().index());
        let mut bindings=Vec::new();
        for (map,texture) in [base.texture(),mr.texture(),nm.texture()].into_iter().enumerate() {
            let bytes=image_bytes(texture.clone(),binary);let(w,h,rgba)=decode_png(bytes);assert_eq!((w,h),(TILE,TILE));
            blit_repeat(&mut atlases[map],&rgba,slot);
            bindings.push(json!({"map":["base_color","ORM","normal"][map],"texture":texture.index(),"image":texture.source().index(),"encoded_sha256":sha(bytes),"rgba_sha256":sha(&rgba),"source_min_filter":texture.sampler().min_filter().map(|v|format!("{v:?}")),"source_wrap":"REPEAT"}));
        }
        let mut geo=Geometry::default();let before_generated=generated_count;
        for node in parsed.default_scene().expect("authored default scene").nodes() {visit_node(node,identity(),binary,&mut geo,&mut generated_count);}
        assert_eq!(geo.indices.len(),geo.source_corners.len());
        // Tangent seam splitting may change indices but never source corner data/order.
        for (&idx,original) in geo.indices.iter().zip(&geo.source_corners) {let i=idx as usize;assert_eq!((geo.positions[i],geo.normals[i],geo.uvs[i]),*original);}
        let(min,max)=bounds(&geo.positions);let ext=core::array::from_fn::<_,3,_>(|c|max[c]-min[c]);
        let thin=(0..3).min_by(|&a,&b|ext[a].total_cmp(&ext[b])).unwrap();
        let mut gallery=identity();
        if thin==1 { gallery[1]=[0.0,0.0,1.0,0.0];gallery[2]=[0.0,-1.0,0.0,0.0]; } else { assert_eq!(thin,2,"catalog display faces must lie in XY or XZ"); }
        let oriented:Vec<_>=geo.positions.iter().map(|&p|point(gallery,p)).collect();let(omin,omax)=bounds(&oriented);
        gallery[3]=[(slot%COLS)as f32*SPACING-(COLS-1)as f32*SPACING/2.0-(omin[0]+omax[0])/2.0,
                    (ROWS-1)as f32*SPACING/2.0-(slot/COLS)as f32*SPACING-(omin[1]+omax[1])/2.0,
                    -(omin[2]+omax[2])/2.0,1.0];
        let first_vertex=vertices.len()/48;let first_index=indices.len()/4;
        for i in 0..geo.positions.len() {
            let p=point(gallery,geo.positions[i]);let n=normal(gallery,geo.normals[i]);let t=geo.tangents[i];let tv=unit(vector(gallery,[t[0],t[1],t[2]]));let uv=atlas_uv(geo.uvs[i],slot);
            for v in p.into_iter().chain(n).chain(uv).chain([tv[0],tv[1],tv[2],t[3]]) {vertices.extend_from_slice(&v.to_le_bytes());}
            all_positions.push(p);
        }
        for i in &geo.indices {indices.extend_from_slice(&(i+first_vertex as u32).to_le_bytes());}
        source_triangles+=geo.indices.len()/3;
        assets.push(json!({"asset":name,"source_revision":revision,"source_sha256":sha(source),"source_bytes":source.len(),"default_scene":parsed.default_scene().unwrap().index(),"source_world_bounds":[min,max],"source_nodes":geo.nodes,"gallery_matrix":gallery,"atlas_slot":slot,"atlas_origin":[slot%COLS*SLOT+GUTTER,slot/COLS*SLOT+GUTTER],"first_vertex":first_vertex,"vertex_count":geo.positions.len(),"first_index":first_index,"index_count":geo.indices.len(),"source_triangle_count":geo.indices.len()/3,"generated_tangents":generated_count-before_generated,"texture_bindings":bindings}));
    }
    assert_eq!(source_triangles,604,"all source triangles must be retained");assert_eq!(generated_count,3);
    let(min,max)=bounds(&all_positions);
    let encoded=atlases.map(|rgba|encode_png(&rgba));
    let metadata=json!({"schema":1,"policy":"Original glTF TRIANGLES; source default scenes and node transforms, then explicit gallery placement; no quad reconstruction, no decimation, no image resampling.","vertex_stride":48,"vertex_count":vertices.len()/48,"index_count":indices.len()/4,"triangle_count":source_triangles,"native_quad_count":0,"asset_count":sources.len(),"bounds_min":min,"bounds_max":max,"atlas":{"width":ATLAS_WIDTH,"height":ATLAS_HEIGHT,"tile_size":TILE,"gutter":GUTTER,"gutter_mode":"REPEAT","columns":COLS,"rows":ROWS,"filtering":"Runtime base-level bilinear only; authored mipmap minification is retained as metadata but mip chains are not generated."},"vertices_sha256":sha(&vertices),"indices_sha256":sha(&indices),"atlas_sha256":encoded.iter().map(|b|sha(b)).collect::<Vec<_>>(),"assets":assets});
    PreparedScene{vertices,indices,atlases:encoded,metadata}
}

pub fn build(root:&Path,out:&Path) {
    let paths=asset_paths(root);
    for path in &paths {println!("cargo:rerun-if-changed={}",path.display());}
    let db_path=out.join("tilepack.picasso.redb");
    if db_path.exists(){fs::remove_file(&db_path).expect("replace build-only database");}
    let store=Store::create(db_path.to_str().unwrap()).unwrap();
    let mut revisions=Vec::new();
    for path in &paths {
        let name=path.strip_prefix(root).unwrap().to_str().unwrap().to_owned();
        let metadata=trueos_picasso::inspect_glb_file(path).expect("metadata admission");
        assert_eq!(metadata.descriptor["asset"]["version"],"2.0");
        revisions.push((name.clone(),store.import_file(&name,path).expect("Picasso GLB import"),sha(&fs::read(path).unwrap())));
    }
    drop(store);
    let store=Store::open(db_path.to_str().unwrap()).expect("reopen imported sources");
    let sources:Vec<_>=revisions.iter().map(|(name,revision,hash)|{
        let record=store.revision(*revision).unwrap();let bytes=store.blob(&record.source_blob).unwrap();assert_eq!(sha(&bytes),*hash);
        (name.clone(),*revision,bytes)
    }).collect();
    let prepared=prepare(&sources);
    let metadata=serde_json::to_vec_pretty(&prepared.metadata).unwrap();
    let provenance=String::from_utf8(metadata.clone()).unwrap();let revision=revisions[0].1;
    for (name,bytes) in [("tile_scene.vertices",prepared.vertices.as_slice()),("tile_scene.indices",prepared.indices.as_slice()),("base_color.png",prepared.atlases[0].as_slice()),("orm.png",prepared.atlases[1].as_slice()),("normal.png",prepared.atlases[2].as_slice()),("tile_scene.json",metadata.as_slice())] {
        store.put_derived(revision,name,&provenance,bytes).expect("persist prepared atlas/geometry");
    }
    drop(prepared);drop(sources);drop(store);
    let store=Store::open(db_path.to_str().unwrap()).expect("reopen derived scene");
    for name in ["tile_scene.vertices","tile_scene.indices","base_color.png","orm.png","normal.png","tile_scene.json"] {
        let artifact=store.derived(revision,name).unwrap().expect("reloaded derived artifact");assert_eq!(artifact.provenance,provenance);
        fs::write(out.join(name),artifact.bytes).unwrap();
    }
    let meta:Value=serde_json::from_slice(&fs::read(out.join("tile_scene.json")).unwrap()).unwrap();
    assert_eq!(sha(&fs::read(out.join("tile_scene.vertices")).unwrap()),meta["vertices_sha256"].as_str().unwrap());
    assert_eq!(sha(&fs::read(out.join("tile_scene.indices")).unwrap()),meta["indices_sha256"].as_str().unwrap());
    for (i,name) in ["base_color.png","orm.png","normal.png"].into_iter().enumerate(){assert_eq!(sha(&fs::read(out.join(name)).unwrap()),meta["atlas_sha256"][i].as_str().unwrap());}
    let generated=format!(r#"pub const SCENE_VERTICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tile_scene.vertices"));
pub const SCENE_INDICES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/tile_scene.indices"));
pub const BASE_COLOR_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/base_color.png"));
pub const ORM_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/orm.png"));
pub const NORMAL_PNG: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/normal.png"));
pub const SCENE_METADATA: &str = include_str!(concat!(env!("OUT_DIR"), "/tile_scene.json"));
pub const SCENE_VERTEX_COUNT: u32 = {};
pub const SCENE_INDEX_COUNT: u32 = {};
pub const SCENE_ASSET_COUNT: u32 = 24;
pub const SCENE_BOUNDS_MIN: [f32; 3] = {:?};
pub const SCENE_BOUNDS_MAX: [f32; 3] = {:?};
pub const ATLAS_WIDTH: u32 = {};
pub const ATLAS_HEIGHT: u32 = {};
"#,meta["vertex_count"],meta["index_count"],serde_json::from_value::<[f32;3]>(meta["bounds_min"].clone()).unwrap(),serde_json::from_value::<[f32;3]>(meta["bounds_max"].clone()).unwrap(),ATLAS_WIDTH,ATLAS_HEIGHT);
    fs::write(out.join("tile_scene_meta.rs"),generated).unwrap();
}

fn generate_tangents(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut [u32],
) -> Vec<[f32; 4]> {
    struct Geometry<'a> {
        positions: &'a [[f32; 3]],
        normals: &'a [[f32; 3]],
        uvs: &'a [[f32; 2]],
        indices: &'a [u32],
        corners: &'a mut [[f32; 4]],
    }
    impl bevy_mikktspace::Geometry for Geometry<'_> {
        fn num_faces(&self) -> usize {
            self.indices.len() / 3
        }
        fn num_vertices_of_face(&self, _face: usize) -> usize {
            3
        }
        fn position(&self, face: usize, vertex: usize) -> [f32; 3] {
            self.positions[self.indices[face * 3 + vertex] as usize]
        }
        fn normal(&self, face: usize, vertex: usize) -> [f32; 3] {
            self.normals[self.indices[face * 3 + vertex] as usize]
        }
        fn tex_coord(&self, face: usize, vertex: usize) -> [f32; 2] {
            self.uvs[self.indices[face * 3 + vertex] as usize]
        }
        fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vertex: usize) {
            self.corners[face * 3 + vertex] = tangent;
        }
    }
    assert!(indices.len().is_multiple_of(3));
    let mut corners = vec![[0.0; 4]; indices.len()];
    assert!(
        bevy_mikktspace::generate_tangents(&mut Geometry {
            positions,
            normals,
            uvs,
            indices,
            corners: &mut corners,
        }),
        "MikkTSpace tangent generation failed"
    );
    let mut tangents = vec![[0.0; 4]; positions.len()];
    let mut initialized = vec![false; positions.len()];
    let mut variants = BTreeMap::<(u32, [u32; 4]), u32>::new();
    for (index, mut tangent) in indices.iter_mut().zip(corners) {
        assert!(tangent.into_iter().all(f32::is_finite));
        assert!(tangent[3] == 1.0 || tangent[3] == -1.0);
        let source_index = *index as usize;
        // A collapsed UV triangle has no defined tangent direction. Mikk can
        // return zero or its default axis there (the helmet contains both).
        // Preserve every valid Mikk frame, and give only the undefined case
        // a unit direction perpendicular to the authored normal so fragment
        // normalization cannot introduce NaNs.
        let length_squared = tangent[..3].iter().map(|v| v * v).sum::<f32>();
        let normal = normals[source_index];
        let normal_length = normal.into_iter().map(|v| v * v).sum::<f32>();
        assert!(normal_length.is_finite() && normal_length > 1e-20);
        let dot = normal
            .into_iter()
            .zip(tangent)
            .map(|(n, t)| n * t)
            .sum::<f32>();
        if length_squared <= 1e-20 || dot.abs() > 1e-4 || (length_squared - 1.0).abs() > 1e-4 {
            let mut direction = core::array::from_fn::<_, 3, _>(|component| {
                tangent[component] - normal[component] * dot / normal_length
            });
            if direction.into_iter().map(|v| v * v).sum::<f32>() <= 1e-20 {
                let axis =
                    if normal[0].abs() <= normal[1].abs() && normal[0].abs() <= normal[2].abs() {
                        [1.0, 0.0, 0.0]
                    } else if normal[1].abs() <= normal[2].abs() {
                        [0.0, 1.0, 0.0]
                    } else {
                        [0.0, 0.0, 1.0]
                    };
                direction = [
                    normal[1] * axis[2] - normal[2] * axis[1],
                    normal[2] * axis[0] - normal[0] * axis[2],
                    normal[0] * axis[1] - normal[1] * axis[0],
                ];
            }
            let inverse_length = direction
                .into_iter()
                .map(|v| v * v)
                .sum::<f32>()
                .sqrt()
                .recip();
            for component in 0..3 {
                tangent[component] = direction[component] * inverse_length;
            }
        }
        let key = (*index, tangent.map(f32::to_bits));
        let vertex = if let Some(&vertex) = variants.get(&key) {
            vertex
        } else {
            let vertex = if !initialized[source_index] {
                initialized[source_index] = true;
                tangents[source_index] = tangent;
                *index
            } else {
                let vertex = u32::try_from(positions.len()).expect("tangent vertex count");
                positions.push(positions[source_index]);
                normals.push(normals[source_index]);
                uvs.push(uvs[source_index]);
                tangents.push(tangent);
                vertex
            };
            variants.insert(key, vertex);
            vertex
        };
        *index = vertex;
    }
    tangents
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root()->PathBuf { PathBuf::from(env!("TILEPACK_ROOT")) }
    #[test]
    fn full_catalog_survives_picasso_reopen_and_lossless_atlas_handoff() {
        let out=std::env::temp_dir().join(format!("quadtexture-scene-test-{}",std::process::id()));
        if out.exists(){fs::remove_dir_all(&out).unwrap();}fs::create_dir_all(&out).unwrap();
        build(&root(),&out);
        let metadata:Value=serde_json::from_slice(&fs::read(out.join("tile_scene.json")).unwrap()).unwrap();
        assert_eq!(metadata["asset_count"],24);assert_eq!(metadata["triangle_count"],604);assert_eq!(metadata["index_count"],1812);assert_eq!(metadata["native_quad_count"],0);
        let vertices=fs::read(out.join("tile_scene.vertices")).unwrap();let indices=fs::read(out.join("tile_scene.indices")).unwrap();
        assert_eq!(vertices.len(),metadata["vertex_count"].as_u64().unwrap() as usize*48);assert_eq!(indices.len(),1812*4);
        assert!(indices.chunks_exact(4).all(|i|u32::from_le_bytes(i.try_into().unwrap())<(vertices.len()/48)as u32));
        let store=Store::open(out.join("tilepack.picasso.redb").to_str().unwrap()).unwrap();
        let paths=asset_paths(&root());
        // Reopen the emitted PNGs and compare every interior source texel, not just a hash of encoded output.
        for (map,name) in ["base_color.png","orm.png","normal.png"].into_iter().enumerate() {
            let(w,h,atlas)=decode_png(&fs::read(out.join(name)).unwrap());assert_eq!((w,h),(ATLAS_WIDTH,ATLAS_HEIGHT));
            for (slot,path) in paths.iter().enumerate() {
                let revision=metadata["assets"][slot]["source_revision"].as_u64().unwrap();
                let source=store.blob(&store.revision(revision).unwrap().source_blob).unwrap();assert_eq!(source,fs::read(path).unwrap());
                let parsed=gltf::Gltf::from_slice(&source).unwrap();let mat=parsed.materials().next().unwrap();
                let texture=match map {0=>mat.pbr_metallic_roughness().base_color_texture().unwrap().texture(),1=>mat.pbr_metallic_roughness().metallic_roughness_texture().unwrap().texture(),_=>mat.normal_texture().unwrap().texture()};
                assert_eq!(texture.index()as u64,metadata["assets"][slot]["texture_bindings"][map]["texture"].as_u64().unwrap());
                let(_,_,tile)=decode_png(image_bytes(texture,parsed.blob.as_deref().unwrap()));
                let ox=slot%COLS*SLOT+GUTTER;let oy=slot/COLS*SLOT+GUTTER;
                for y in 0..TILE { let a=((oy+y)*ATLAS_WIDTH+ox)*4;assert_eq!(&atlas[a..a+TILE*4],&tile[y*TILE*4..(y+1)*TILE*4]); }
                for (x,y) in [(0,0),(1,1),(SLOT-1,0),(0,SLOT-1),(SLOT-1,SLOT-1)] {
                    let a=((slot/COLS*SLOT+y)*ATLAS_WIDTH+slot%COLS*SLOT+x)*4;
                    let t=(((y+TILE-GUTTER)%TILE)*TILE+(x+TILE-GUTTER)%TILE)*4;
                    assert_eq!(&atlas[a..a+4],&tile[t..t+4]);
                }
            }
        }
        let preserved=out.join("test-artifacts.json");fs::write(&preserved,serde_json::to_vec_pretty(&metadata).unwrap()).unwrap();
        println!("Full scene artifacts verified at {}",out.display());
        // Keep the output for the independent kernel PNG decoder capacity audit.
    }
    #[test]
    fn inverse_transpose_handles_nonuniform_scale_and_rotation() {
        let mut m=identity();m[0]=[0.0,2.0,0.0,0.0];m[1]=[-3.0,0.0,0.0,0.0];m[2]=[0.0,0.0,4.0,0.0];m[3]=[7.0,8.0,9.0,1.0];
        let n=unit([1.0,1.0,0.0]);let transformed=normal(m,n);let edge=vector(m,[1.0,-1.0,0.0]);
        assert!(dot(transformed,edge).abs()<1e-6);assert_eq!(point(m,[0.0;3]),[7.0,8.0,9.0]);
    }
    #[test]
    fn atlas_uv_preserves_source_endpoints_inside_repeat_gutters() {
        let uv=atlas_uv([0.0,1.0],23);
        assert!((uv[0]*ATLAS_WIDTH as f32-(5*SLOT+GUTTER)as f32).abs()<0.001);
        assert!((uv[1]*ATLAS_HEIGHT as f32-(3*SLOT+GUTTER+TILE)as f32).abs()<0.001);
    }
    #[test]
    #[should_panic(expected="atlas requires source UV0")]
    fn atlas_rejects_out_of_range_source_uvs_instead_of_clamping() {atlas_uv([1.001,0.0],0);}
    #[test]
    fn mikktspace_splits_mirrored_seams_without_changing_corner_data() {
        let mut p=vec![[0.0,0.0,0.0],[1.0,0.0,0.0],[0.0,1.0,0.0],[-1.0,0.0,0.0]];
        let mut n=vec![[0.0,0.0,1.0];4];let mut uv=vec![[0.0,0.0],[1.0,0.0],[0.0,1.0],[1.0,0.0]];let mut inds=vec![0,1,2,0,2,3];
        let before:Vec<_>=inds.iter().map(|&i|(p[i as usize],n[i as usize],uv[i as usize])).collect();let t=generate_tangents(&mut p,&mut n,&mut uv,&mut inds);
        assert_eq!(t[inds[0]as usize][3],-t[inds[3]as usize][3]);
        for(&i,corner)in inds.iter().zip(before){assert_eq!((p[i as usize],n[i as usize],uv[i as usize]),corner);}
    }
}
