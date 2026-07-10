use rl_env::EnvHarness;
use rs_grid::CoordGrid;

#[test]
fn player_damages_npc_in_melee() {
    let mut env = EnvHarness::boot_arena();
    let pcoord = CoordGrid::new(3200, 0, 3912);
    let pid = env.engine.spawn_player("attacker", pcoord);
    env.buff_melee(pid);
    // adjacent tile
    let ncoord = CoordGrid::new(3201, 0, 3912);
    let nid = env
        .engine
        .spawn_npc(/* cow */ 81, ncoord)
        .expect("npc")
        .nid();
    env.cycle(); // build_area / visibility settles
    let hp0 = env.npc_hp(nid);
    env.attack_npc(pid, nid);
    // 40 ticks (not 12): the arena-mode AFK-gate fix (see
    // `rs-engine/src/phases/input.rs::check_afk`) removed the ONLY
    // absolute-clock-gated draw from `engine.random` in arena mode, which
    // shifts every subsequent accuracy roll's position in the RNG stream
    // relative to the pre-fix behavior this test's window was tuned
    // against. A short, RNG-stream-position-sensitive window is exactly
    // the kind of fragility that fix was meant to eliminate, so widen the
    // window instead of re-coupling the test to a specific stream offset.
    for _ in 0..40 {
        env.cycle();
    }
    let hp1 = env.npc_hp(nid);
    assert!(hp1 < hp0, "npc HP should drop: {hp0} -> {hp1}");
}

#[test]
fn player_damages_player_in_deep_wilderness() {
    let mut env = EnvHarness::boot_arena();
    // Both in the Scorpion Valley multi-combat wilderness zone (deep wild ⇒
    // level-difference check trivially passes; multiway ⇒ in-combat check
    // passes). Confirmed against content/274/maps/multiway.csv block
    // 0_50_61_0_8 (mapsquare 50,61, local zone x0-7,y8-15 covers both tiles)
    // and content/274/scripts/areas/area_wilderness/configs/wilderness_zones.dbrow
    // (main wilderness coord_pair covers z0, x2944-3391, y3520-6399; this
    // tile computes to wilderness level 50).
    let a_coord = CoordGrid::new(3200, 0, 3912);
    let b_coord = CoordGrid::new(3201, 0, 3912); // adjacent
    let a = env.engine.spawn_player("pker", a_coord);
    let b = env.engine.spawn_player("victim", b_coord);
    // Buff BOTH: equal combat levels make pvp_level_check's level-difference
    // gate trivially pass at any wilderness depth, and 99 HP on the victim
    // (XP-consistent -- see `EnvHarness::buff_melee`) means it survives long
    // enough (doesn't die/respawn, which would reset HP and mask the
    // result) to show a clear, genuine HP drop.
    env.buff_melee(a);
    env.buff_melee(b);
    env.cycle(); // both become visible to each other (build_area)

    let hp0 = env.player_hp(b);

    // Re-inject the attack interaction every tick, mirroring a real client
    // holding down the attack: a single `attack_player` injection does not
    // reliably keep the interaction armed across the attacker's own attack
    // cooldown in this headless harness (confirmed by a per-tick
    // instrumented trace -- the attacker's interaction target did not
    // survive past the first cooldown window, so it only ever swung once).
    // Re-injecting is idempotent/safe: the combat script's own
    // `%action_delay > map_clock` guard (`pvp_melee.rs2`) still governs the
    // real attack cadence (unarmed attack speed = 4 ticks), so this cannot
    // double-count hits within a single cooldown window -- it only
    // guarantees the attacker keeps *trying* to swing, exactly like a
    // player holding the attack option down.
    //
    // 60 ticks gives ~15 real attack attempts at the 4-tick unarmed
    // cooldown. Unarmed accuracy/max-hit at 99 vs 99 is roughly a coin
    // flip with a small max hit, so this is comfortably enough attempts
    // for at least one real hit to land under the engine's fixed/seeded
    // RNG (deterministic once observed to pass).
    let mut hp_trace = Vec::with_capacity(61);
    hp_trace.push(hp0);
    for _ in 0..60 {
        env.attack_player(a, b);
        env.cycle();
        hp_trace.push(env.player_hp(b));
    }
    let hp1 = env.player_hp(b);

    // Genuine-evidence assertions (post XP-consistency fix, the only way
    // `player_hp(b)` can drop is a real `ActivePlayer::damage()` call from
    // landed PvP melee combat -- regen only heals, and the `rs_stat`
    // "snap" desync that produced the prior artifact is a no-op now that
    // `buff_melee` backs level 99 with real level-99 xp):
    //
    // 1. The victim must have taken damage at all.
    assert!(hp1 < hp0, "★ PvP damage must land: victim HP {hp0} -> {hp1}");
    // 2. The drop must be a realistic melee-combat magnitude across ~15
    //    attack attempts (a handful of small unarmed hits), not a
    //    near-total-HP artifact like the prior 99->10 (89) desync.
    let total_drop = hp0 - hp1;
    assert!(
        (1..=40).contains(&total_drop),
        "victim HP drop should be a realistic cumulative melee magnitude, not an artifact: \
         dropped {total_drop} ({hp0} -> {hp1})"
    );
    // 3. The drop must be the accumulation of one-or-more discrete combat
    //    hits over the episode, not a single instantaneous snap: walk the
    //    per-tick trace and confirm every observed decrease is itself a
    //    plausible single-hit magnitude (unarmed max hit at 99 str is
    //    small; a legitimate hit is well under the 89-point snap that
    //    produced the original artifact).
    let mut hits = 0usize;
    for w in hp_trace.windows(2) {
        let (prev, cur) = (w[0], w[1]);
        if cur < prev {
            let hit = prev - cur;
            assert!(
                hit <= 15,
                "single-tick HP delta {hit} is too large for a real unarmed hit \
                 (looks like a desync snap, not combat damage)"
            );
            hits += 1;
        }
    }
    assert!(
        hits >= 1,
        "expected at least one discrete combat hit in the per-tick trace"
    );
}
