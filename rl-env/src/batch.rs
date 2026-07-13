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
    pub reward_w: f32,
}

pub(crate) struct Duel {
    pub a: u16,
    pub b: u16,
    pub spot: (u16, u8, u16),
    pub tick: u32,
    pub episodes: u64,
}

pub struct BatchEnv {
    pub(crate) harness: EnvHarness,
    pub(crate) duels: Vec<Duel>,
    pub(crate) sides: [Loadout; 2],
    pub(crate) term: Terminal,
    pub(crate) timeout: Option<u32>,
    pub(crate) reward_w: f32,
}

impl BatchEnv {
    pub const OBS_STRIDE: usize = 22; // 16 obs + 6 mask
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
            duels.push(Duel { a, b, spot, tick: 0, episodes: 0 });
        }

        BatchEnv {
            harness, duels, sides: sc.sides, term: sc.terminal,
            timeout, reward_w: cfg.reward_w,
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
    pub fn duel_spots(&self) -> Vec<(u16, u8, u16)> {
        self.duels.iter().map(|d| d.spot).collect()
    }

    /// Fills `out` (len == num_agents * OBS_STRIDE) with each agent's
    /// 16-float Phase-A observation followed by its 6 mask bits.
    pub fn write_obs(&self, out: &mut [f32]) {
        debug_assert_eq!(out.len(), self.num_agents() * Self::OBS_STRIDE);
        for (i, d) in self.duels.iter().enumerate() {
            self.fill_agent(out, 2 * i, d.a, d.b);
            self.fill_agent(out, 2 * i + 1, d.b, d.a);
        }
    }

    fn fill_agent(&self, out: &mut [f32], agent: usize, me: u16, opp: u16) {
        let base = agent * Self::OBS_STRIDE;
        let (v, mask) = self.harness.observe(me, opp);
        out[base..base + 16].copy_from_slice(&v[..16]);
        out[base + 16] = mask.move_ok as u8 as f32;
        out[base + 17] = mask.attack_ok as u8 as f32;
        out[base + 18] = mask.prayer_ok as u8 as f32;
        out[base + 19] = mask.eat_ok as u8 as f32;
        out[base + 20] = mask.equip_ok as u8 as f32;
        out[base + 21] = mask.spec_ok as u8 as f32;
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
        let na = self.harness.spawn_and_equip("pker",
            CoordGrid::new(spot.0, spot.1, spot.2), &self.sides[0].clone());
        let nb = self.harness.spawn_and_equip("opponent",
            CoordGrid::new(spot.0 + 1, spot.1, spot.2), &self.sides[1].clone());
        self.duels[i] = Duel { a: na, b: nb, spot, tick: 0, episodes: eps + 1 };
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
            let (ra, rb) = self.harness.step_reward_pair(a, b, self.reward_w);
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
