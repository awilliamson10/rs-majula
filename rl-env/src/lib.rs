pub mod action;
pub mod batch;
pub mod observe;
pub mod reward;
pub mod scenario;

use once_cell::sync::OnceCell;
use std::path::Path;
use rs_engine::{Engine, TickStats, LoginRequest};
use rs_engine::{EtherInbound, DbResponse};
use rs_pack::cache::{CacheStore, VarValue};
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
    /// Last-observed HP per pid. No longer used by `step_reward` (Task 11
    /// switched that to the event-based `player.hits` accumulator, which is
    /// immune to same-tick eat-vs-damage HP diffing bugs -- see
    /// `step_reward`'s doc comment), but left in place / still cleared on
    /// reset in case a future caller wants HP-delta bookkeeping for
    /// something other than reward (e.g. logging/diagnostics).
    prev_hp: std::collections::HashMap<u16, u16>,
    /// Per-episode tick counter, incremented once per [`Self::cycle`] call
    /// and reset to 0 by [`Self::load_scenario`] (and [`Self::reset_duel`]).
    /// Used by [`Self::is_terminal`] to resolve `Terminal::Timeout(n)` /
    /// `Terminal::DeathOrTimeout(n)` scenario conditions.
    episode_tick: u32,
    /// Log of resolved (actually-dispatched) actions, appended to by
    /// [`Self::apply_actions`] and drained by [`Self::drain_recorded`] --
    /// see `action::ResolvedAction`'s doc comment. Never cleared on
    /// `load_scenario`/`reset_duel`: draining is the caller's job, same as
    /// `player.hits`.
    recorded: Vec<crate::action::ResolvedAction>,
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
            episode_tick: 0,
            recorded: Vec::new(),
        }
    }

    /// Advances the engine one tick and bumps [`Self::episode_tick`] (used
    /// by [`Self::is_terminal`]'s `Timeout`/`DeathOrTimeout` resolution).
    pub fn cycle(&mut self) {
        self.engine.cycle();
        self.episode_tick = self.episode_tick.saturating_add(1);
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
    /// task (Task 8), all six heads are wired up.
    ///
    /// Note: the combat interaction set by `Engage` does not persist across
    /// ticks on its own (nothing here re-arms it), so a caller that wants
    /// sustained combat must call this every tick, same as `attack_player`.
    pub fn apply_actions(&mut self, pid: u16, opp: u16, act: &crate::action::MultiAction) {
        use crate::action::{AttackIntent, ResolvedKind};
        let engine_ptr = &mut self.engine as *mut Engine;
        // Collected locally (not into `self.recorded`) because this closure
        // already holds a raw `*mut Engine` borrow of `self.engine` via
        // `with_engine` -- touching `self.recorded` in here too would be a
        // second, conflicting borrow of `self`. Stamped with pid/tick and
        // appended to `self.recorded` after the closure returns instead.
        let mut resolved: Vec<ResolvedKind> = Vec::new();
        with_engine(&mut self.engine, || {
            let engine = unsafe { &mut *engine_ptr };

            // prayer: toggle Protect-from-Melee to match act.prayer, only
            // clicking the button when current state disagrees (checked via
            // player.headicons) so holding the same value across ticks
            // doesn't re-toggle (and thus re-flip) the prayer every tick.
            if act.prayer == 1 {
                if let Some(active) = engine.get_player_mut(pid) {
                    let on =
                        active.player.headicons & crate::action::HEADICON_PROTECT_MELEE != 0;
                    if !on {
                        crate::action::if_button(active, crate::action::com_protect_melee());
                        resolved.push(ResolvedKind::Prayer(1));
                    }
                }
            } else if act.prayer == 0 {
                if let Some(active) = engine.get_player_mut(pid) {
                    let on =
                        active.player.headicons & crate::action::HEADICON_PROTECT_MELEE != 0;
                    if on {
                        crate::action::if_button(active, crate::action::com_protect_melee());
                        resolved.push(ResolvedKind::Prayer(0));
                    }
                }
            }

            // equip: wield the first backpack weapon (M1: act.equip == 1
            // is the only defined gear-set index -- the scenario's spec
            // weapon). Fires the item's own OpHeld{op} (op = the obj's
            // iop-table index for "Wield", NOT necessarily 1 -- see
            // `action::first_wieldable`) through the real handler.
            if act.equip == 1 {
                if let Some(active) = engine.get_player_mut(pid) {
                    if let Some((slot, obj, op)) = crate::action::first_wieldable(active) {
                        crate::action::op_held(active, op, obj, slot, crate::action::inv_com());
                        resolved.push(ResolvedKind::Equip);
                    }
                }
            }
            // eat: op the first edible backpack slot (fires the item's own
            // OpHeld{op} for its "Eat" iop entry).
            if act.eat {
                if let Some(active) = engine.get_player_mut(pid) {
                    if let Some((slot, obj, op)) = crate::action::first_edible(active) {
                        crate::action::op_held(active, op, obj, slot, crate::action::inv_com());
                        resolved.push(ResolvedKind::Eat);
                    }
                }
            }
            // spec: arm the special attack (fires on the next attack that
            // consumes it). See `action::com_special_attack`'s docs for the
            // weapon-category caveat (this targets the stab-weapon spec
            // bar).
            if act.spec {
                if let Some(active) = engine.get_player_mut(pid) {
                    crate::action::if_button(active, crate::action::com_special_attack());
                    resolved.push(ResolvedKind::Spec);
                }
            }

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
                        resolved.push(ResolvedKind::Attack);
                    }
                    AttackIntent::Disengage => {
                        active.player.clear_interaction();
                        resolved.push(ResolvedKind::Disengage);
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
                    // Record the CLAMPED (actually-dispatched) dx/dz, not
                    // the raw requested `act.move_dx/dz` -- the dispatched
                    // move itself clamps to +/-8 above, so recording the
                    // raw value would silently desync the log from what
                    // Phase C replay would need to reproduce this tick.
                    resolved.push(ResolvedKind::Move { dx: dx as i8, dz: dz as i8 });
                }
            }
        });
        self.recorded.extend(resolved.into_iter().map(|kind| crate::action::ResolvedAction {
            pid,
            tick: self.episode_tick,
            kind,
        }));
    }

    /// Drains and returns everything [`Self::apply_actions`] has recorded so
    /// far (across any number of calls/players), leaving `self.recorded`
    /// empty -- the compact resolved-action log Phase C replays to reproduce
    /// a fight. Like `player.hits`, this is an accumulator: callers own when
    /// to drain it (e.g. once per episode, or once per step).
    pub fn drain_recorded(&mut self) -> Vec<crate::action::ResolvedAction> {
        std::mem::take(&mut self.recorded)
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

    /// Fixed-length partial-info observation vector for `pid` w.r.t.
    /// opponent `opp`, plus `pid`'s legality mask -- see
    /// [`crate::observe`]'s index map for field layout.
    ///
    /// **Self fields are exact** (own HP/prayer/spec/run-energy/overhead).
    /// **Opponent fields are client-visible only**: relative position
    /// (dx/dz/dist), overhead prayer icon, a *coarse* HP-bar bucket
    /// (`IDX_OPP_HP_BUCKET`, never the exact HP number), and a recent-hit
    /// flag. The opponent's exact HP, spec energy, and inventory never
    /// appear in the vector -- this is the mission-critical faithfulness
    /// property (the agent must see only what a real 2004 client shows).
    ///
    /// A handful of fields aren't readily sourced yet in M1
    /// (is_attacking, opp weapon class, self attack/eat timers, opp
    /// is_moving) and are left at `0.0` -- see the `TODO M1` comments on
    /// their index constants in [`crate::observe`].
    ///
    /// If either player is absent (e.g. died/despawned), the corresponding
    /// block is left all-zero rather than panicking, so a caller mid-episode
    /// never has to special-case a missing entity.
    pub fn observe(&self, pid: u16, opp: u16) -> (Vec<f32>, crate::observe::Mask) {
        use crate::observe as ob;
        let mut v = vec![0.0f32; ob::OBS_LEN];
        let me = self.engine.get_player(pid);
        let ot = self.engine.get_player(opp);
        if let Some(m) = me {
            v[ob::IDX_SELF_HP] = m.player.stats.levels[3] as f32;
            v[ob::IDX_SELF_PRAYER] = m.player.stats.levels[5] as f32;
            v[ob::IDX_SELF_SPEC] = crate::action::spec_energy(m) as f32;
            v[ob::IDX_SELF_RUN] = m.player.runenergy as f32 / 10000.0;
            v[ob::IDX_SELF_OVERHEAD] =
                (m.player.headicons & crate::action::HEADICON_PROTECT_MELEE != 0) as u8 as f32;
        }
        if let (Some(m), Some(o)) = (me, ot) {
            let (mc, oc) = (m.player.pathing.coord, o.player.pathing.coord);
            let dx = oc.x() as f32 - mc.x() as f32;
            let dz = oc.z() as f32 - mc.z() as f32;
            v[ob::IDX_OPP_DX] = dx;
            v[ob::IDX_OPP_DZ] = dz;
            v[ob::IDX_OPP_DIST] = (dx * dx + dz * dz).sqrt();
            v[ob::IDX_OPP_OVERHEAD] =
                (o.player.headicons & crate::action::HEADICON_PROTECT_MELEE != 0) as u8 as f32;
            // COARSE hp bar: bucket index in [0, OPP_HP_BUCKETS], never exact hp.
            // Faithfulness requirement: the raw opponent HP number must never
            // appear anywhere in the observation vector.
            let hp = o.player.stats.levels[3] as f32;
            let maxhp = (o.player.stats.base_levels[3] as f32).max(1.0);
            let frac = (hp / maxhp).clamp(0.0, 1.0);
            v[ob::IDX_OPP_HP_BUCKET] = (frac * ob::OPP_HP_BUCKETS as f32).round();
            // Sourced from `last_hit_tick` (a plain overwrite), NOT
            // `hits` (an accumulator `step_reward` drains) -- see
            // `Player::last_hit_tick`'s doc comment. In the normal step
            // loop (observe -> apply_actions -> cycle -> step_reward),
            // `hits` is always empty by the time the *next* `observe()`
            // runs (the prior step's `step_reward` already drained it),
            // which would make a `hits`-sourced recent-hit flag a
            // permanently-dead field. `last_hit_tick` survives being read,
            // so this correctly reports "opponent was hit during the
            // just-completed cycle" regardless of reward-draining order.
            let cur_clock = self.engine.clock;
            v[ob::IDX_OPP_RECENT_HIT] =
                (o.player.last_hit_tick == Some(cur_clock.saturating_sub(1))) as u8 as f32;
        }
        (v, self.legal_mask(pid))
    }

    /// Per-head legality mask for `pid` (Task 9) -- see
    /// [`crate::observe::Mask`]'s field docs for what each head checks. A
    /// missing player (e.g. died/despawned) reads as fully illegal on every
    /// head except `move_ok` (which has no player-state precondition),
    /// mirroring [`Self::observe`]'s "never panic on absence" policy.
    pub fn legal_mask(&self, pid: u16) -> crate::observe::Mask {
        let p = self.engine.get_player(pid);
        crate::observe::Mask {
            move_ok: true,
            attack_ok: p.is_some(),
            prayer_ok: p.is_some_and(|a| a.player.stats.levels[5] > 0),
            eat_ok: p.is_some_and(|a| crate::action::first_edible(a).is_some()),
            equip_ok: p.is_some_and(|a| crate::action::first_wieldable(a).is_some()),
            spec_ok: p.is_some_and(|a| {
                crate::action::spec_energy(a) >= crate::action::SPEC_COST_DRAGON_DAGGER
            }),
        }
    }

    /// Reward = `w * (damage dealt to opp this step - damage taken by me
    /// this step)`, read from each player's `player.hits` event accumulator
    /// (Task 2), plus a terminal bonus of `+1.0` if `opp` is dead (HP == 0
    /// or the player is absent) and/or `-1.0` if `me` is dead. This
    /// deliberately replaced an earlier HP-delta implementation: diffing
    /// cached HP across cycles silently *hides* damage when a player eats
    /// (or otherwise heals) the same tick the damage lands, since the net
    /// HP change can end up zero or positive even though a hit landed. The
    /// `hits` accumulator records the explicit damage event regardless of
    /// same-tick healing, so it can't be fooled that way (see
    /// `rl-env/tests/reward.rs::eat_on_damage_tick_does_not_hide_damage`).
    ///
    /// # `hits` is drained here
    /// This reads AND CLEARS both `me.player.hits` and `opp.player.hits`
    /// (the accumulator is a per-tick event queue, not a running total, so
    /// once read it must be emptied or the same hits would double-count on
    /// the next call). [`Self::observe`]'s `IDX_OPP_RECENT_HIT` does NOT read
    /// `hits` -- it derives recent-hit from `player.last_hit_tick` (an
    /// overwrite, not a drainable queue; see the accessor's doc), so there is
    /// no observe-vs-step_reward call-order constraint: draining `hits` here
    /// cannot clobber the recent-hit observation.
    ///
    /// # Not safe to call twice per step (self-play trap)
    /// This is intended to be called exactly ONCE per step, for ONE agent's
    /// perspective. Computing both agents' rewards in the same step by
    /// calling `step_reward(a, b, w)` then `step_reward(b, a, w)` is NOT
    /// safe: the first call drains `b`'s hits while computing `a`'s
    /// "dealt", so the second call (computing `b`'s "taken") sees an
    /// already-empty accumulator and silently under-reports. Phase A's
    /// tests only ever compute one agent's reward per step, so this is fine
    /// for now. Phase B's self-play wrapper (both agents learning
    /// simultaneously) will need a snapshot-then-drain variant -- read both
    /// players' `hits` sums first, THEN clear both -- rather than calling
    /// this method twice; that pair API is deliberately not built yet
    /// (YAGNI for Phase A).
    pub fn step_reward(&mut self, me: u16, opp: u16, w: f32) -> f32 {
        let dealt: u32 = self.engine.get_player_mut(opp)
            .map(|p| { let s = p.player.hits.iter().map(|h| h.amount as u32).sum(); p.player.hits.clear(); s })
            .unwrap_or(0);
        let taken: u32 = self.engine.get_player_mut(me)
            .map(|p| { let s = p.player.hits.iter().map(|h| h.amount as u32).sum(); p.player.hits.clear(); s })
            .unwrap_or(0);
        let mut r = w * (dealt as f32 - taken as f32);
        // terminal bonus folded in when a death is observed this step
        if self.engine.get_player(opp).map_or(true, |p| p.player.stats.levels[3] == 0) { r += 1.0; }
        if self.engine.get_player(me).map_or(true, |p| p.player.stats.levels[3] == 0) { r -= 1.0; }
        r
    }

    /// Snapshot-then-drain-BOTH reward, from both perspectives at once.
    /// Unlike calling [`Self::step_reward`] twice (which drains the first
    /// player's `hits` before the second call reads them — the self-play
    /// trap documented on `step_reward`), this reads both `hits` sums FIRST,
    /// then clears both, so each side's dealt/taken is correct. Terminal
    /// ±1 is folded per side (opp dead -> +1, me dead -> -1), matching
    /// `step_reward`'s bonus. Call exactly once per duel per step.
    pub fn step_reward_pair(&mut self, a: u16, b: u16, w: f32) -> (f32, f32) {
        let a_hits: u32 = self.engine.get_player(a)
            .map(|p| p.player.hits.iter().map(|h| h.amount as u32).sum())
            .unwrap_or(0);
        let b_hits: u32 = self.engine.get_player(b)
            .map(|p| p.player.hits.iter().map(|h| h.amount as u32).sum())
            .unwrap_or(0);
        if let Some(p) = self.engine.get_player_mut(a) { p.player.hits.clear(); }
        if let Some(p) = self.engine.get_player_mut(b) { p.player.hits.clear(); }

        let a_dead = self.engine.get_player(a).map_or(true, |p| p.player.stats.levels[3] == 0);
        let b_dead = self.engine.get_player(b).map_or(true, |p| p.player.stats.levels[3] == 0);

        // a deals b_hits, takes a_hits; b is the mirror.
        let mut ra = w * (b_hits as f32 - a_hits as f32);
        let mut rb = w * (a_hits as f32 - b_hits as f32);
        if b_dead { ra += 1.0; }
        if a_dead { ra -= 1.0; }
        if a_dead { rb += 1.0; }
        if b_dead { rb -= 1.0; }
        (ra, rb)
    }

    /// Resolves `term` against `me`/`opp`'s current liveness and
    /// [`Self::episode_tick`], from `me`'s perspective: `opp` dead -> `Win`,
    /// `me` dead -> `Loss` (checked first for `Death` so a mutual/ambiguous
    /// double-KO tick still resolves the same way Timeout does), and for
    /// `Timeout(n)`/`DeathOrTimeout(n)`, `episode_tick >= n` with neither
    /// side dead -> `Draw`. `None` means the episode has not ended yet.
    ///
    /// Note for Phase B callers: [`Self::step_reward`] already folds a
    /// terminal `+-1.0` bonus into its return value for the same death
    /// condition this resolves. A caller using both must not double-count
    /// that bonus on the terminal step (e.g. by also adding a win/loss
    /// bonus keyed off this method's `Outcome`).
    pub fn is_terminal(&self, me: u16, opp: u16, term: &crate::scenario::Terminal) -> Option<crate::reward::Outcome> {
        use crate::reward::Outcome;
        let me_dead = self.engine.get_player(me).map_or(true, |p| p.player.stats.levels[3] == 0);
        let opp_dead = self.engine.get_player(opp).map_or(true, |p| p.player.stats.levels[3] == 0);
        let timed_out = |limit: u32| self.episode_tick >= limit;
        match term {
            crate::scenario::Terminal::Death => {
                if opp_dead { Some(Outcome::Win) } else if me_dead { Some(Outcome::Loss) } else { None }
            }
            crate::scenario::Terminal::Timeout(n) | crate::scenario::Terminal::DeathOrTimeout(n) => {
                if opp_dead { Some(Outcome::Win) }
                else if me_dead { Some(Outcome::Loss) }
                else if timed_out(*n) { Some(Outcome::Draw) }
                else { None }
            }
        }
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
        self.episode_tick = 0;
        let a = self.engine.spawn_player("pker", CoordGrid::new(3200, 0, 3912));
        let b = self.engine.spawn_player("victim", CoordGrid::new(3201, 0, 3912));
        self.buff_melee(a);
        self.buff_melee(b);
        (a, b)
    }

    /// Spawns one player at `spot`, opens the standard inventory/worn
    /// interfaces (see [`Self::open_standard_interfaces`]'s cold-start note),
    /// and applies `lo`'s stats/inventory/worn/vars. Returns the new pid.
    /// This is the per-player spawn path [`crate::batch::BatchEnv`] uses to
    /// populate its duels; it deliberately does NOT reseed the RNG or draw
    /// jitter (the batch owns its own deterministic spawn order), so it is
    /// independent of `load_scenario`'s single-duel spawn sequence.
    pub fn spawn_and_equip(
        &mut self,
        name: &str,
        spot: rs_grid::CoordGrid,
        lo: &crate::scenario::Loadout,
    ) -> u16 {
        let pid = self.engine.spawn_player(name, spot);
        self.open_standard_interfaces(pid);
        self.apply_loadout_stats_inv(pid, lo);
        pid
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
        self.episode_tick = 0;

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

        // Player vars (varps): resolved up front against the same
        // shared_cache() snapshot as stats/obj lookups above, same
        // fail-loud policy -- an unresolved varp debugname means the
        // scenario is malformed and must abort at load time rather than
        // silently spawning a bot missing quest/state gating it needs
        // (e.g. `zanaris` gating `dragon_dagger`'s Wield in mirror_melee).
        // Mirrors the `::setvar` cheat's resolution (`cache().varps
        // .get_by_debugname`, `rs-engine/src/handlers/client_cheat.rs`
        // around line 972) and its coercion via `VarValue::from_int` using
        // the varp's own declared type and transmit flag.
        let var_updates: Vec<(u16, VarValue, bool)> = lo
            .vars
            .iter()
            .map(|(name, value)| {
                let varp = cache.varps.get_by_debugname(name).unwrap_or_else(|| {
                    panic!("scenario loadout: unresolved varp debugname {name:?}")
                });
                (varp.id, VarValue::from_int(varp.var_type, *value), varp.transmit)
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
            for (id, value, transmit) in &var_updates {
                active.set_varp(*id, value.clone(), *transmit);
            }
            active.recalc_combat_and_appearance();
        });
    }
}
