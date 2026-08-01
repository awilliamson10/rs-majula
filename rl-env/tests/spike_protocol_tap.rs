//! SPIKE S1 (throwaway): can we tap the real client protocol for a
//! socket-less, headlessly-spawned player, WITHOUT modifying `rs-engine`?
//!
//! G1's design rests on two claims:
//!   (a) OUTBOUND — `Engine::process_output` encodes the full per-tick packet
//!       stream for a bot player, so the decoded stream can serve as the
//!       agent's observation.
//!   (b) INBOUND — pushing real client packets into the player's inbox drives
//!       the engine's genuine client-message handlers, so the inbound
//!       vocabulary can serve as the action space.
//!
//! Run single-threaded (process-global pathfinder state).

use rl_env::EnvHarness;
use rs_crypto::isaac::{Isaac, IsaacPair};
use rs_grid::CoordGrid;

/// The ISAAC seed `Engine::spawn_player` hardcodes for fabricated bot handles.
/// A virtual client constructs the identical pair and stays in lockstep.
fn bot_isaac() -> IsaacPair {
    IsaacPair::new(&[0; 4], &[0; 4])
}

/// rev-274 `ClientProt::MoveGameClick` (`rs-protocol/.../client_prot.rs:523`).
/// Opcodes are REV-SCOPED — a virtual client must resolve them through the
/// same table, never hardcode them.
const MOVE_GAMECLICK: u8 = 207;

/// rev-274 `ServerProt` opcodes (`rs-protocol/.../server_prot.rs`). Also
/// rev-scoped.
const PLAYER_INFO: u8 = 167;
const NPC_INFO: u8 = 197;

#[test]
fn outbound_stream_is_tappable_for_a_headless_player() {
    let mut env = EnvHarness::boot_arena();
    let pid = env.engine.spawn_player("tap", CoordGrid::new(3200, 0, 3912));

    // Swap in an outbox we hold the receiving end of. `spawn_player` drops the
    // network-side `ClientIO`, so the stock outbox sends into a closed channel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    {
        let p = env.engine.get_player_mut(pid).unwrap();
        p.handle.outbox = tx;
        // `accept_login` already flushed the whole login sequence into the
        // dropped channel, advancing the server's ISAAC stream by an unknown
        // amount. Re-seat both sides so the tap starts in lockstep.
        p.handle.isaac_encode = Isaac::new(&[0; 4]);
    }
    let mut isaac_encode = Isaac::new(&[0; 4]);

    let mut packets = 0usize;
    let mut bytes = 0usize;
    let mut opcodes = Vec::new();
    for _ in 0..5 {
        env.engine.cycle();
        while let Ok(buf) = rx.try_recv() {
            let opcode = (buf[0] as u32).wrapping_sub(isaac_encode.next_int()) as u8;
            opcodes.push(opcode);
            bytes += buf.len();
            packets += 1;
        }
    }

    println!("SPIKE-S1a packets={packets} bytes={bytes}");
    println!("SPIKE-S1a opcodes={opcodes:?}");

    assert!(
        packets > 0,
        "no outbound packets reached the tap -- process_output does not encode \
         for socket-less players, so G1 needs a sink inside rs-engine"
    );
    // The real discriminator: not "bytes arrived" but "the bytes are the
    // client's actual per-tick feed". PlayerInfo is sent to every player on
    // every tick, so 5 cycles must yield exactly 5 -- and recovering that
    // opcode at all proves the ISAAC mirror is in lockstep.
    let player_info = opcodes.iter().filter(|&&o| o == PLAYER_INFO).count();
    let npc_info = opcodes.iter().filter(|&&o| o == NPC_INFO).count();
    println!("SPIKE-S1a player_info={player_info} npc_info={npc_info}");
    assert_eq!(
        player_info, 5,
        "expected exactly one PlayerInfo ({PLAYER_INFO}) per tick over 5 ticks; \
         got {player_info}. Opcodes: {opcodes:?}"
    );
    assert_eq!(
        npc_info, 5,
        "expected exactly one NpcInfo ({NPC_INFO}) per tick over 5 ticks; got {npc_info}"
    );
    // Negative control (already observed): without the `isaac_encode` re-seat
    // above, the same run recovered 153 uniformly-scattered opcodes containing
    // neither a stable 167 nor a stable 197. Exactly-5-of-each is therefore a
    // real discriminator for "the mirror is in lockstep", not a tautology.
}

#[test]
fn inbound_packets_drive_the_real_client_handlers() {
    let mut env = EnvHarness::boot_arena();
    let start = CoordGrid::new(3200, 0, 3912);
    let pid = env.engine.spawn_player("walk", start);

    // Swap in an inbox we hold the sending end of.
    let (tx, rx) = tokio::sync::mpsc::channel::<Vec<u8>>(128);
    env.engine.get_player_mut(pid).unwrap().handle.inbox = rx;

    let mut isaac_decode: Isaac = bot_isaac().decode;

    // MOVE_GAMECLICK, VarByte frame. Wire = [opcode+isaac][len:u8][payload].
    // Payload per `MoveGameClick::decode`: ctrl:g1, x:g2(BE), z:g2(BE).
    let dest = CoordGrid::new(3205, 0, 3912);
    let payload = [
        0u8, // ctrl
        (dest.x() >> 8) as u8,
        (dest.x() & 0xFF) as u8,
        (dest.z() >> 8) as u8,
        (dest.z() & 0xFF) as u8,
    ];
    let mut buf = Vec::with_capacity(2 + payload.len());
    buf.push((MOVE_GAMECLICK as u32).wrapping_add(isaac_decode.next_int()) as u8);
    buf.push(payload.len() as u8);
    buf.extend_from_slice(&payload);
    tx.try_send(buf).expect("inbox accepted the packet");

    for _ in 0..10 {
        env.engine.cycle();
    }

    let end = env.engine.get_player(pid).unwrap().player.pathing.coord;
    println!(
        "SPIKE-S1b start=({},{}) end=({},{}) dest=({},{})",
        start.x(),
        start.z(),
        end.x(),
        end.z(),
        dest.x(),
        dest.z()
    );
    assert_ne!(
        (end.x(), end.z()),
        (start.x(), start.z()),
        "the player never moved -- an inbound MOVE_GAMECLICK did not reach the \
         real client handler, so the action space cannot be the inbound vocabulary"
    );
}
