pub mod cache;
pub mod pack;
pub mod types;
pub mod unpack;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::pack::pack_registry::PackRegistry;
use cache::CacheStore;
use cache::category::CategoryType;
use cache::dbrow::DbRowType;
use cache::dbtable::{DbTableIndex, DbTableType};
use cache::r#enum::EnumType;
use cache::flo::FloType;
use cache::font::FontTypeProvider;
use cache::hunt::HuntType;
use cache::idk::IdkType;
use cache::r#if::IfTypeProvider;
use cache::inv::InvType;
use cache::loc::LocType;
use cache::mesanim::MesAnimType;
use cache::midi::MidiProvider;
use cache::npc::NpcType;
use cache::obj::{ObjContext, ObjType};
use cache::param::ParamType;
use cache::provider::TypeProvider;
use cache::seq::SeqType;
use cache::spotanim::SpotAnimType;
use cache::r#struct::StructType;
use cache::varn::VarnType;
use cache::varp::VarPlayerType;
use cache::vars::VarsType;
use cache::wordenc::WordEncProvider;
use pack::other;
use rs_io::crc;
use rs_io::jag::JagFile;
use tracing::info;

pub use types::ParamValue;

fn insert_jag(
    crcs: &mut HashMap<&'static str, i32>,
    packs: &mut HashMap<&'static str, Arc<[u8]>>,
    name: &'static str,
    data: Vec<u8>,
    expected_crc: i32,
    verify: bool,
) {
    if data.is_empty() {
        panic!("Jag file is empty");
    }
    if verify {
        let actual = crc::getcrc(&data, 0, data.len());
        if actual != expected_crc {
            panic!("CRC mismatch ['{name}']: Got: {actual}, Expected: {expected_crc}");
        }
        crcs.insert(name, expected_crc);
    } else {
        crcs.insert(name, crc::getcrc(&data, 0, data.len()));
    }
    packs.insert(name, Arc::from(data));
}

fn unwrap_thread<T>(name: &str, result: std::thread::Result<T>) -> T {
    result.unwrap_or_else(|e| {
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        panic!("{name} thread panicked: {msg}");
    })
}

pub fn pack_all(
    source: &Path,
    pack: &Path,
    verify: bool,
) -> anyhow::Result<(Box<CacheStore>, cache::script::ScriptProvider)> {
    info!("Packing assets...");
    info!("  source: {}", source.display());
    info!("  pack:   {}", pack.display());
    if verify {
        info!("  mode:   VERIFY (strict)");
    }

    let registry = PackRegistry::load(pack)?;

    // Run independent packing tasks in parallel using scoped threads.
    // Script compilation runs alongside asset packing - it's only needed
    // at the end for ScriptProvider construction.
    let (
        script_dat,
        script_idx,
        mut assets,
        media,
        textures,
        title,
        models,
        sounds,
        wordenc,
        jingles,
        songs,
        mapsquares,
        mapcrcs,
        multimap,
        freemap,
    ) = std::thread::scope(|s| {
        let h_scripts = s.spawn(|| {
            info!("Compiling RuneScript sources...");
            match runec::compile_memory(source, Some(pack), false) {
                Ok(bytes) => bytes,
                Err(e) => panic!("RuneScript compilation failed: {}", e),
            }
        });
        let h_assets = s.spawn(|| pack::pack::pack_assets(&registry, source, verify));
        let h_media = s.spawn(|| pack::media::pack_media_jag(source));
        let h_textures = s.spawn(|| pack::texture::pack_textures_jag(&registry, source));
        let h_title = s.spawn(|| pack::title::pack_title_jag(source));
        let h_models = s.spawn(|| pack::model::pack_models(&registry, source, pack));
        let h_sounds = s.spawn(|| pack::sound::pack_sounds(&registry, source, pack));
        let h_wordenc = s.spawn(|| pack::wordenc::pack_wordenc(source));
        let h_jingles = s.spawn(|| other::jingle::pack_jingles(source));
        let h_songs = s.spawn(|| other::song::pack_songs(source));
        let h_maps = s.spawn(|| other::map::pack_maps(source));

        let (script_dat, script_idx) = unwrap_thread("scripts", h_scripts.join());
        let assets = unwrap_thread("assets", h_assets.join()).unwrap();
        let media = unwrap_thread("media", h_media.join());
        let textures = unwrap_thread("textures", h_textures.join());
        let title = unwrap_thread("title", h_title.join());
        let models = unwrap_thread("models", h_models.join());
        let sounds = unwrap_thread("sounds", h_sounds.join());
        let wordenc = unwrap_thread("wordenc", h_wordenc.join());
        let jingles = unwrap_thread("jingles", h_jingles.join());
        let songs = unwrap_thread("songs", h_songs.join());
        let (mapsquares, mapcrcs, multimap, freemap) = unwrap_thread("maps", h_maps.join());

        (
            script_dat, script_idx, assets, media, textures, title, models, sounds, wordenc,
            jingles, songs, mapsquares, mapcrcs, multimap, freemap,
        )
    });

    let mut crcs = HashMap::new();
    let mut jags = HashMap::new();

    info!("Packing config...");
    insert_jag(
        &mut crcs,
        &mut jags,
        "config",
        assemble_config_jag(&mut assets),
        511217062,
        verify,
    );

    info!("Packing interface...");
    insert_jag(
        &mut crcs,
        &mut jags,
        "interface",
        assemble_interface_jag(&mut assets),
        1614084464,
        verify,
    );

    insert_jag(&mut crcs, &mut jags, "media", media, -343404987, verify);
    insert_jag(
        &mut crcs, &mut jags, "textures", textures, 1703545114, verify,
    );
    insert_jag(&mut crcs, &mut jags, "title", title, -430779560, verify);
    insert_jag(&mut crcs, &mut jags, "models", models, -2000991154, verify);
    insert_jag(&mut crcs, &mut jags, "sounds", sounds, -1532605973, verify);
    insert_jag(&mut crcs, &mut jags, "wordenc", wordenc, 1570981179, verify);

    info!("Pack complete.");

    // Build CRC table
    let mut crctable = [0; 9];
    for (i, &key) in [
        "title",
        "config",
        "interface",
        "media",
        "models",
        "textures",
        "wordenc",
        "sounds",
    ]
    .iter()
    .enumerate()
    {
        if let Some(&data) = crcs.get(key) {
            crctable[i + 1] = data;
        }
    }
    let crctable_bytes: Arc<[u8]> = Arc::from(
        crctable
            .iter()
            .flat_map(|n| n.to_be_bytes())
            .collect::<Vec<u8>>(),
    );

    // Build TypeProviders from server-side packed data
    let objs = build_type_provider::<ObjType>(&assets, "obj", ObjContext { members: true });
    let invs = build_type_provider::<InvType>(&assets, "inv", ());
    let varps = build_type_provider::<VarPlayerType>(&assets, "varp", ());
    let dbrows = build_type_provider::<DbRowType>(&assets, "dbrow", ());
    let dbtables = build_type_provider::<DbTableType>(&assets, "dbtable", ());
    let db_index = DbTableIndex::build(&dbtables, &dbrows);
    let enums = build_type_provider::<EnumType>(&assets, "enum", ());
    let flos = build_type_provider::<FloType>(&assets, "flo", ());
    let hunts = build_type_provider::<HuntType>(&assets, "hunt", ());
    let idks = build_type_provider::<IdkType>(&assets, "idk", ());
    let locs = build_type_provider::<LocType>(&assets, "loc", ());
    let mesanims = build_type_provider::<MesAnimType>(&assets, "mesanim", ());
    let npcs = build_type_provider::<NpcType>(&assets, "npc", ());
    let params = build_type_provider::<ParamType>(&assets, "param", ());
    let seq_frames = cache::seq_frame::SeqFrameProvider::from_jag(
        jags.get("models")
            .expect("Missing models JAG for seq frames"),
    );
    let seqs = build_type_provider::<SeqType>(&assets, "seq", seq_frames.delays.clone());
    let spotanims = build_type_provider::<SpotAnimType>(&assets, "spotanim", ());
    let structs = build_type_provider::<StructType>(&assets, "struct", ());
    let varns = build_type_provider::<VarnType>(&assets, "varn", ());
    let varss = build_type_provider::<VarsType>(&assets, "vars", ());
    let categories = build_type_provider::<CategoryType>(&assets, "category", ());
    let interfaces = {
        let packed_file = assets
            .get("interface")
            .expect("Missing packed data for interface");
        IfTypeProvider::from_bytes(&packed_file.server.dat)
    };
    let fonts = {
        let title_jag = jags.get("title").expect("Missing title JAG for fonts");
        FontTypeProvider::from_jag(title_jag)
    };
    let wordenc = {
        let wordenc_jag = jags.get("wordenc").expect("Missing wordenc JAG");
        WordEncProvider::from_jag(wordenc_jag)
    };
    let midi_songs = MidiProvider::from_compressed(songs);
    let midi_jingles = MidiProvider::from_compressed(jingles);

    info!(
        "TypeProviders: objs={} invs={} varps={} dbrows={} dbtables={} enums={} flos={} hunts={} idks={} locs={} mesanims={} npcs={} params={} seqs={} spotanims={} structs={} varns={} varss={} categories={} interfaces={} fonts={} wordenc=bad:{}/frag:{}/tld:{}/dom:{} songs={} jingles={}",
        objs.count(),
        invs.count(),
        varps.count(),
        dbrows.count(),
        dbtables.count(),
        enums.count(),
        flos.count(),
        hunts.count(),
        idks.count(),
        locs.count(),
        mesanims.count(),
        npcs.count(),
        params.count(),
        seqs.count(),
        spotanims.count(),
        structs.count(),
        varns.count(),
        varss.count(),
        categories.count(),
        interfaces.count(),
        fonts.count(),
        wordenc.bads.len(),
        wordenc.fragments.len(),
        wordenc.tlds.len(),
        wordenc.domains.len(),
        midi_songs.count(),
        midi_jingles.count(),
    );

    // Script bytecode -> ScriptProvider
    let scripts = cache::script::ScriptProvider::from_bytes(&script_dat, &script_idx);
    info!("Scripts: {} loaded", scripts.count());

    let store = Box::new(CacheStore {
        crctable,
        crctable_bytes,
        crcs,
        jags,
        mapsquares,
        mapcrcs,
        objs,
        invs,
        varps,
        dbrows,
        dbtables,
        db_index,
        enums,
        flos,
        hunts,
        idks,
        locs,
        mesanims,
        npcs,
        params,
        seq_frames,
        seqs,
        spotanims,
        structs,
        varns,
        varss,
        categories,
        interfaces,
        fonts,
        wordenc,
        songs: midi_songs,
        jingles: midi_jingles,
        static_assets: load_static_assets(),
        multimap,
        freemap,
    });

    info!("CacheStore built successfully");
    Ok((store, scripts))
}

fn load_static_assets() -> HashMap<Box<str>, Arc<[u8]>> {
    let dir = Path::new("public");
    let mut assets = HashMap::new();
    if !dir.exists() {
        return assets;
    }
    load_assets_recursive(dir, dir, &mut assets);
    info!("Loaded {} static assets into memory", assets.len());
    assets
}

fn load_assets_recursive(base: &Path, dir: &Path, assets: &mut HashMap<Box<str>, Arc<[u8]>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            load_assets_recursive(base, &path, assets);
        } else if let Ok(data) = std::fs::read(&path) {
            let key = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let key = format!("/{key}");
            assets.insert(Box::from(key), Arc::from(data));
        }
    }
}

fn build_type_provider<T: cache::provider::CacheType>(
    assets: &HashMap<String, pack::pack_registry::PackedFile>,
    name: &str,
    ctx: T::Context,
) -> TypeProvider<T> {
    let packed_file = assets
        .get(name)
        .unwrap_or_else(|| panic!("Missing packed data for {name}"));
    TypeProvider::from_bytes(&packed_file.server.dat, ctx)
}

fn assemble_config_jag(assets: &mut HashMap<String, pack::pack_registry::PackedFile>) -> Vec<u8> {
    let config_types = ["seq", "loc", "flo", "spotanim", "obj", "npc", "idk", "varp"];
    let mut jag = JagFile::new();
    let mut has_data = false;
    for name in config_types {
        if let Some(packed_file) = assets.get_mut(name)
            && let Some(client) = packed_file.client.take()
        {
            jag.write(&format!("{name}.dat"), client.dat);
            jag.write(&format!("{name}.idx"), client.idx);
            has_data = true;
        }
    }
    if has_data { jag.build() } else { Vec::new() }
}

fn assemble_interface_jag(
    assets: &mut HashMap<String, pack::pack_registry::PackedFile>,
) -> Vec<u8> {
    if let Some(packed_file) = assets.get_mut("interface")
        && let Some(client) = packed_file.client.take()
    {
        let mut jag = JagFile::new();
        jag.write("data", client.dat);
        return jag.build();
    }
    Vec::new()
}
