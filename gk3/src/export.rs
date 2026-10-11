//! `gk3 export`: write assets in formats other tools open.
//!
//! Bitmaps become PNG, lightmap sets a folder of PNGs, models and rooms OBJ
//! with an MTL file, and sounds 16-bit PCM WAV. Anything else is written as
//! it is stored.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use gk3_barn::Library;
use gk3_formats::bsp::Bsp;
use gk3_formats::model::Model;
use gk3_formats::{Kind, image, lightmap, png};

pub fn run(lib: &Library, patterns: &[String], out: &Path) -> Result<ExitCode, String> {
    if patterns.is_empty() {
        return Err("export needs at least one pattern; use '*' for everything".into());
    }
    let assets = crate::matching(lib, patterns);
    if assets.is_empty() {
        return Err("nothing matches".into());
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;

    let failures = Mutex::new(Vec::new());
    let written = AtomicUsize::new(0);
    crate::for_each_parallel(&assets, |(barn, entry)| {
        let result = barn
            .read(entry)
            .map_err(|e| e.to_string())
            .and_then(|bytes| export(&entry.name, &bytes, out));
        match result {
            Ok(()) => {
                written.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => failures
                .lock()
                .unwrap()
                .push(format!("{}: {e}", entry.name)),
        }
    });

    println!(
        "Exported {} assets to {}",
        written.into_inner(),
        out.display()
    );
    crate::report_failures(&failures.into_inner().unwrap())
}

fn export(name: &str, bytes: &[u8], out: &Path) -> Result<(), String> {
    let base = crate::safe_file_name(name);
    let stem = base
        .rsplit_once('.')
        .map_or(base.as_str(), |(s, _)| s)
        .to_string();
    let write = |file: String, data: &[u8]| {
        let path = out.join(&file);
        std::fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))
    };
    let fmt = |e: gk3_formats::Error| e.to_string();

    match Kind::of(bytes) {
        Kind::Gk3Bitmap | Kind::WindowsBitmap => {
            let image = image::decode(bytes).map_err(fmt)?;
            // Upper case, to match the names materials refer to.
            write(
                format!("{}.png", stem.to_ascii_uppercase()),
                &png::encode(&image),
            )
        }
        Kind::Lightmap => {
            let dir = out.join(format!("{stem}.lightmaps"));
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for (i, map) in lightmap::parse(bytes).map_err(fmt)?.iter().enumerate() {
                let path = dir.join(format!("{i:04}.png"));
                std::fs::write(&path, png::encode(map)).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Kind::Model => {
            let model = Model::parse(bytes).map_err(fmt)?;
            let (obj, mtl) = model_obj(&model, &stem);
            write(format!("{stem}.obj"), obj.as_bytes())?;
            write(format!("{stem}.mtl"), mtl.as_bytes())
        }
        Kind::Scene => {
            let bsp = Bsp::parse(bytes).map_err(fmt)?;
            let (obj, mtl) = scene_obj(&bsp, &stem);
            write(format!("{stem}.obj"), obj.as_bytes())?;
            write(format!("{stem}.mtl"), mtl.as_bytes())
        }
        Kind::Wave => {
            let pcm = gk3_media::decode_wave(bytes).map_err(|e| e.to_string())?;
            write(format!("{stem}.wav"), &pcm.to_wave())
        }
        _ => write(base, bytes),
    }
}

/// One material per texture, each pointing at the PNG `export` would write
/// for it, so a folder exported with `'*'` opens textured.
fn materials<'a>(textures: impl Iterator<Item = &'a str>) -> String {
    let mut seen: Vec<&str> = textures.collect();
    seen.sort_unstable_by_key(|t| t.to_ascii_uppercase());
    seen.dedup_by_key(|t| t.to_ascii_uppercase());
    let mut mtl = String::new();
    for t in seen {
        let _ = writeln!(
            mtl,
            "newmtl {t}\nKd 1 1 1\nmap_Kd {}.png\n",
            t.to_ascii_uppercase()
        );
    }
    mtl
}

fn model_obj(model: &Model, stem: &str) -> (String, String) {
    let mut obj = format!("# {stem}.MOD exported by gk3\nmtllib {stem}.mtl\n");
    let mut base = 1usize;
    for (m, mesh) in model.meshes.iter().enumerate() {
        let [i, j, k, origin] = mesh.transform;
        let _ = writeln!(obj, "o mesh{m}");
        for sub in &mesh.submeshes {
            for p in &sub.positions {
                let world: [f32; 3] =
                    std::array::from_fn(|a| i[a] * p[0] + j[a] * p[1] + k[a] * p[2] + origin[a]);
                let _ = writeln!(obj, "v {} {} {}", world[0], world[1], world[2]);
            }
            for uv in &sub.uvs {
                // OBJ puts the texture origin at the bottom left; GK3 at the top.
                let _ = writeln!(obj, "vt {} {}", uv[0], 1.0 - uv[1]);
            }
            let _ = writeln!(obj, "usemtl {}", sub.texture);
            for t in &sub.triangles {
                let [a, b, c] = t.map(|i| base + i as usize);
                let _ = writeln!(obj, "f {a}/{a} {b}/{b} {c}/{c}");
            }
            base += sub.positions.len();
        }
    }
    let mtl = materials(model.submeshes().map(|s| s.texture.as_str()));
    (obj, mtl)
}

fn scene_obj(bsp: &Bsp, stem: &str) -> (String, String) {
    let mut obj = format!("# {stem}.BSP exported by gk3\nmtllib {stem}.mtl\n");
    for v in &bsp.vertices {
        let _ = writeln!(obj, "v {} {} {}", v[0], v[1], v[2]);
    }
    for i in 0..bsp.vertices.len() {
        let uv = bsp.uv(i as u16);
        let _ = writeln!(obj, "vt {} {}", uv[0], 1.0 - uv[1]);
    }
    // Group polygons by the named object their surface belongs to.
    let mut by_object: Vec<Vec<usize>> = vec![Vec::new(); bsp.objects.len()];
    for (p, polygon) in bsp.polygons.iter().enumerate() {
        let object = bsp.surfaces[polygon.surface as usize].object as usize;
        by_object[object].push(p);
    }
    for (o, polygons) in by_object.iter().enumerate() {
        if polygons.is_empty() {
            continue;
        }
        let _ = writeln!(obj, "o {}", bsp.objects[o]);
        let mut current = None;
        for &p in polygons {
            let polygon = &bsp.polygons[p];
            let surface = polygon.surface;
            if current != Some(surface) {
                let _ = writeln!(obj, "usemtl {}", bsp.surfaces[surface as usize].texture);
                current = Some(surface);
            }
            for t in bsp.triangles(polygon) {
                let [a, b, c] = t.map(|i| i as usize + 1);
                let _ = writeln!(obj, "f {a}/{a} {b}/{b} {c}/{c}");
            }
        }
    }
    let mtl = materials(bsp.surfaces.iter().map(|s| s.texture.as_str()));
    (obj, mtl)
}
