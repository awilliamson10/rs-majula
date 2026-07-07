use once_cell::sync::OnceCell;
use std::path::Path;
use rs_engine::{Engine, TickStats, LoginRequest};
use rs_engine::{EtherInbound, EtherOutbound, DbRequest, DbResponse};
use rs_pack::cache::CacheStore;
use rs_pack::cache::script::ScriptProvider;
use tokio::sync::{mpsc::unbounded_channel, watch};

static CACHE: OnceCell<&'static CacheStore> = OnceCell::new();

/// `rs_pack::CONTENT_DIR` / `PACK_DIR` are relative paths (`content/274`,
/// `content/274/pack`) intended to be resolved against the workspace root.
/// `cargo run` for `rs-server` is conventionally invoked from the workspace
/// root, so the relative paths just work there. `cargo test` however sets
/// the process cwd to the *package's* manifest directory (`rl-env/`), not
/// the workspace root, so we resolve against the workspace root explicitly
/// via `CARGO_MANIFEST_DIR` (which points at `majula/rl-env`) to keep the
/// same underlying paths regardless of how the test binary is invoked.
fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rl-env has a parent workspace directory")
        .to_path_buf()
}

/// Packs the rev-274 cache exactly once, leaks it to `'static`, and returns it
/// plus a fresh ScriptProvider (each Engine gets its own ScriptProvider).
pub fn shared_cache() -> (&'static CacheStore, ScriptProvider) {
    let root = workspace_root();
    let content_dir = root.join(rs_pack::CONTENT_DIR);
    let pack_dir = root.join(rs_pack::PACK_DIR);

    // pack_all returns (Box<CacheStore>, ScriptProvider). We pack once for the
    // cache (leaked), and re-pack only to obtain a ScriptProvider when needed.
    let cache: &'static CacheStore = *CACHE.get_or_init(|| {
        let (store, _scripts) = rs_pack::pack_all(
            &content_dir,
            &pack_dir,
            false, // verify=false: recompute CRCs, don't assert
            true,  // members
        ).expect("pack_all rev-274");
        Box::leak(store)
    });
    // ScriptProvider is cheap-ish to rebuild; pack again to get one.
    let (_store2, scripts) = rs_pack::pack_all(
        &content_dir,
        &pack_dir,
        false, true,
    ).expect("pack_all scripts");
    (cache, scripts)
}

pub struct EnvHarness {
    pub engine: Engine,
    // Kept so channels stay open (dropping the tx side would close them).
    _stats_rx: watch::Receiver<TickStats>,
    _new_player_tx: tokio::sync::mpsc::UnboundedSender<LoginRequest>,
    _reload_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

impl EnvHarness {
    pub fn boot() -> Self {
        let (cache, scripts) = shared_cache();
        let cache_ptr = cache as *const CacheStore as *mut CacheStore;

        let (stats_tx, _stats_rx) = watch::channel(TickStats::default());
        let (new_player_tx, new_player_rx) = unbounded_channel::<LoginRequest>();
        let (reload_tx, _reload_rx) = unbounded_channel::<()>();
        // Ether/DB disabled: None senders + dummy receivers (never fed).
        let (_e_in_tx, ether_rx) = unbounded_channel::<EtherInbound>();
        let (_d_resp_tx, db_rx) = unbounded_channel::<DbResponse>();

        let (engine, _clock_rate_rx) = Engine::new(
            true,            // members
            1,               // multi_xp
            true,            // client_pathfinder
            new_player_rx,
            scripts,
            cache,
            cache_ptr,
            stats_tx,
            reload_tx.clone(),
            10,              // node_id
            None,            // ether_tx
            ether_rx,
            None,            // db_tx
            db_rx,
        );

        EnvHarness { engine, _stats_rx, _new_player_tx: new_player_tx, _reload_tx: reload_tx }
    }

    pub fn cycle(&mut self) {
        self.engine.cycle();
    }

    pub fn clock(&self) -> u64 {
        self.engine.clock as u64
    }
}
