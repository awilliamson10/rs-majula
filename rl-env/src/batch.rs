//! Batched multi-agent env: M independent 1v1 duels hosted in ONE arena
//! engine, stepped together by a single `cycle()`. Agent index `2i`/`2i+1`
//! are duel `i`'s two sides (pid changes across auto-reset; the index does
//! not). See the B.1 plan / Phase B design for the throughput rationale.

use rs_grid::CoordGrid;
use crate::EnvHarness;
use crate::scenario::{Scenario, Loadout, Terminal};
use crate::action::{MultiAction, AttackIntent};

pub struct BatchConfig {
    pub scenario_path: String,
    pub num_duels: usize,
    pub base_seed: u64,
    /// Tiles between adjacent duel spawn spots on a square grid. The only
    /// cross-duel interference channel is the shared collision map (a
    /// player's tile is flagged occupied); obs and attack-targeting are by
    /// explicit pid, so they never leak across duels. A stride comfortably
    /// beyond how far a bot wanders in one episode keeps collision isolated.
    pub spot_stride: i32,
    /// Retained for API compatibility; the shaped reward now uses the four
    /// coefficients below.
    pub reward_w: f32,
    /// Dense shaping on FRESH damage. Keep SMALL -- PuffeRL clamps each step's
    /// reward to [-1,1], and the kill must dominate the cumulative dense term.
    /// Swept by Protein.
    pub damage_coeff: f32,
    /// Terminal reward for a kill. Dominant. Swept.
    pub win_bonus: f32,
    /// Terminal penalty for dying. Low but nonzero. Swept.
    pub death_penalty: f32,
    /// Terminal penalty for a timeout draw. Anti-stall. Swept.
    pub timeout_penalty: f32,
}

pub(crate) struct Duel {
    pub a: u16,
    pub b: u16,
    pub spot: (u16, u8, u16),
    pub tick: u32,
    pub episodes: u64,
    /// Lowest HP each side has been reduced to THIS episode. Damage that does
    /// not push a player below their own minimum is NOT "fresh" -- it is
    /// damage they already took and healed back, and paying for it again is a
    /// measured reward-farm exploit.
    pub min_hp_a: u16,
    pub min_hp_b: u16,
}

pub struct BatchEnv {
    pub(crate) harness: EnvHarness,
    pub(crate) duels: Vec<Duel>,
    pub(crate) sides: [Loadout; 2],
    pub(crate) term: Terminal,
    pub(crate) timeout: Option<u32>,
    pub(crate) reward_w: f32,
    pub(crate) damage_coeff: f32,
    pub(crate) win_bonus: f32,
    pub(crate) death_penalty: f32,
    pub(crate) timeout_penalty: f32,
}

impl BatchEnv {
    pub const OBS_STRIDE: usize = 26; // 20 obs + 6 mask
    pub const ACT_STRIDE: usize = 6;  // move,attack,prayer,eat,equip,spec

    /// `i`-th duel spot: a square grid around the scenario `spot`, columns
    /// first. Deterministic function of `i` and `stride` only.
    fn spot_for(base: (u16, u8, u16), stride: i32, i: usize) -> (u16, u8, u16) {
        let cols = 64usize; // wide grid; wilderness is large & open here
        let gx = (i % cols) as i32 * stride;
        let gz = (i / cols) as i32 * stride;
        ((base.0 as i32 + gx) as u16, base.1, (base.2 as i32 + gz) as u16)
    }

    pub fn new(cfg: BatchConfig) -> Self {
        let sc = Scenario::load(&cfg.scenario_path).expect("BatchEnv: load scenario");
        let mut harness = EnvHarness::boot_arena_seeded(cfg.base_seed);
        // One reseed up front; the whole batch's stream is then a
        // deterministic function of base_seed + the action stream.
        harness.engine.random.set_seed(cfg.base_seed as i64);

        let timeout = match sc.terminal {
            Terminal::Death => None,
            Terminal::Timeout(n) | Terminal::DeathOrTimeout(n) => Some(n),
        };

        let mut duels = Vec::with_capacity(cfg.num_duels);
        for i in 0..cfg.num_duels {
            let spot = Self::spot_for(sc.spot, cfg.spot_stride, i);
            let a = harness.spawn_and_equip("pker",
                CoordGrid::new(spot.0, spot.1, spot.2), &sc.sides[0]);
            let b = harness.spawn_and_equip("opponent",
                CoordGrid::new(spot.0 + 1, spot.1, spot.2), &sc.sides[1]);
            let min_hp_a = harness.player_hp(a);
            let min_hp_b = harness.player_hp(b);
            duels.push(Duel { a, b, spot, tick: 0, episodes: 0, min_hp_a, min_hp_b });
        }

        BatchEnv {
            harness, duels, sides: sc.sides, term: sc.terminal,
            timeout, reward_w: cfg.reward_w,
            damage_coeff: cfg.damage_coeff,
            win_bonus: cfg.win_bonus,
            death_penalty: cfg.death_penalty,
            timeout_penalty: cfg.timeout_penalty,
        }
    }

    pub fn num_agents(&self) -> usize { self.duels.len() * 2 }
    pub fn num_duels(&self) -> usize { self.duels.len() }

    /// Current length of `harness.recorded` *without* draining it -- test
    /// hook proving `step`'s per-step drain (see the end of [`Self::step`])
    /// actually bounds the accumulator instead of leaking across steps.
    pub fn recorded_len(&self) -> usize { self.harness.recorded.len() }

    /// Test/introspection helpers.
    pub fn agent_hp(&self, agent: usize) -> u16 {
        let d = &self.duels[agent / 2];
        let pid = if agent % 2 == 0 { d.a } else { d.b };
        self.harness.player_hp(pid)
    }

    /// Test-support: toggle auto-retaliate for one agent's player. See
    /// `EnvHarness::set_auto_retaliate`.
    pub fn set_agent_auto_retaliate(&mut self, agent: usize, on: bool) {
        let d = &self.duels[agent / 2];
        let pid = if agent % 2 == 0 { d.a } else { d.b };
        self.harness.set_auto_retaliate(pid, on);
    }
    pub fn duel_spots(&self) -> Vec<(u16, u8, u16)> {
        self.duels.iter().map(|d| d.spot).collect()
    }

    /// Fills `out` (len == num_agents * OBS_STRIDE) with each agent's
    /// `OBS_LEN`-float observation followed by its 6 mask bits.
    pub fn write_obs(&self, out: &mut [f32]) {
        debug_assert_eq!(out.len(), self.num_agents() * Self::OBS_STRIDE);
        for (i, d) in self.duels.iter().enumerate() {
            self.fill_agent(out, 2 * i, d.a, d.b);
            self.fill_agent(out, 2 * i + 1, d.b, d.a);
        }
    }

    fn fill_agent(&self, out: &mut [f32], agent: usize, me: u16, opp: u16) {
        use crate::observe::OBS_LEN;
        let base = agent * Self::OBS_STRIDE;
        let (v, mask) = self.harness.observe(me, opp);
        out[base..base + OBS_LEN].copy_from_slice(&v[..OBS_LEN]);
        out[base + OBS_LEN + 0] = mask.move_ok as u8 as f32;
        out[base + OBS_LEN + 1] = mask.attack_ok as u8 as f32;
        out[base + OBS_LEN + 2] = mask.prayer_ok as u8 as f32;
        out[base + OBS_LEN + 3] = mask.eat_ok as u8 as f32;
        out[base + OBS_LEN + 4] = mask.equip_ok as u8 as f32;
        out[base + OBS_LEN + 5] = mask.spec_ok as u8 as f32;
    }

    fn move_offset(m: i32) -> (i8, i8) {
        // 0=stay,1=N,2=NE,3=E,4=SE,5=S,6=SW,7=W,8=NW  (N=+z, E=+x)
        match m {
            1 => (0, 1), 2 => (1, 1), 3 => (1, 0), 4 => (1, -1),
            5 => (0, -1), 6 => (-1, -1), 7 => (-1, 0), 8 => (-1, 1),
            _ => (0, 0),
        }
    }

    fn decode_action(row: &[i32]) -> MultiAction {
        let (dx, dz) = Self::move_offset(row[0]);
        let attack = match row[1] { 1 => AttackIntent::Engage, 2 => AttackIntent::Disengage, _ => AttackIntent::Hold };
        MultiAction {
            move_dx: dx, move_dz: dz, attack,
            prayer: row[2].clamp(0, 1) as u8,
            eat: row[3] != 0,
            equip: row[4].clamp(0, 1) as u8,
            spec: row[5] != 0,
        }
    }

    fn duel_terminal(&self, d: &Duel) -> bool {
        let a_dead = self.harness.player_hp(d.a) == 0;
        let b_dead = self.harness.player_hp(d.b) == 0;
        let timed = self.timeout.map_or(false, |n| d.tick >= n);
        a_dead || b_dead || (matches!(self.term, Terminal::Timeout(_) | Terminal::DeathOrTimeout(_)) && timed)
    }

    fn respawn(&mut self, i: usize) {
        let (a, b, spot, eps) = {
            let d = &self.duels[i];
            (d.a, d.b, d.spot, d.episodes)
        };
        let _ = self.harness.engine.remove_player(a);
        let _ = self.harness.engine.remove_player(b);
        self.harness.forget_player(a);
        self.harness.forget_player(b);
        let na = self.harness.spawn_and_equip("pker",
            CoordGrid::new(spot.0, spot.1, spot.2), &self.sides[0].clone());
        let nb = self.harness.spawn_and_equip("opponent",
            CoordGrid::new(spot.0 + 1, spot.1, spot.2), &self.sides[1].clone());
        // A freshly spawned player has not moved. Seed prev_coord with their
        // spawn tile so the write_obs() later in THIS SAME step reports
        // is-moving = 0.0 instead of comparing against a recycled pid's stale
        // tile.
        self.harness.note_position(na);
        self.harness.note_position(nb);
        let min_hp_a = self.harness.player_hp(na);
        let min_hp_b = self.harness.player_hp(nb);
        self.duels[i] = Duel { a: na, b: nb, spot, tick: 0, episodes: eps + 1, min_hp_a, min_hp_b };
    }

    pub fn step(&mut self, actions: &[i32], obs: &mut [f32], rewards: &mut [f32], dones: &mut [f32]) {
        debug_assert_eq!(actions.len(), self.num_agents() * Self::ACT_STRIDE);
        // 1. Apply both sides of every duel (no cycle yet).
        for i in 0..self.duels.len() {
            let (a, b) = (self.duels[i].a, self.duels[i].b);
            let ra = 2 * i * Self::ACT_STRIDE;
            let rb = (2 * i + 1) * Self::ACT_STRIDE;
            let act_a = Self::decode_action(&actions[ra..ra + Self::ACT_STRIDE]);
            let act_b = Self::decode_action(&actions[rb..rb + Self::ACT_STRIDE]);
            self.harness.apply_actions(a, b, &act_a);
            self.harness.apply_actions(b, a, &act_b);
        }
        // 2. One cycle advances every duel.
        self.harness.cycle();
        // 3. Reward + terminal + auto-reset, per duel (deterministic index order).
        for i in 0..self.duels.len() {
            self.duels[i].tick += 1;
            let (a, b) = (self.duels[i].a, self.duels[i].b);

            let (a_took, b_took) = self.harness.hits_pair(a, b);

            // FRESH damage: only credit damage that pushes a player BELOW their
            // episode minimum. Damage they healed back is not paid twice.
            let hp_a = self.harness.player_hp(a);
            let hp_b = self.harness.player_hp(b);
            let fresh_on_a = self.duels[i].min_hp_a.saturating_sub(hp_a) as u32;
            let fresh_on_b = self.duels[i].min_hp_b.saturating_sub(hp_b) as u32;
            self.duels[i].min_hp_a = self.duels[i].min_hp_a.min(hp_a);
            self.duels[i].min_hp_b = self.duels[i].min_hp_b.min(hp_b);
            // Cap fresh damage by the damage actually dealt this step, so a
            // non-combat HP drop could never be credited as a hit.
            let fresh_dealt_by_a = fresh_on_b.min(b_took);
            let fresh_dealt_by_b = fresh_on_a.min(a_took);

            let d = self.damage_coeff;
            let mut ra = d * (fresh_dealt_by_a as f32 - a_took as f32);
            let mut rb = d * (fresh_dealt_by_b as f32 - b_took as f32);

            let a_dead = hp_a == 0;
            let b_dead = hp_b == 0;
            let timed_out = self.timeout.map_or(false, |n| self.duels[i].tick >= n);

            if b_dead { ra += self.win_bonus;  rb -= self.death_penalty; }
            if a_dead { rb += self.win_bonus;  ra -= self.death_penalty; }
            if !a_dead && !b_dead && timed_out {
                ra -= self.timeout_penalty;
                rb -= self.timeout_penalty;
            }

            rewards[2 * i] = ra;
            rewards[2 * i + 1] = rb;

            let done = self.duel_terminal(&self.duels[i]);
            dones[2 * i] = done as u8 as f32;
            dones[2 * i + 1] = done as u8 as f32;
            if done { self.respawn(i); }
        }
        // 4. Fresh observation. Uses the PRE-this-tick `prev_coord` snapshot
        // (from the end of the previous step) to derive is-moving for the
        // tick just completed -- must run BEFORE `note_positions` below, or
        // it would compare the just-cycled position against itself.
        self.write_obs(obs);
        // Snapshot positions so the NEXT step's observe() can derive
        // is-moving for the tick THAT step completes.
        self.harness.note_positions();
        // 5. `apply_actions` (step 1, twice per duel) appended every
        // dispatched action to `self.harness.recorded` -- the accumulator
        // Phase C replay drains via `drain_recorded()`. `BatchEnv` doesn't
        // use the replay log, so leaving it unconsumed would grow it
        // unbounded over a training run; drain (and discard) it here to
        // bound it to at most one step's worth.
        self.harness.drain_recorded();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(m: usize) -> BatchConfig {
        BatchConfig {
            scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
            num_duels: m, base_seed: 1000, spot_stride: 32, reward_w: 1.0,
            damage_coeff: 0.005, win_bonus: 1.0, death_penalty: 0.1, timeout_penalty: 0.4,
        }
    }

    /// Regression for the `forget_player`/`note_position` fix `respawn`
    /// applies around its `remove_player`/`spawn_and_equip` pair.
    ///
    /// # Why this is a `#[cfg(test)]` unit test, not an integration test
    /// exercising `BatchEnv::step` end to end
    ///
    /// The engine's pid allocator (`PlayerList::next_pid`,
    /// `rs-engine/src/engine.rs`) is FORWARD-ONLY: it fills
    /// `cursor+1..MAX_PLAYERS-1` in ascending order and only falls back to
    /// reusing a freed id once the cursor has climbed all the way to the top
    /// of that range. A duel's own pids are also always numerically smaller
    /// than any pid allocated after it, so once reuse finally kicks in, a
    /// duel that respawns reclaims ITS OWN just-freed pids first (they're
    /// the smallest free ids in existence) -- never a different duel's. Real
    /// cross-duel pid reuse (what actually corrupts `prev_coord` in
    /// production) requires an EARLIER duel's pids to still be sitting
    /// free, unclaimed, when a LATER duel respawns after the cursor has
    /// wrapped -- a scenario that needs a huge, specific amount of batch
    /// churn to arise naturally (confirmed empirically: a `BatchEnv::step`
    /// loop driven for 1200 ticks with a single duel never reused a pid at
    /// all, fix or no fix). This test manufactures that exact scenario
    /// directly -- via raw `harness.engine` spawn/remove calls this
    /// `mod tests` can reach because it's compiled inside the crate (an
    /// external `tests/*.rs` integration test cannot: `harness` and
    /// `respawn` are both crate-private) -- and then calls the real,
    /// private `respawn` under test.
    #[test]
    fn respawn_does_not_leave_stale_prev_coord_on_a_reused_pid() {
        let mut env = BatchEnv::new(cfg(2));
        // Fresh engine + 2 duels: duel0 = pids (1,2), duel1 = pids (3,4).
        assert_eq!((env.duels[0].a, env.duels[0].b), (1, 2));
        assert_eq!((env.duels[1].a, env.duels[1].b), (3, 4));
        assert_ne!(env.duels[0].spot, env.duels[1].spot, "duels must occupy different tiles");

        // Simulate duel0 having died at an earlier tick: record its
        // players' (unmoved-since-spawn) tile as "last known", exactly what
        // a real step loop's `note_positions()` does once per tick, then
        // free its pids WITHOUT respawning it. Pids 1 and 2 are now the
        // smallest free ids in existence -- nothing allocated after them
        // can ever be smaller.
        env.harness.note_position(1);
        env.harness.note_position(2);
        let _ = env.harness.engine.remove_player(1);
        let _ = env.harness.engine.remove_player(2);

        // Walk the allocator's cursor from 4 up to the top of its range
        // (MAX_PLAYERS - 2) with throwaway spawn+remove pairs, so the next
        // allocation's forward search is empty and it MUST wrap -- and the
        // wrap phase always returns the SMALLEST free id, i.e. pid 1 (then
        // 2), not one of these dummies or duel1's own pids.
        let dummy_spot = CoordGrid::new(3300, 0, 3300);
        let dummy_count = (rs_engine::MAX_PLAYERS as u16) - 2 - 4; // ids 5..=MAX_PLAYERS-2
        for _ in 0..dummy_count {
            let pid = env.harness.engine.spawn_player("dummy", dummy_spot);
            let _ = env.harness.engine.remove_player(pid);
        }

        // The real `respawn` under test. duel1's new pids MUST come out as
        // (1, 2) -- duel0's old identity -- per the reasoning above.
        env.respawn(1);
        assert_eq!(
            (env.duels[1].a, env.duels[1].b), (1, 2),
            "test setup did not force duel1's respawn to reuse duel0's old (1,2) pids"
        );

        let mut obs = vec![0.0f32; env.num_agents() * BatchEnv::OBS_STRIDE];
        env.write_obs(&mut obs);
        for agent in [2usize, 3] {
            let base = agent * BatchEnv::OBS_STRIDE;
            assert_eq!(
                obs[base + crate::observe::IDX_OPP_ISMOVING], 0.0,
                "agent {agent}: spurious is-moving=1.0 right after respawn \
                 (stale prev_coord inherited from duel0's reused pid)"
            );
        }
    }
}
