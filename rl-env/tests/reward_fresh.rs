use rl_env::batch::{BatchConfig, BatchEnv};

fn cfg(damage_coeff: f32) -> BatchConfig {
    BatchConfig {
        scenario_path: concat!(env!("CARGO_MANIFEST_DIR"), "/scenarios/mirror_melee.ron").into(),
        num_duels: 1,
        base_seed: 1000,
        spot_stride: 32,
        reward_w: 1.0,
        damage_coeff,
        win_bonus: 1.0,
        death_penalty: 0.1,
        timeout_penalty: 0.4,
        min_sep: 1,
        max_sep: 12,
    }
}

fn engage_row(dst: &mut [i32]) { dst.copy_from_slice(&[0, 1, 0, 0, 0, 0]); }
fn eat_row(dst: &mut [i32])    { dst.copy_from_slice(&[0, 0, 0, 1, 0, 0]); }

#[test]
fn damage_the_opponent_eats_back_is_not_paid_twice() {
    // Agent A attacks; agent B eats to heal back up. With RAW dealt-taken, A
    // would be paid again for re-dealing the same HP. With FRESH damage, A is
    // only paid for pushing B below B's lowest-ever HP this episode.
    let mut env = BatchEnv::new(cfg(1.0));
    env.set_agent_auto_retaliate(1, false);
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];

    // Phase 1: A attacks, B just tanks. Accumulate A's reward.
    let mut r_phase1 = 0.0f32;
    for _ in 0..25 {
        engage_row(&mut acts[0..6]);
        acts[6..12].copy_from_slice(&[0, 0, 0, 0, 0, 0]); // B idles
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 { break; }
        r_phase1 += rew[0];
    }
    let hp_low = env.agent_hp(1);
    assert!(hp_low < 99, "B should have taken damage in phase 1");

    // Phase 2: A holds; B eats back up above its minimum.
    for _ in 0..12 {
        acts[0..6].copy_from_slice(&[0, 0, 0, 0, 0, 0]); // A idles
        eat_row(&mut acts[6..12]);                        // B eats
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 { break; }
    }
    let hp_healed = env.agent_hp(1);
    assert!(hp_healed > hp_low, "B should have healed (got {hp_low} -> {hp_healed})");

    // Sanity: phase 1 must actually be a positive baseline, otherwise the
    // final comparison below could pass vacuously (0 < non-positive * 0.1).
    assert!(r_phase1 > 0.0, "phase 1 should have earned a positive reward for fresh damage, got {r_phase1}");

    // Phase 3: A re-deals the damage B just healed back. Under FRESH-damage
    // accounting this must pay (close to) NOTHING, because it does not push B
    // below B's episode minimum.
    let mut r_phase3 = 0.0f32;
    for _ in 0..10 {
        engage_row(&mut acts[0..6]);
        acts[6..12].copy_from_slice(&[0, 0, 0, 0, 0, 0]);
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 { break; }
        r_phase3 += rew[0];
        if env.agent_hp(1) <= hp_low { break; } // stop once we're back at fresh ground
    }
    let hp_after_phase3 = env.agent_hp(1);

    // Sanity: A must have actually re-dealt damage in phase 3 (re-taken B
    // below the healed-up level). If nothing was re-dealt, the test proves
    // nothing about double-paying for healed-back HP.
    assert!(
        hp_after_phase3 < hp_healed,
        "B should have been re-damaged in phase 3 (got {hp_healed} -> {hp_after_phase3}); \
         the test doesn't exercise the healing-farm path"
    );

    // Tight bound: under the (correct) fresh-damage logic, re-dealing HP that
    // was already paid for (and never pushing B below its episode minimum,
    // since B doesn't auto-retaliate so A takes ~nothing either) pays exactly
    // 0.0. A loose bound (e.g. < r_phase1 * 0.5) would also pass under a raw
    // dealt-taken regression whenever B doesn't fully heal back or the phase-3
    // tick cap limits re-dealing -- so it wouldn't prove anything. This bound
    // is tight enough that it FAILS under raw dealt-taken accounting (proven
    // separately) and PASSES under fresh-damage accounting.
    assert!(
        r_phase3 < r_phase1 * 0.1,
        "re-dealing healed-back damage paid {r_phase3} vs {r_phase1} in phase 1 -- \
         the healing farm is still open"
    );
}

/// Side B's hitpoints under `mirror_melee.ron`, i.e. `Duel::start_hp_b` -- and
/// therefore the hard ceiling on the FRESH damage one episode can ever pay for
/// (see `the_kill_dominates_a_whole_fights_dense_reward`).
const START_HP_B: f32 = 99.0;
/// The coefficients `cfg()` ships (and `BatchConfig`'s defaults). Named so the
/// domination assertion below reads as the inequality it is testing.
const DAMAGE_COEFF: f32 = 0.005;
const WIN_BONUS: f32 = 1.0;

#[test]
fn the_kill_dominates_a_whole_fights_dense_reward() {
    // The scaling intent: PuffeRL clamps each step's reward to [-1,1], so the
    // balance must come from the COEFFICIENTS -- the kill must outweigh
    // everything the dense damage term can pay across an ENTIRE episode.
    // Otherwise we train a poker, not a killer.
    //
    // ## What is measured, and what is a constant
    //
    // The dense side is MEASURED over a real, full-length fight. The terminal
    // side is `win_bonus`, read from the config: it is exactly what `step`'s
    // terminal branch pays for a kill you survive. The kill is not assumed --
    // this fixture produces a REAL one (see below), which the terminal-score
    // assertion at the bottom pins.
    //
    // ## Why the dense stream is measured with side B PACIFIED
    //
    // Agent 1's auto-retaliate is off and its action row is all-zero, so agent
    // 0 takes ZERO damage. That makes each step's reward exactly
    // `damage_coeff * fresh_dealt_by_a` -- the dense INCOME, with no damage-
    // taken term partially cancelling it. Summed over the episode it is
    // therefore exactly `damage_coeff * (total fresh HP dealt)`: the whole
    // pot a "poker" could farm in an episode, not a lower bound on it. The
    // pacification is asserted, not assumed (agent 0's HP must stay full).
    //
    // ## Why this is a real bound and not a tautology
    //
    // Fresh damage is capped by construction -- `fresh_on_b` is
    // `min_hp_b - hp_b` with `min_hp_b` monotonically decreasing from
    // `start_hp_b` -- so an episode can pay AT MOST
    // `damage_coeff * start_hp_b` = 0.005 * 99 = 0.495 of dense reward, versus
    // a win_bonus of 1.0. The test measures how close a genuine fight gets to
    // that ceiling and requires the win to still beat it. It is a live
    // constraint on the ratio: at `damage_coeff = 0.02` the same fight pays
    // ~1.8 and this goes RED.
    //
    // ## ★ The episode ends in a REAL clean win, and that is asserted
    //
    // A pacified side B is beaten from 99 HP to 0 and dies, with side A
    // untouched -- so this is the one fixture in the suite that observes the
    // full win path end to end, and the terminal assertion below pins
    // `scores[0] == 1.0` (the `b_dead && !a_dead` branch of `episode_score`).
    // That matters for more than the score: it proves `step`'s terminal
    // branch reached the `+win_bonus` arm at all, so the `WIN_BONUS >
    // dense_total` comparison is between two things that both really happen,
    // not between a measurement and an unreached constant.
    //
    // This used to be impossible: `EnvHarness` spawns players through
    // `Engine::accept_login`, which flags them `bot = false`, and the engine's
    // logout phase force-logged-out any non-bot player with no inbound packet
    // for `TIMEOUT_NO_RESPONSE` = 100 ticks. These players never send packets
    // (the arena drives the engine directly), so BOTH sides were removed at
    // ~tick 101 and `BatchEnv` read the two absent players as a mutual death --
    // every episode ended in a spurious ~101-tick double-KO, too short to chew
    // through 99 HP. That force-logout is now gated off in arena mode
    // (`rs-engine/src/phases/logout.rs`), so episodes resolve by combat.
    let mut env = BatchEnv::new(cfg(DAMAGE_COEFF));
    env.set_agent_auto_retaliate(1, false);
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    engage_row(&mut acts[0..6]);                       // A attacks
    acts[6..12].copy_from_slice(&[0, 0, 0, 0, 0, 0]);  // B idles (and can't retaliate)

    // Sum agent 0's DENSE reward over the whole episode, up to (but excluding)
    // the terminal step, whose reward mixes in the terminal coefficient.
    let mut dense_total = 0.0f32;
    let mut ended = false;
    let mut terminal_score = -1.0f32;
    let mut terminal_tick = 0usize;
    for t in 0..600 {
        // Pacification, asserted every tick: if B ever fights back, the sum
        // below stops being pure dense income and the whole measurement is
        // off. Checked BEFORE the step so it reads the live episode's players
        // (the terminal step auto-respawns the duel at full HP).
        assert_eq!(
            env.agent_hp(0), START_HP_B as u16,
            "agent 0 took damage -- side B is not pacified, so the dense sum is \
             no longer `damage_coeff * fresh dealt` and this test measures nothing"
        );
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        if done[0] == 1.0 {
            ended = true;
            terminal_score = scores[0];
            terminal_tick = t + 1;
            break;
        }
        assert!(
            rew[0] >= 0.0,
            "non-terminal dense reward {} went negative against a pacified opponent",
            rew[0]
        );
        dense_total += rew[0];
    }
    assert!(ended, "episode never terminated in 600 steps");

    // ★ The episode ended in a CLEAN WIN, not a timeout draw. `episode_score`
    // returns exactly 1.0 only on `b_dead && !a_dead`, so this single equality
    // pins: B actually died, A actually survived (the per-tick HP assertion
    // above already proved A was never even scratched), and `step` took the
    // `+win_bonus` arm of the terminal branch. If the arena force-logout gate
    // regressed, both players would vanish together and this would read a
    // double-KO's graded 0.99 instead; if the fight simply ran out of road it
    // would read the timeout's graded partial.
    assert_eq!(
        terminal_score, 1.0,
        "episode ended at tick {terminal_tick} with score {terminal_score}, not a clean \
         win (1.0) -- a pacified opponent must be killed, so the win path this test's \
         `win_bonus` constant stands in for never actually fired"
    );

    // Non-vacuity: a whole fight's worth of shaping must actually have been
    // accumulated. Killing a pacified 99 HP opponent pays for ~99 HP of fresh
    // damage, minus the killing blow (whose reward lands on the terminal step
    // and is excluded from the sum), so the floor is deliberately far below
    // what is expected: it guards against measuring a fight that never
    // happened, it is not the property under test.
    let fresh_hp = dense_total / DAMAGE_COEFF;
    assert!(
        fresh_hp > 30.0,
        "only {fresh_hp} HP of fresh damage was dealt across the episode -- that is \
         not 'a whole fight's dense reward', so the comparison below is vacuous"
    );
    // The capping invariant that makes the bound true in the first place. A
    // regression that paid for healed-back damage (the farm
    // `damage_the_opponent_eats_back_is_not_paid_twice` guards) would blow
    // through this ceiling.
    assert!(
        dense_total <= DAMAGE_COEFF * START_HP_B + 1e-4,
        "dense reward {dense_total} exceeded the structural ceiling \
         damage_coeff * start_hp_b = {} -- fresh-damage capping is broken",
        DAMAGE_COEFF * START_HP_B
    );
    assert!(
        WIN_BONUS > dense_total,
        "a kill pays {WIN_BONUS} but a whole episode of dense damage reward pays \
         {dense_total} ({fresh_hp} fresh HP) -- the kill does not dominate, so this \
         trains a poker, not a killer"
    );
}

/// ★ A real mutual fight is settled by ONE kill, and exactly ONE side is paid
/// for it.
///
/// # What replaced what, and why
///
/// This slot used to hold `a_double_ko_pays_neither_side_the_win_bonus`
/// (whole-branch review, Blocker 2: two independent `if`s in the terminal block
/// paid each side of a mutual kill `win_bonus - death_penalty` = +0.9, i.e. 90%
/// of a clean win for suiciding into the opponent). Its fixture was NOT a
/// mutual kill: the engine force-logged-out both non-bot arena players ~101
/// ticks after spawn, `BatchEnv` read the two absent players as `a_dead &&
/// b_dead`, and every episode "ended" that way. With that force-logout gated
/// off in arena mode (`rs-engine/src/phases/logout.rs`) episodes resolve by
/// real combat, one side dies first, and a genuine simultaneous double-KO is no
/// longer a producible fixture at all. The Blocker-2 property therefore moved
/// to `batch.rs`'s `terminal_reward_double_ko_pays_neither_side_the_win_bonus`
/// unit test, which pins it exactly and deterministically instead of riding on
/// an outcome the engine happens to deliver.
///
/// # What this live test proves that the unit test cannot
///
/// That `step` actually REACHES the terminal branch, with the configured
/// coefficients, on a real mutual fight -- the mirror-self-play case, where
/// both sides fight back (`the_kill_dominates_a_whole_fights_dense_reward`
/// only ever exercises a pacified opponent, so side B can never win there).
///
/// # Why it is discriminating and not vacuous
///
///   * ending before the scenario's 400-tick timeout rules out the timeout
///     branch (which pays `-timeout_penalty` to BOTH sides, so neither would
///     clear the bonus threshold);
///   * exactly one side clearing `+win_bonus`-scale reward while the other goes
///     negative can only be the solo-kill branch. Both sides clearing it is the
///     Blocker-2 regression; neither side clearing it means the win bonus never
///     got paid for a kill that happened.
///   * the score and the reward must AGREE about who won -- `episode_score`
///     returns 1.0 only for `b_dead && !a_dead`, so a terminal branch that paid
///     the bonus to the wrong side would be caught here even though both
///     streams individually look sane.
///
/// The `0.5` threshold is safe by a wide margin: the winner is paid
/// `win_bonus` (1.0) plus at most one tick of dense shaping (`|damage_coeff *
/// hit| < 0.2`), the loser `-death_penalty` (-0.1) plus the same.
#[test]
fn a_real_fight_is_settled_by_one_kill_and_pays_exactly_one_winner() {
    let mut env = BatchEnv::new(cfg(0.005)); // win_bonus 1.0, death_penalty 0.1
    let na = env.num_agents();
    let mut obs = vec![0.0f32; na * BatchEnv::OBS_STRIDE];
    let mut rew = vec![0.0f32; na];
    let mut done = vec![0.0f32; na];
    let mut scores = vec![-1.0f32; env.num_duels()];
    let mut acts = vec![0i32; na * BatchEnv::ACT_STRIDE];
    for a in 0..na { engage_row(&mut acts[a * 6..a * 6 + 6]); }

    let mut ticks = 0usize;
    let mut terminal = None;
    for _ in 0..600 {
        env.step(&acts, &mut obs, &mut rew, &mut done, &mut scores);
        ticks += 1;
        if done[0] == 1.0 { terminal = Some((rew[0], rew[1], scores[0])); break; }
    }
    let (ra, rb, score) = terminal.expect("fight never terminated in 600 ticks");
    assert!(
        ticks < 400,
        "episode ended at tick {ticks} -- that is the scenario's timeout, not a \
         death, so no kill was paid for and this test measures nothing"
    );

    // Exactly one winner. Never both (the Blocker-2 regression), never neither
    // (a kill that went unpaid).
    let winners: Vec<&str> = [("A", ra), ("B", rb)]
        .iter()
        .filter(|(_, r)| *r >= 0.5)
        .map(|(who, _)| *who)
        .collect();
    assert_eq!(
        winners.len(), 1,
        "expected exactly one side to be paid the win bonus after a solo kill, got \
         {winners:?} (A {ra}, B {rb}) at tick {ticks}"
    );
    // ...and the loser is paid a death, not merely "less".
    let (loser, r_loser) = if winners[0] == "A" { ("B", rb) } else { ("A", ra) };
    assert!(
        r_loser < 0.0,
        "side {loser} died but was paid {r_loser} >= 0 (A {ra}, B {rb})"
    );

    // The reward-independent score must name the SAME winner as the reward.
    let a_won_by_score = score == 1.0;
    let a_won_by_reward = winners[0] == "A";
    assert_eq!(
        a_won_by_score, a_won_by_reward,
        "score {score} says A {} but the rewards (A {ra}, B {rb}) say A {} -- the \
         terminal reward and the sweep objective disagree about who won",
        if a_won_by_score { "won" } else { "did not win" },
        if a_won_by_reward { "won" } else { "did not win" },
    );
}
