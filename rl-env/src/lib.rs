pub mod action;
pub mod scenario;

use once_cell::sync::OnceCell;
use std::path::Path;
use rs_engine::{Engine, TickStats, LoginRequest};
use rs_engine::{EtherInbound, DbResponse};
use rs_pack::cache::CacheStore;
use rs_pack::cache::script::ScriptProvider;
use rs_entity::InteractionTarget;
use rs_grid::CoordGrid;
use rs_vm::engine::with_engine;
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

/// Packs the rev-274 cache exactly once, leaks it to `'static`, and returns
/// the leaked reference. Used by both [`shared_cache`] (which additionally
/// rebuilds a fresh `ScriptProvider`) and [`cache`] (which just wants the
/// `CacheStore` for obj/inv/interface lookups, without paying for another
/// script pack on every call).
fn packed_cache() -> &'static CacheStore {
    let root = workspace_root();
    let content_dir = root.join(rs_pack::CONTENT_DIR);
    let pack_dir = root.join(rs_pack::PACK_DIR);
    *CACHE.get_or_init(|| {
        let (store, _scripts) = rs_pack::pack_all(
            &content_dir,
            &pack_dir,
            false, // verify=false: recompute CRCs, don't assert
            true,  // members
        ).expect("pack_all rev-274");
        Box::leak(store)
    })
}

/// Packs the rev-274 cache exactly once, leaks it to `'static`, and returns it
/// plus a fresh ScriptProvider (each Engine gets its own ScriptProvider).
pub fn shared_cache() -> (&'static CacheStore, ScriptProvider) {
    let cache = packed_cache();
    let root = workspace_root();
    let content_dir = root.join(rs_pack::CONTENT_DIR);
    let pack_dir = root.join(rs_pack::PACK_DIR);
    // ScriptProvider is cheap-ish to rebuild; pack again to get one.
    let (_store2, scripts) = rs_pack::pack_all(
        &content_dir,
        &pack_dir,
        false, true,
    ).expect("pack_all scripts");
    (cache, scripts)
}

/// The leaked rev-274 [`CacheStore`] (obj/inv/interface metadata etc.),
/// without also rebuilding a `ScriptProvider` (unlike [`shared_cache`]).
/// Cheap after the first call -- `packed_cache` only packs once per
/// process, via the same memoized `CACHE` cell `shared_cache` uses (and
/// `EnvHarness::boot_inner` always calls `shared_cache` before any
/// `cache()` caller could run, so the cell is populated by construction).
/// Used by [`crate::action`]'s obj/iop lookups (`first_edible`,
/// `first_wieldable`, `inv_com`), which run once per action-head per tick
/// and must not pay for a script repack each time.
pub fn cache() -> &'static CacheStore {
    packed_cache()
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
        Self::boot_inner(true, 1084838400000)
    }

    /// Arena-mode boot: skips spawning the static world NPCs entirely, so
    /// the engine ticks (near) nothing but whatever players the caller spawns
    /// (e.g. via `spawn_player`/`reset_duel`). This is the training-time
    /// mode -- static NPCs are ~98.6% of a full-world tick's cost and are
    /// irrelevant to a headless PvP env that only spawns its own bots.
    pub fn boot_arena() -> Self {
        Self::boot_inner(false, 1084838400000)
    }

    /// Arena-mode boot with an explicit RNG seed, for deterministic/repeatable
    /// episodes (e.g. RL training with seeded resets).
    pub fn boot_arena_seeded(seed: u64) -> Self {
        Self::boot_inner(false, seed)
    }

    /// Shared `Engine::new` construction for [`Self::boot`], [`Self::boot_arena`],
    /// and [`Self::boot_arena_seeded`]; `spawn_static_npcs` and `seed` are
    /// forwarded straight to [`Engine::new`]'s equivalent parameters.
    fn boot_inner(spawn_static_npcs: bool, seed: u64) -> Self {
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
            seed,
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

    /// Applies one tick's worth of [`crate::action::MultiAction`] to `pid`
    /// (with `opp` as the combat/observation counterpart), inside
    /// `with_engine` since the attack and move heads both touch the
    /// thread-local `engine()`/`cache()` state (the latter via
    /// `MoveGameClick`'s handler).
    ///
    /// Heads are applied in the FROZEN intra-tick order `prayer -> equip ->
    /// eat -> spec -> attack -> move`; reordering this changes which head
    /// "wins" when two heads could otherwise interact within the same tick
    /// (e.g. a prayer flick landing before/after an attack). As of this
    /// task (Task 7), `equip`/`eat`/`attack`/`move` are wired up;
    /// `prayer`/`spec` remain reserved placeholders for Task 8.
    ///
    /// Note: the combat interaction set by `Engage` does not persist across
    /// ticks on its own (nothing here re-arms it), so a caller that wants
    /// sustained combat must call this every tick, same as `attack_player`.
    pub fn apply_actions(&mut self, pid: u16, opp: u16, act: &crate::action::MultiAction) {
        use crate::action::AttackIntent;
        let engine_ptr = &mut self.engine as *mut Engine;
        with_engine(&mut self.engine, || {
            let engine = unsafe { &mut *engine_ptr };

            // prayer (Task 8: reserved, no-op)

            // equip: wield the first backpack weapon (M1: act.equip == 1
            // is the only defined gear-set index -- the scenario's spec
            // weapon). Fires the item's own OpHeld{op} (op = the obj's
            // iop-table index for "Wield", NOT necessarily 1 -- see
            // `action::first_wieldable`) through the real handler.
            if act.equip == 1 {
                if let Some(active) = engine.get_player_mut(pid) {
                    if let Some((slot, obj, op)) = crate::action::first_wieldable(active) {
                        crate::action::op_held(active, op, obj, slot, crate::action::inv_com());
                    }
                }
            }
            // eat: op the first edible backpack slot (fires the item's own
            // OpHeld{op} for its "Eat" iop entry).
            if act.eat {
                if let Some(active) = engine.get_player_mut(pid) {
                    if let Some((slot, obj, op)) = crate::action::first_edible(active) {
                        crate::action::op_held(active, op, obj, slot, crate::action::inv_com());
                    }
                }
            }
            // spec   (Task 8: reserved, no-op)

            // attack
            if let Some(active) = engine.get_player_mut(pid) {
                match act.attack {
                    AttackIntent::Engage => {
                        active.player.set_interaction(
                            InteractionTarget::Player { pid: opp },
                            ServerTriggerType::ApPlayer2 as u8,
                            true,
                        );
                        active.player.opcalled = true;
                    }
                    AttackIntent::Disengage => {
                        active.player.clear_interaction();
                    }
                    AttackIntent::Hold => {}
                }
            }

            // move: build a MoveGameClick to the relative destination tile
            // and run it through the real handler (pathing/collision), not
            // a direct state shortcut.
            if act.move_dx != 0 || act.move_dz != 0 {
                if let Some(active) = engine.get_player_mut(pid) {
                    let c = active.player.pathing.coord;
                    let dx = act.move_dx.clamp(-8, 8) as i32;
                    let dz = act.move_dz.clamp(-8, 8) as i32;
                    let dest = CoordGrid::new(
                        (c.x() as i32 + dx) as u16,
                        c.y(),
                        (c.z() as i32 + dz) as u16,
                    );
                    crate::action::move_to(active, dest);
                }
            }
        });
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

    /// Applies a [`crate::scenario::Scenario`] to a freshly-reset engine:
    /// despawns any existing players, reseeds the RNG, draws deterministic
    /// spawn-position jitter, spawns both sides, then applies each side's
    /// stats + backpack inventory (worn equipment lands in a later task).
    /// Returns `(pker_pid, opp_pid)`.
    ///
    /// # Determinism
    /// The sequence -- despawn, reseed, draw jitter (side 0 then side 1,
    /// x then z each), spawn (side 0 "pker" then side 1 "opponent"), apply
    /// loadouts -- is fixed and identical on every call for a given seed, so
    /// two harnesses booted with the same seed and given the same scenario
    /// reach bit-identical spawn state (see `load_is_reproducible` test).
    /// Reordering any of these steps changes the RNG draw stream and breaks
    /// reproducibility.
    pub fn load_scenario(&mut self, sc: &crate::scenario::Scenario) -> (u16, u16) {
        // despawn everyone
        let pids: Vec<u16> = (0..rs_engine::MAX_PLAYERS as u16)
            .filter(|&p| self.engine.get_player(p).is_some())
            .collect();
        for p in pids {
            let _ = self.engine.remove_player(p);
        }
        self.prev_hp.clear();

        // Crisp deterministic fight stream: reseed, THEN draw jitter, THEN
        // spawn. Train and replay run this identical sequence, so the whole
        // episode (jitter + spawn scripts + combat) is reproducible.
        self.engine.random.set_seed(sc.seed as i64);
        let (bx, bl, bz) = sc.spot;
        let jit = |r: &mut rs_util::random::JavaRandom, j: u8| -> i32 {
            if j == 0 { 0 } else { r.next_int_bound(j as i32 * 2 + 1) - j as i32 }
        };
        let (ja_x, ja_z) = (
            jit(&mut self.engine.random, sc.start_jitter),
            jit(&mut self.engine.random, sc.start_jitter),
        );
        let (jb_x, jb_z) = (
            jit(&mut self.engine.random, sc.start_jitter),
            jit(&mut self.engine.random, sc.start_jitter),
        );
        let a = self.engine.spawn_player(
            "pker",
            CoordGrid::new((bx as i32 + ja_x) as u16, bl, (bz as i32 + ja_z) as u16),
        );
        let b = self.engine.spawn_player(
            "opponent",
            CoordGrid::new((bx as i32 + 1 + jb_x) as u16, bl, (bz as i32 + jb_z) as u16),
        );

        // See `open_standard_interfaces` doc comment: a freshly-spawned
        // bot's *first* login script run leaves the backpack inv
        // unregistered (`inv_transmits` empty, standard tabs unresolved),
        // so OpHeld-based actions (eat/equip, Task 7) would be silently
        // rejected without this.
        self.open_standard_interfaces(a);
        self.open_standard_interfaces(b);

        self.apply_loadout_stats_inv(a, &sc.sides[0]);
        self.apply_loadout_stats_inv(b, &sc.sides[1]);
        (a, b)
    }

    /// Re-runs `[proc,initalltabs]` (`content/274/scripts/login_logout/login.rs2`)
    /// for `pid`, right after spawn.
    ///
    /// # Why this is needed (discovered while wiring Task 7's eat/equip
    /// actions, which fire `OpHeld{1..5}`)
    ///
    /// `OpHeld`'s handler (`rs-engine/src/handlers/opheld.rs:132-153`)
    /// rejects the op unless `com` (the inventory-interface component)
    /// resolves to a visible+operable interface *and* is registered in
    /// `player.inv_transmits` for the target inv. Both are normally
    /// established once, during login, by the `Login` trigger script
    /// (`content/274/scripts/login_logout/login.rs2`), whose `[proc,initalltabs]`
    /// runs `inv_transmit(inv, inventory:inv); if_settab(inventory, ^tab_inventory);`
    /// (and the equivalent for "worn"/other tabs).
    ///
    /// `Engine::spawn_player` does drive that same `Login` trigger via
    /// `accept_login` (`rs-engine/src/engine.rs`). But empirically (see
    /// `rl-env/tests/action_eat_equip.rs` and the discovery spike behind
    /// this comment), on a freshly booted `Engine` that *first* run of
    /// `[proc,initalltabs]` leaves `player.inv_transmits` completely empty
    /// and every standard tab (`player.tabs`) holding the interface
    /// system's `0xFFFF` "unresolved" sentinel instead of the real
    /// component ids -- reproduced deterministically across independent
    /// fresh-boot trials, for both the first- and second-spawned player in
    /// a scenario, so it is not an ordering artifact of spawning two
    /// players. A second invocation of the *exact same* script, run here,
    /// resolves every tab correctly and is idempotent (`INV_TRANSMIT`'s vm
    /// op dedupes/replaces existing bindings for the same `com`; see
    /// `rs-engine/rs-vm/src/ops/inv.rs` op 4331). The underlying cold-start
    /// resolution gap looks like a pre-existing bug in this vendored
    /// engine's script/interface-symbol pipeline, not something specific
    /// to this action-space feature; root-causing it lives outside
    /// `rl-env`'s scope (would touch `rs-engine`/`rs-vm` internals) and is
    /// out of scope for Task 7. This re-run is the "minimal, correct step
    /// that opens/registers the standard inventory interface on the
    /// spawned bot" called out in the Task 7 brief's known-risk note: it
    /// drives the exact canonical, content-defined login path a second
    /// time rather than bypassing OpHeld's validation or poking
    /// `inv_transmits`/`tabs` state directly.
    fn open_standard_interfaces(&mut self, pid: u16) {
        let Some(uid) = self.engine.get_player(pid).map(|p| p.player.uid) else {
            return;
        };
        let engine_ptr = &mut self.engine as *mut Engine;
        with_engine(&mut self.engine, || {
            let engine = unsafe { &mut *engine_ptr };
            let _ = engine.run_script_by_name(
                "[proc,initalltabs]",
                Some(rs_vm::subject::ScriptSubject::Player(uid)),
                None,
                Some(true),
                None,
                None,
            );
        });
    }

    /// Applies a [`crate::scenario::Loadout`]'s stats and backpack inventory
    /// to player `pid`. Obj/inv debugname lookups are resolved against
    /// [`shared_cache`] *before* entering [`with_engine`], so they don't
    /// depend on the engine's thread-local `cache()` being installed --
    /// only the pure field mutations (stats, inventory add) and the final
    /// `recalc_combat_and_appearance` (which does touch thread-local
    /// engine/cache state via script triggers) run inside `with_engine`.
    fn apply_loadout_stats_inv(&mut self, pid: u16, lo: &crate::scenario::Loadout) {
        let (cache, _) = shared_cache();
        let inv_id = cache.invs.get_by_debugname("inv").map(|i| i.id);
        // Fail loud: an unresolved stat name or obj debugname means the
        // scenario is malformed. This is a training/authoring harness -- an
        // invalid loadout must abort at load time, never silently spawn a
        // valid-but-incomplete bot (see rl-env/tests/scenario_apply.rs
        // `unresolved_obj_debugname_panics`).
        let stat_updates: Vec<(usize, u8)> = lo
            .stats
            .iter()
            .map(|(name, lvl)| {
                let i = crate::scenario::stat_index(name)
                    .unwrap_or_else(|| panic!("scenario loadout: unknown stat name {name:?}"));
                (i, *lvl)
            })
            .collect();
        let inv_items: Vec<(u16, u32, bool)> = lo
            .inventory
            .iter()
            .map(|(name, count)| {
                let obj = cache.objs.get_by_debugname(name).unwrap_or_else(|| {
                    panic!("scenario loadout: unresolved obj debugname {name:?}")
                });
                (obj.id, *count, obj.stackable)
            })
            .collect();
        // Worn equipment: resolve each declared obj debugname and its
        // cache-declared wear slot up front, same fail-loud policy as
        // stats/inventory above -- an unresolved obj debugname, or an obj
        // with no `wearpos` (i.e. not wearable), aborts loudly rather than
        // silently spawning a bot missing gear it was supposed to have.
        let worn_id = cache.invs.get_by_debugname("worn").map(|i| i.id).unwrap_or(94);
        let worn_items: Vec<(u16, u16)> = lo
            .worn
            .iter()
            .map(|name| {
                let obj = cache.objs.get_by_debugname(name).unwrap_or_else(|| {
                    panic!("scenario loadout: unresolved worn obj debugname {name:?}")
                });
                let wearpos = obj.wearpos.unwrap_or_else(|| {
                    panic!(
                        "scenario loadout: worn obj {name:?} (id {}) has no wearpos in cache",
                        obj.id
                    )
                });
                (wearpos as u16, obj.id)
            })
            .collect();

        // accept_login (spawn_player) already ran with_engine and restored
        // the previous (null) thread-local state on exit, so we re-install
        // it here for the duration of the mutation + recalc, mirroring the
        // raw-pointer pattern `Engine::spawn_player` uses to sidestep the
        // double-mutable-borrow of `self` inside the closure.
        let engine_ptr = &mut self.engine as *mut Engine;
        with_engine(&mut self.engine, || {
            let engine = unsafe { &mut *engine_ptr };
            let Some(active) = engine.get_player_mut(pid) else {
                return;
            };
            for (i, lvl) in &stat_updates {
                active.player.stats.base_levels[*i] = *lvl as u16;
                active.player.stats.levels[*i] = *lvl as u16;
                active.player.stats.xp[*i] = rs_stat::get_exp_by_level(*lvl);
            }
            if let Some(inv_id) = inv_id {
                if let Some(inv) = active.player.invs.get_mut(&inv_id) {
                    for (obj_id, count, stackable) in &inv_items {
                        inv.add(*obj_id, *count, *stackable);
                    }
                }
            }
            if !worn_items.is_empty() {
                // Fail loud: unlike the backpack inv above (which no-ops if
                // absent), a missing "worn" inv on the player means worn
                // equipment silently vanishes -- surface it instead.
                let worn_inv = active.player.invs.get_mut(&worn_id).unwrap_or_else(|| {
                    panic!("scenario loadout: player has no \"worn\" inventory (id {worn_id})")
                });
                for (slot, obj_id) in &worn_items {
                    worn_inv.set(*slot, *obj_id, 1);
                }
                active.buildappearance(worn_id);
            }
            active.recalc_combat_and_appearance();
        });
    }
}
