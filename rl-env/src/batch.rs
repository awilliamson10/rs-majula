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
    /// Terminal reward for a kill you SURVIVE. Dominant. Swept. A double-KO
    /// pays neither side this -- both just take `death_penalty` (see the
    /// terminal block in [`BatchEnv::step`]).
    pub win_bonus: f32,
    /// Terminal penalty for dying. Low but nonzero. Swept.
    pub death_penalty: f32,
    /// Terminal penalty for a timeout draw. Anti-stall. Swept.
    pub timeout_penalty: f32,
    /// Minimum / maximum Chebyshev separation (tiles) between the two sides at
    /// spawn. Randomized per episode from the engine RNG: this is the organic
    /// PK variable (you close distance, or get jumped). NOT randomized: HP and
    /// spec energy -- synthetic start states that never occur in real play are
    /// a documented training confound.
    pub min_sep: i32,
    pub max_sep: i32,
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
    /// Side B's hitpoints at the start of this episode -- the denominator of
    /// the score's graded partial credit. Read from the freshly-equipped
    /// player, so it follows the scenario loadout instead of a hardcode.
    pub start_hp_b: u16,
    /// Total FRESH damage side A has dealt this episode (for the score).
    pub fresh_dealt_a: u32,
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
    pub(crate) min_sep: i32,
    pub(crate) max_sep: i32,
}

/// REWARD-INDEPENDENT per-episode score for side A -- the sweep/eval objective.
///
/// A clean win (opponent dead, we survived) is exactly `1.0`. Anything else
/// (we died, a double-KO, or a timeout draw) is graded partial credit for the
/// FRESH HP damage we dealt: `0.99 * (fresh_dealt_a / opp_max_hp)^2`, capped
/// below 1. `opp_max_hp` is side B's hitpoints at the start of the episode
/// (`Duel::start_hp_b`), read from the scenario loadout -- an engine fact, NOT
/// a reward coefficient.
///
/// # Why it takes no reward coefficient
///
/// It uses ZERO reward coefficients (`damage_coeff`, `win_bonus`, ...) BY
/// CONSTRUCTION -- there is deliberately no parameter to thread one through.
/// That is the whole point: it is the sweep/eval objective, so the reward can
/// be tuned freely (by Protein or by hand) without moving the thing being
/// optimised. Sweeping a reward against an objective derived from that same
/// reward optimises the reward function against itself.
///
/// # ★ Caveat 1: under MIRROR self-play the mean score is ~skill-invariant
///
/// Both sides run the SAME policy, so by symmetry `P(A wins) ≈ 0.5` no matter
/// how good that policy is. The mean mirror score therefore measures mutual
/// aggression (how fast the two copies trade damage), NOT PK skill. This is a
/// real skill objective ONLY when side A is the learner and side B is a fixed
/// / frozen-pool opponent. **B.2 eval MUST grade against a fixed opponent --
/// never wire the mean mirror score straight into the sweep.**
///
/// # ★ Caveat 2: the graded band is top-compressed
///
/// A win is `1.0`, but every double-KO scores `0.99` and near-losses cluster
/// in `0.97..0.99`, so the metric applies little pressure to SURVIVE -- dying
/// while dealing full damage is nearly as good as winning. Revisit the shape
/// when building eval/Elo.
pub(crate) fn episode_score(a_dead: bool, b_dead: bool, fresh_dealt_a: u32, opp_max_hp: f32) -> f32 {
    if b_dead && !a_dead {
        1.0
    } else {
        // `opp_max_hp` comes from the loadout, so guard the degenerate
        // zero-HP case rather than emitting a NaN into the sweep objective.
        let frac = if opp_max_hp > 0.0 {
            (fresh_dealt_a as f32 / opp_max_hp).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (0.99 * frac * frac).clamp(0.0, 1.0)
    }
}

/// TERMINAL payoff `(side A, side B)`, added on top of the tick's dense
/// damage reward. Pure: a function of the outcome flags and the swept
/// coefficients only, so every branch is unit-testable without having to
/// coax a live fight into producing that outcome.
///
/// Payoff table at the shipped coefficients (win 1.0 / death 0.1 / timeout
/// 0.4):
///
/// | outcome            | A    | B    |
/// |--------------------|------|------|
/// | clean win (B dies) | +1.0 | -0.1 |
/// | clean loss (A dies)| -0.1 | +1.0 |
/// | double-KO          | -0.1 | -0.1 |
/// | timeout draw       | -0.4 | -0.4 |
/// | still fighting     |  0.0 |  0.0 |
///
/// # ★ Why the double-KO branch is explicit
///
/// The win bonus is paid for KILLING AND SURVIVING, never for a trade. Two
/// independent `if`s (`b_dead -> ra += win`; `a_dead -> ra -= death`) would
/// pay each side of a mutual kill `win_bonus - death_penalty` = +0.9 -- i.e.
/// suiciding into the opponent scores 90% of a clean win, so the policy has
/// almost no incentive to survive its kill. A double-KO is just two deaths.
///
/// A death (either side's) takes precedence over `timed_out`: an episode that
/// resolves by a kill on its very last legal tick is a kill, not a draw.
///
/// `episode_score` is deliberately NOT derived from this: it reads only the
/// death flags and fresh damage, so the sweep objective stays
/// reward-independent (`score_metric.rs`).
pub(crate) fn terminal_reward(
    a_dead: bool,
    b_dead: bool,
    timed_out: bool,
    win_bonus: f32,
    death_penalty: f32,
    timeout_penalty: f32,
) -> (f32, f32) {
    if a_dead && b_dead {
        (-death_penalty, -death_penalty)
    } else if b_dead {
        (win_bonus, -death_penalty)
    } else if a_dead {
        (-death_penalty, win_bonus)
    } else if timed_out {
        (-timeout_penalty, -timeout_penalty)
    } else {
        (0.0, 0.0)
    }
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

    /// Draws a seeded `(dx, dz)` offset for side B at a Chebyshev separation in
    /// `[min_sep, max_sep]`.
    ///
    /// # ★ Determinism: the draw order is FIXED and load-bearing
    ///
    /// Exactly three bounded draws, ALWAYS in this order:
    ///   1. `sep`  -- `next_int_bound(span + 1)`, the ring radius
    ///   2. `side` -- `next_int_bound(4)`, which arm of the square ring
    ///   3. `t`    -- `next_int_bound(2 * sep + 1)`, position along that arm
    /// and it is called at exactly one point in the spawn sequence (after side
    /// A is spawned, before side B is), identically in [`BatchEnv::new`] and
    /// [`BatchEnv::respawn`]. A fixed `base_seed` therefore reproduces every
    /// spawn of an entire run exactly -- the cross-process
    /// `determinism_across_processes` gate depends on this. Adding, removing or
    /// reordering ANY draw here (or moving the call relative to the spawns)
    /// shifts the whole RNG stream and breaks it.
    ///
    /// Taking the RNG by `&mut` rather than `&mut self`: `new` builds its duels
    /// before a `BatchEnv` exists, so there is no `self` to borrow there.
    fn draw_offset(rng: &mut rs_util::random::JavaRandom, min_sep: i32, max_sep: i32) -> (i32, i32) {
        // Both halves of this catch a MISCONFIGURATION that would otherwise
        // fail far from its cause:
        //   * `min_sep < 1` can draw `sep <= 0`, and `sep < 0` makes
        //     `2 * sep + 1 <= 0`, which panics deep inside the engine RNG
        //     (`next_int_bound` asserts `n > 0`). `sep == 0` would also spawn
        //     side B on top of side A.
        //   * an INVERTED band (e.g. 10..2) leaves `span == 0` via the
        //     `.max(0)` below, silently pinning every duel to `min_sep` and
        //     ignoring `max_sep` entirely -- a constant separation that looks
        //     healthy in every metric.
        debug_assert!(
            min_sep >= 1 && max_sep >= min_sep,
            "draw_offset needs 1 <= min_sep <= max_sep, got {min_sep}..{max_sep}"
        );
        let span = (max_sep - min_sep).max(0);
        let sep = min_sep + rng.next_int_bound(span + 1);
        // Pick a point on the square ring of Chebyshev radius `sep`: one arm
        // is pinned at ±sep and the other coordinate sweeps [-sep, sep], so
        // max(|dx|, |dz|) == sep on all four arms.
        let side = rng.next_int_bound(4);
        let t = rng.next_int_bound(2 * sep + 1) - sep;
        match side {
            0 => (sep, t),
            1 => (-sep, t),
            2 => (t, sep),
            _ => (t, -sep),
        }
    }

    /// Duel `i`'s two sides' current tiles, `((ax, az), (bx, bz))`.
    ///
    /// Side B's tile is a SEEDED DRAW (see [`Self::draw_offset`]), so anything
    /// reconstructing a duel's spawn -- notably `tests/batch_obs.rs`'s
    /// single-harness obs cross-check -- must READ it from here. Assuming the
    /// old fixed `spot.0 + 1` silently desynchronises the moment the drawn
    /// separation isn't 1.
    pub fn duel_coords(&self, i: usize) -> ((u16, u16), (u16, u16)) {
        let d = &self.duels[i];
        (self.harness.player_coord(d.a), self.harness.player_coord(d.b))
    }

    /// Current Chebyshev separation of each duel's two sides (tiles).
    pub fn duel_separations(&self) -> Vec<i32> {
        (0..self.duels.len()).map(|i| {
            let ((ax, az), (bx, bz)) = self.duel_coords(i);
            (ax as i32 - bx as i32).abs().max((az as i32 - bz as i32).abs())
        }).collect()
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
            // Same seeded draw (and same position in the spawn sequence) as
            // `respawn`, so the opening engagement range varies per duel and
            // per episode. `.max(0)` only guards the u16 conversion: duel
            // spots sit around x/z ~3200 (see the scenario's `spot`), so it
            // never actually clamps and never shrinks the drawn separation.
            let (dx, dz) = Self::draw_offset(&mut harness.engine.random, cfg.min_sep, cfg.max_sep);
            let bx = (spot.0 as i32 + dx).max(0) as u16;
            let bz = (spot.2 as i32 + dz).max(0) as u16;
            let b = harness.spawn_and_equip("opponent",
                CoordGrid::new(bx, spot.1, bz), &sc.sides[1]);
            let min_hp_a = harness.player_hp(a);
            let min_hp_b = harness.player_hp(b);
            let start_hp_b = harness.player_hp(b);
            duels.push(Duel {
                a, b, spot, tick: 0, episodes: 0, min_hp_a, min_hp_b,
                start_hp_b, fresh_dealt_a: 0,
            });
        }

        BatchEnv {
            harness, duels, sides: sc.sides, term: sc.terminal,
            timeout, reward_w: cfg.reward_w,
            damage_coeff: cfg.damage_coeff,
            win_bonus: cfg.win_bonus,
            death_penalty: cfg.death_penalty,
            timeout_penalty: cfg.timeout_penalty,
            min_sep: cfg.min_sep,
            max_sep: cfg.max_sep,
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
        // Read the bounds into locals BEFORE the draw: `draw_offset` takes the
        // RNG by `&mut`, and that borrow ends at the call, leaving `self` free
        // for `spawn_and_equip` below.
        let (min_sep, max_sep) = (self.min_sep, self.max_sep);
        let (dx, dz) = Self::draw_offset(&mut self.harness.engine.random, min_sep, max_sep);
        let bx = (spot.0 as i32 + dx).max(0) as u16;
        let bz = (spot.2 as i32 + dz).max(0) as u16;
        let nb = self.harness.spawn_and_equip("opponent",
            CoordGrid::new(bx, spot.1, bz), &self.sides[1].clone());
        // A freshly spawned player has not moved. Seed prev_coord with their
        // spawn tile so the write_obs() later in THIS SAME step reports
        // is-moving = 0.0 instead of comparing against a recycled pid's stale
        // tile.
        self.harness.note_position(na);
        self.harness.note_position(nb);
        let min_hp_a = self.harness.player_hp(na);
        let min_hp_b = self.harness.player_hp(nb);
        let start_hp_b = self.harness.player_hp(nb);
        self.duels[i] = Duel {
            a: na, b: nb, spot, tick: 0, episodes: eps + 1, min_hp_a, min_hp_b,
            start_hp_b, fresh_dealt_a: 0,
        };
    }

    pub fn step(
        &mut self,
        actions: &[i32],
        obs: &mut [f32],
        rewards: &mut [f32],
        dones: &mut [f32],
        scores: &mut [f32],
    ) {
        debug_assert_eq!(actions.len(), self.num_agents() * Self::ACT_STRIDE);
        debug_assert_eq!(scores.len(), self.duels.len());
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
            self.duels[i].fresh_dealt_a += fresh_dealt_by_a;

            let d = self.damage_coeff;
            let mut ra = d * (fresh_dealt_by_a as f32 - a_took as f32);
            let mut rb = d * (fresh_dealt_by_b as f32 - b_took as f32);

            let a_dead = hp_a == 0;
            let b_dead = hp_b == 0;
            let timed_out = self.timeout.map_or(false, |n| self.duels[i].tick >= n);

            // Terminal payoff on top of the dense stream. Pure function of the
            // outcome flags and the coefficients -- see `terminal_reward`,
            // whose unit tests pin the whole payoff table (including the
            // double-KO rule) without needing a live fight to produce each
            // outcome.
            let (ta, tb) = terminal_reward(
                a_dead, b_dead, timed_out,
                self.win_bonus, self.death_penalty, self.timeout_penalty,
            );
            ra += ta;
            rb += tb;

            rewards[2 * i] = ra;
            rewards[2 * i + 1] = rb;

            let done = self.duel_terminal(&self.duels[i]);
            dones[2 * i] = done as u8 as f32;
            dones[2 * i + 1] = done as u8 as f32;
            scores[i] = -1.0;
            if done {
                // REWARD-INDEPENDENT score: a win is 1.0; a loss/draw is graded
                // partial credit for how close we came, out of the opponent's
                // STARTING hitpoints (from the loadout). Uses NO reward
                // coefficient -- that is what makes sweeping the reward safe.
                // See `episode_score` for the metric's caveats.
                scores[i] = episode_score(
                    a_dead, b_dead,
                    self.duels[i].fresh_dealt_a,
                    self.duels[i].start_hp_b as f32,
                );
                self.respawn(i);
            }
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
            min_sep: 1, max_sep: 12,
        }
    }

    /// Side B's starting hitpoints under `mirror_melee.ron`. The production
    /// denominator is `Duel::start_hp_b` (read from the loadout at spawn);
    /// this is just the value that scenario yields, so these unit tests pin
    /// `episode_score`'s shape at a concrete, readable number.
    const HP99: f32 = 99.0;

    // `episode_score` is the sweep objective. These unit tests pin every
    // branch DETERMINISTICALLY. A live fight only ever exhibits the ONE outcome
    // its seed and action script happen to produce, and the double-KO branch in
    // particular is not reliably producible at all now that episodes resolve by
    // real combat rather than by a simultaneous force-logout -- so the full
    // table is proven here, and the integration tests only have to confirm that
    // the outcome they DO produce is scored correctly.
    #[test]
    fn episode_score_clean_win_is_exactly_one() {
        // Opponent dead, we survived -> 1.0, regardless of fresh damage dealt.
        assert_eq!(episode_score(false, true, 12, HP99), 1.0);
        assert_eq!(episode_score(false, true, 99, HP99), 1.0);
    }

    #[test]
    fn episode_score_double_ko_is_graded_not_a_win() {
        // Both dead is NOT a win (requires `!a_dead`): graded partial, < 1.0.
        let s = episode_score(true, true, 99, HP99);
        assert!(s < 1.0, "double-KO must not score a full win, got {s}");
        // fresh == opp_max_hp -> frac == 1 -> 0.99.
        assert!((s - 0.99).abs() < 1e-6, "expected 0.99 at full fresh damage, got {s}");
    }

    #[test]
    fn episode_score_loss_is_graded_by_fresh_damage_and_below_one() {
        // We died, opponent lived: graded partial credit for fresh damage.
        let none = episode_score(true, false, 0, HP99);
        let half = episode_score(true, false, 50, HP99);
        let near = episode_score(true, false, 90, HP99);
        assert_eq!(none, 0.0, "no fresh damage -> 0.0");
        assert!(half > none && near > half, "score must rise with fresh damage");
        assert!(near < 1.0, "a loss can never reach a full win, got {near}");
    }

    #[test]
    fn episode_score_is_clamped_to_unit_interval() {
        // Fresh damage exceeding opp_max_hp (e.g. overkill accounting) stays
        // bounded; a graded loss never reaches or exceeds 1.0.
        let over = episode_score(true, true, 1000, HP99);
        assert!((0.0..=1.0).contains(&over), "score {over} escaped [0,1]");
        assert!(over < 1.0, "a non-win must stay strictly below the 1.0 win, got {over}");
    }

    #[test]
    fn episode_score_scales_with_the_loadouts_hitpoints_not_a_hardcoded_99() {
        // The denominator is the OPPONENT'S STARTING HP (`Duel::start_hp_b`),
        // so a 50-HP loadout reaches full graded credit at 50 fresh damage.
        // Under the old hardcoded 99.0 this would have scored 0.99*(50/99)^2
        // ~= 0.2526 instead.
        let s = episode_score(true, true, 50, 50.0);
        assert!((s - 0.99).abs() < 1e-6, "expected 0.99 at full fresh damage vs a 50 HP loadout, got {s}");
        // Same fresh damage, tougher opponent -> strictly less credit.
        assert!(episode_score(true, true, 50, 99.0) < s);
    }

    #[test]
    fn episode_score_zero_max_hp_is_zero_not_nan() {
        // Degenerate loadout: the denominator is now DATA (from the scenario),
        // not a hardcoded 99.0, so a 0-HP side B must not divide-by-zero a
        // NaN into the sweep objective.
        let s = episode_score(true, false, 0, 0.0);
        assert_eq!(s, 0.0, "0-HP opponent must score 0.0, got {s}");
        assert!(!episode_score(true, true, 7, 0.0).is_nan(), "score must never be NaN");
    }

    #[test]
    fn episode_score_uses_no_reward_coefficient() {
        // The function's ONLY inputs are the two death flags, raw fresh HP
        // damage, and the fixed opponent max HP. There is deliberately no
        // `damage_coeff`/`win_bonus`/... parameter to thread a reward knob
        // through -- if one were ever added, this call would stop compiling.
        // This documents (and, via the signature, enforces) the property that
        // `score_does_not_depend_on_reward_coefficients` proves end-to-end.
        let _: fn(bool, bool, u32, f32) -> f32 = episode_score;
    }

    // ---- `terminal_reward`: the whole payoff table, deterministically ----
    //
    // These are the primary coverage for the terminal branch. Deliberately
    // asymmetric, non-round coefficients so a branch that returned the WRONG
    // constant (or swapped the two sides) cannot pass by numeric coincidence:
    // every one of the three magnitudes is distinct, and none is the negation
    // or the sum/difference of another.
    const WIN: f32 = 1.0;
    const DEATH: f32 = 0.1;
    const TIMEOUT: f32 = 0.4;

    #[test]
    fn terminal_reward_clean_win_pays_the_bonus_to_the_survivor_only() {
        // B dead, A alive: A survived its kill -> +win_bonus; B just died.
        assert_eq!(
            terminal_reward(false, true, false, WIN, DEATH, TIMEOUT),
            (WIN, -DEATH)
        );
    }

    #[test]
    fn terminal_reward_clean_loss_is_the_mirror_image() {
        // The table must be symmetric under swapping the two sides -- a
        // hardcoded `ra`/`rb` mix-up in one branch shows up here.
        assert_eq!(
            terminal_reward(true, false, false, WIN, DEATH, TIMEOUT),
            (-DEATH, WIN)
        );
    }

    /// ★ Blocker 2 (whole-branch review): a double-KO must pay NEITHER side the
    /// win bonus.
    ///
    /// Two independent `if`s used to pay each side `win_bonus - death_penalty`
    /// = +0.9 for a mutual kill -- 90% of a clean win for suiciding into the
    /// opponent. This was previously covered by a live integration test whose
    /// fixture was the engine's ~101-tick force-logout removing BOTH players at
    /// once (read as `a_dead && b_dead`). That force-logout is now gated off in
    /// arena mode, episodes resolve by real combat, and a simultaneous mutual
    /// kill is no longer reliably producible -- so the property lives here,
    /// where it is exact and deterministic.
    #[test]
    fn terminal_reward_double_ko_pays_neither_side_the_win_bonus() {
        let (a, b) = terminal_reward(true, true, false, WIN, DEATH, TIMEOUT);
        assert_eq!((a, b), (-DEATH, -DEATH), "a double-KO is two deaths, not a win");
        // Stated as the property, not just the constants: under the old
        // two-`if` code both of these were +0.9 and this goes RED.
        assert!(a < 0.0 && b < 0.0, "a mutual kill must be a net negative for both sides");
    }

    #[test]
    fn terminal_reward_timeout_penalises_both_sides() {
        assert_eq!(
            terminal_reward(false, false, true, WIN, DEATH, TIMEOUT),
            (-TIMEOUT, -TIMEOUT)
        );
    }

    #[test]
    fn terminal_reward_is_zero_while_the_fight_is_still_running() {
        // Non-terminal ticks must add nothing to the dense stream -- otherwise
        // every step of every episode carries a terminal coefficient.
        assert_eq!(terminal_reward(false, false, false, WIN, DEATH, TIMEOUT), (0.0, 0.0));
    }

    #[test]
    fn terminal_reward_a_death_on_the_timeout_tick_is_a_kill_not_a_draw() {
        // The timeout tick can also be the tick someone dies. That is a real
        // result and must pay the kill/death table, NOT the draw penalty --
        // and it must not pay both (an `if timed_out` that wasn't mutually
        // exclusive with the death branches would stack -0.4 on top).
        assert_eq!(
            terminal_reward(false, true, true, WIN, DEATH, TIMEOUT),
            (WIN, -DEATH),
            "a kill landing on the timeout tick must still be scored as a kill"
        );
        assert_eq!(
            terminal_reward(true, false, true, WIN, DEATH, TIMEOUT),
            (-DEATH, WIN)
        );
        assert_eq!(
            terminal_reward(true, true, true, WIN, DEATH, TIMEOUT),
            (-DEATH, -DEATH)
        );
    }

    #[test]
    fn terminal_reward_scales_with_the_swept_coefficients() {
        // The coefficients are SWEPT, so the branches must read them rather
        // than bake in the shipped 1.0/0.1/0.4. Distinct primes make a branch
        // that returned the wrong one impossible to miss.
        assert_eq!(terminal_reward(false, true, false, 3.0, 5.0, 7.0), (3.0, -5.0));
        assert_eq!(terminal_reward(true, false, false, 3.0, 5.0, 7.0), (-5.0, 3.0));
        assert_eq!(terminal_reward(true, true, false, 3.0, 5.0, 7.0), (-5.0, -5.0));
        assert_eq!(terminal_reward(false, false, true, 3.0, 5.0, 7.0), (-7.0, -7.0));
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

    /// ★ The `last_dealt` / `last_taken` half of the same stale-per-pid-state
    /// bug class as the test above. `EnvHarness::forget_player` dropped
    /// `prev_coord`/`prev_hp` but NOT the two last-hit maps Task 3 added, so a
    /// player spawned onto a recycled pid inherited the previous occupant's
    /// last-hit magnitudes at `IDX_LAST_DEALT`/`IDX_LAST_TAKEN` (`observe`
    /// reads them with `unwrap_or(0)`, so a leftover entry is indistinguishable
    /// from a real hit) -- 2 of 20 observation floats wrong, once per episode,
    /// forever, in any run long enough for the pid allocator to wrap.
    ///
    /// # Why it needs the same cursor-wrap machinery, and its own env
    ///
    /// Same reason as `respawn_does_not_leave_stale_prev_coord_on_a_reused_pid`
    /// (read its doc comment): the allocator is FORWARD-ONLY, so pid reuse only
    /// happens after the cursor has climbed to the top of its range. It cannot
    /// share that test's fixture, though, because the two bugs need OPPOSITE
    /// setups: that one requires `forget_player` NOT to have run on the pid
    /// (a stale `prev_coord` is what it detects -- `observe` maps an ABSENT
    /// `prev_coord` to is-moving `0.0`, so calling `forget_player` there would
    /// silently destroy its discrimination), whereas this one must go through
    /// the production path, where the pid IS forgotten when it is freed and
    /// the fix is precisely that `forget_player` clears these two maps too.
    ///
    /// # Discrimination
    ///
    /// Delete the two `last_dealt`/`last_taken` removes from
    /// `EnvHarness::forget_player` and the final assertions go RED: pid 1's
    /// entry (`last_dealt = 11`) survives its owner's removal and the reused
    /// pid observes `11.0 / 40.0`. The mid-test sanity assertion is what keeps
    /// the final `== 0.0` from being vacuous -- it proves the values really
    /// were nonzero and really do reach the observation.
    #[test]
    fn stale_last_hit_magnitudes_are_cleared_when_a_pid_is_reused() {
        use rs_entity::player::HitEvent;
        let mut env = BatchEnv::new(cfg(1));
        assert_eq!((env.duels[0].a, env.duels[0].b), (1, 2));

        // Record last-hit magnitudes through the PRODUCTION writer: push the
        // hit events the engine's combat would, then let `hits_pair` (which
        // `step` calls once per duel per tick) drain them into `last_dealt` /
        // `last_taken`. Distinct amounts so a leak can't be mistaken for a
        // coincidence.
        env.harness.engine.get_player_mut(1).expect("pid 1 spawned")
            .player.hits.push(HitEvent { amount: 7, kind: 0 });
        env.harness.engine.get_player_mut(2).expect("pid 2 spawned")
            .player.hits.push(HitEvent { amount: 11, kind: 0 });
        assert_eq!(env.harness.hits_pair(1, 2), (7, 11));

        // Non-vacuity: those magnitudes must actually be observable, or the
        // `== 0.0` assertions at the end would pass no matter what.
        let mut obs = vec![0.0f32; env.num_agents() * BatchEnv::OBS_STRIDE];
        env.write_obs(&mut obs);
        let agent0 = 0; // agent 0 == duel 0 side A == pid 1; its block starts at 0
        assert_eq!(
            obs[agent0 + crate::observe::IDX_LAST_DEALT], 11.0 / 40.0,
            "fixture never reached the observation -- the assertions below would be vacuous"
        );
        assert_eq!(
            obs[agent0 + crate::observe::IDX_LAST_TAKEN], 7.0 / 40.0,
            "fixture never reached the observation -- the assertions below would be vacuous"
        );

        // Walk the allocator's cursor to the top of its forward range
        // (`next_free_id` scans `cursor+1..MAX_PLAYERS-1`, then falls back to
        // the SMALLEST free id), so the next allocation must wrap. Ids
        // 3..=MAX_PLAYERS-2; duel0 still holds 1 and 2 at this point, so the
        // dummies can only take ids above them. These are raw engine
        // spawn/remove calls -- they never touch the last-hit maps.
        let dummy_spot = CoordGrid::new(3300, 0, 3300);
        let dummy_count = (rs_engine::MAX_PLAYERS as u16) - 4; // ids 3..=MAX_PLAYERS-2
        let mut last_dummy = 0;
        for _ in 0..dummy_count {
            last_dummy = env.harness.engine.spawn_player("dummy", dummy_spot);
            let _ = env.harness.engine.remove_player(last_dummy);
        }
        assert_eq!(
            last_dummy, (rs_engine::MAX_PLAYERS as u16) - 2,
            "cursor walk did not reach the top of the allocator's forward range"
        );

        // The real `respawn` under test: it frees pids 1 and 2 (calling
        // `forget_player` on each, as production does) and immediately
        // re-spawns -- and since the cursor has wrapped, 1 and 2 are the
        // smallest free ids, so the new players land right back on them.
        env.respawn(0);
        assert_eq!(
            (env.duels[0].a, env.duels[0].b), (1, 2),
            "test setup did not force the respawn to reuse pids (1, 2)"
        );

        env.write_obs(&mut obs);
        for agent in [0usize, 1] {
            let base = agent * BatchEnv::OBS_STRIDE;
            assert_eq!(
                obs[base + crate::observe::IDX_LAST_DEALT], 0.0,
                "agent {agent}: freshly spawned player observes a last-DEALT hit \
                 inherited from the previous occupant of its reused pid"
            );
            assert_eq!(
                obs[base + crate::observe::IDX_LAST_TAKEN], 0.0,
                "agent {agent}: freshly spawned player observes a last-TAKEN hit \
                 inherited from the previous occupant of its reused pid"
            );
        }
    }
}
