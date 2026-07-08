use once_cell::sync::OnceCell;
use std::path::Path;
use rs_engine::{Engine, TickStats, LoginRequest};
use rs_engine::{EtherInbound, EtherOutbound, DbRequest, DbResponse};
use rs_pack::cache::CacheStore;
use rs_pack::cache::script::ScriptProvider;
use rs_entity::InteractionTarget;
use rs_vm::trigger::ServerTriggerType;
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

    /// Stat indices (OSRS order): 0=Attack 1=Defence 2=Strength 3=Hitpoints.
    /// Sets both current and base levels high for reliable melee hits.
    pub fn buff_melee(&mut self, pid: u16) {
        if let Some(p) = self.engine.get_player_mut(pid) {
            for i in [0usize, 1, 2] {
                p.player.stats.levels[i] = 99;
                p.player.stats.base_levels[i] = 99;
            }
            p.player.stats.levels[3] = 99;
            p.player.stats.base_levels[3] = 99;
        }
    }

    /// Injects a melee-attack interaction on `attacker` targeting NPC `target_nid`,
    /// bypassing the packet/handler pipeline (direct engine-state mutation). The
    /// cow's (and most attackable low-level NPCs') "Attack" menu op is op2, so the
    /// trigger used is `ApNpc2`.
    ///
    /// This mirrors the tail of the real `OpNpc` handler's `set_interaction` +
    /// `opcalled = true` sequence (see `rs-engine/src/handlers/opnpc.rs`), but
    /// -- unlike that handler -- does not call `clear_pending_action()` first,
    /// since a freshly spawned bot has no prior interaction/modal state to
    /// clear (`Engine::spawn_player` already closes the post-login welcome
    /// modal via `EnginePlayer::clear_pending_action` before returning).
    pub fn attack_npc(&mut self, attacker: u16, target_nid: u16) {
        if let Some(p) = self.engine.get_player_mut(attacker) {
            p.player.set_interaction(
                InteractionTarget::Npc { nid: target_nid },
                ServerTriggerType::ApNpc2 as u8,
                true,
            );
            p.player.opcalled = true;
        }
    }

    /// Injects a melee-attack interaction on `attacker` targeting player `target_pid`.
    /// See [`Self::attack_npc`] for details on how this relates to the real handler.
    pub fn attack_player(&mut self, attacker: u16, target_pid: u16) {
        if let Some(p) = self.engine.get_player_mut(attacker) {
            p.player.set_interaction(
                InteractionTarget::Player { pid: target_pid },
                ServerTriggerType::ApPlayer2 as u8,
                true,
            );
            p.player.opcalled = true;
        }
    }

    pub fn player_hp(&self, pid: u16) -> u16 {
        self.engine
            .get_player(pid)
            .map(|p| p.player.stats.levels[3])
            .unwrap_or(0)
    }

    /// Current (live) hitpoints of NPC `nid`. NPCs share the same
    /// `stats.levels`/`base_levels` layout as players (index 3 = Hitpoints;
    /// see `rs-pack::types::NpcStat::Hitpoints = 3`), and `ActiveNpc::damage`
    /// decrements `npc.stats.levels[Hitpoints]` directly, so this reads the
    /// true current HP (not a static/base value).
    pub fn npc_hp(&self, nid: u16) -> u16 {
        self.engine
            .get_npc(nid)
            .map(|n| n.npc.stats.levels[3])
            .unwrap_or(0)
    }
}
