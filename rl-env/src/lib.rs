use once_cell::sync::OnceCell;
use std::path::Path;
use rs_engine::{Engine, TickStats, LoginRequest};
use rs_engine::{EtherInbound, DbResponse};
use rs_pack::cache::CacheStore;
use rs_pack::cache::script::ScriptProvider;
use rs_entity::InteractionTarget;
use rs_grid::CoordGrid;
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
    /// Last-observed HP per pid, used by `step_reward` to compute HP deltas
    /// across cycles (event masks reset every tick, so a single tick's state
    /// can't tell "damage happened" -- differencing cached HP levels can).
    prev_hp: std::collections::HashMap<u16, u16>,
}

impl EnvHarness {
    /// Full-world boot: spawns all ~7,300 static world NPCs in addition to
    /// whatever players the caller spawns. This is the "real world" env,
    /// used when static-NPC interaction/comparison matters. It is far slower
    /// per-tick than [`Self::boot_arena`] — profiled pure-tick ~72x (1,391 vs
    /// ~100k ticks/s); the `perf` bench under sustained combat shows ~60x
    /// (~0.9k vs ~50k+ ticks/s).
    pub fn boot() -> Self {
        Self::boot_inner(true)
    }

    /// Arena-mode boot: skips spawning the static world NPCs entirely, so
    /// the engine ticks (near) nothing but whatever players the caller spawns
    /// (e.g. via `spawn_player`/`reset_duel`). This is the training-time
    /// mode -- static NPCs are ~98.6% of a full-world tick's cost and are
    /// irrelevant to a headless PvP env that only spawns its own bots.
    pub fn boot_arena() -> Self {
        Self::boot_inner(false)
    }

    /// Shared `Engine::new` construction for [`Self::boot`] and
    /// [`Self::boot_arena`]; `spawn_static_npcs` is forwarded straight to
    /// [`Engine::new`]'s equivalent parameter.
    fn boot_inner(spawn_static_npcs: bool) -> Self {
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
            spawn_static_npcs,
        );

        EnvHarness {
            engine,
            _stats_rx,
            _new_player_tx: new_player_tx,
            _reload_tx: reload_tx,
            prev_hp: std::collections::HashMap::new(),
        }
    }

    pub fn cycle(&mut self) {
        self.engine.cycle();
    }

    pub fn clock(&self) -> u64 {
        self.engine.clock as u64
    }

    /// Latest per-phase tick timings published by the engine after the most
    /// recent `cycle()` (profiling). Fields are per-phase wall-ms.
    pub fn tick_stats(&self) -> TickStats {
        self._stats_rx.borrow().clone()
    }

    /// Stat indices (OSRS order): 0=Attack 1=Defence 2=Strength 3=Hitpoints.
    /// Sets both current and base levels high for reliable melee hits, *and*
    /// backs each level with the matching XP total.
    ///
    /// This XP-consistency matters: `rs_stat::Stats::add_xp` has a "snap"
    /// behavior where, if a stat's current level equals its base level at
    /// the moment any xp is awarded to it, the current level is overwritten
    /// with the XP-derived level (`get_level_by_exp`). If we only patched
    /// `levels`/`base_levels` to 99 while leaving `xp` at its spawn default
    /// (level 10 worth, for Hitpoints), the *very next* incidental xp drop
    /// in that stat (e.g. HP xp from a retaliation hit) would silently
    /// collapse the level back down to what the stale xp implies (10),
    /// with no `damage()` call and no hitsplat -- an desync artifact, not
    /// combat. Setting `xp[i] = get_exp_by_level(99)` makes 99 the
    /// genuine, XP-backed level, so `add_xp`'s snap branch recomputes
    /// `get_level_by_exp(xp)` back to 99 and is a no-op.
    pub fn buff_melee(&mut self, pid: u16) {
        if let Some(p) = self.engine.get_player_mut(pid) {
            let xp99 = rs_stat::get_exp_by_level(99);
            for i in [0usize, 1, 2, 3] {
                p.player.stats.levels[i] = 99;
                p.player.stats.base_levels[i] = 99;
                p.player.stats.xp[i] = xp99;
            }
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

    /// Flat symbolic observation for `pid` w.r.t. opponent `opp`:
    /// `[self_hp, opp_hp, dx, dz, dist]`. If either player is absent (e.g.
    /// died/despawned), returns an all-zero vector of the same length rather
    /// than panicking, so a caller mid-episode never has to special-case a
    /// missing entity.
    pub fn observe(&self, pid: u16, opp: u16) -> Vec<f32> {
        let me = self.engine.get_player(pid);
        let ot = self.engine.get_player(opp);
        let (mc, oc) = match (me, ot) {
            (Some(m), Some(o)) => (m.player.pathing.coord, o.player.pathing.coord),
            _ => return vec![0.0; 5],
        };
        let dx = oc.x() as f32 - mc.x() as f32;
        let dz = oc.z() as f32 - mc.z() as f32;
        vec![
            self.player_hp(pid) as f32,
            self.player_hp(opp) as f32,
            dx,
            dz,
            (dx * dx + dz * dz).sqrt(),
        ]
    }

    /// Reward = damage dealt to `opp` this step minus damage taken by `me`
    /// this step, computed via HP deltas cached in `prev_hp` across cycles
    /// (the engine's own hit/combat event masks reset every tick, so a
    /// single tick's state can't distinguish "no hit" from "a hit already
    /// consumed" -- differencing the cached HP levels can, and is robust
    /// regardless of how many cycles elapsed between calls).
    ///
    /// On the first call for a given pid (e.g. right after `reset_duel`
    /// clears `prev_hp`), that pid's "previous" HP is seeded from its
    /// current HP, so the first observed delta is always zero rather than a
    /// phantom drop/gain from stale bookkeeping.
    pub fn step_reward(&mut self, me: u16, opp: u16) -> f32 {
        let opp_now = self.player_hp(opp);
        let me_now = self.player_hp(me);
        let opp_prev = *self.prev_hp.get(&opp).unwrap_or(&opp_now);
        let me_prev = *self.prev_hp.get(&me).unwrap_or(&me_now);
        self.prev_hp.insert(opp, opp_now);
        self.prev_hp.insert(me, me_now);
        let dealt = opp_prev.saturating_sub(opp_now) as f32;
        let taken = me_prev.saturating_sub(me_now) as f32;
        dealt - taken
    }

    /// Despawns every currently-connected player, clears the `prev_hp`
    /// reward bookkeeping (so the next `step_reward` call doesn't report a
    /// phantom delta against a stale pre-reset HP value), and spawns a
    /// fresh, XP-consistently-buffed attacker/victim pair adjacent to each
    /// other in the Scorpion Valley deep-wilderness zone. Returns their pids.
    pub fn reset_duel(&mut self) -> (u16, u16) {
        let pids: Vec<u16> = (0..rs_engine::MAX_PLAYERS as u16)
            .filter(|&p| self.engine.get_player(p).is_some())
            .collect();
        for p in pids {
            let _ = self.engine.remove_player(p);
        }
        self.prev_hp.clear();
        let a = self.engine.spawn_player("pker", CoordGrid::new(3200, 0, 3912));
        let b = self.engine.spawn_player("victim", CoordGrid::new(3201, 0, 3912));
        self.buff_melee(a);
        self.buff_melee(b);
        (a, b)
    }
}
