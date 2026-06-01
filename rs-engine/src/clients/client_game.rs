use rs_crypto::isaac::{Isaac, IsaacPair};
use rs_io::Packet;
use std::collections::VecDeque;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedReceiver, UnboundedSender};

/// Maximum number of inbound packet messages that can be buffered before
/// back-pressure is applied by the channel.
const INBOX_CAPACITY: usize = 128;

/// Channel capacity for the disconnect signal (only one signal is ever sent).
const DISCONNECT_CAPACITY: usize = 1;

/// The engine-side handle for a single game client connection.
///
/// Holds the decoded packet inbox, the outbound byte sender, ISAAC cipher
/// state for opcode encryption/decryption, and an intermediate read queue
/// for reassembling fragmented messages.
pub struct ClientHandle {
    pub inbox: Receiver<Vec<u8>>,
    pub outbox: UnboundedSender<Vec<u8>>,
    pub recycle_rx: UnboundedReceiver<Vec<u8>>,
    pub buffer_pool: Vec<Vec<u8>>,
    pub write_queue: Packet,
    pub read_queue: VecDeque<u8>,
    pub pending_msg: Option<Vec<u8>>,
    pub isaac_encode: Isaac,
    pub isaac_decode: Isaac,
    pub disconnect_rx: Receiver<()>,
}

/// Bundles the engine-side [`ClientHandle`] together with the network-side
/// channel endpoints needed by the I/O task to relay packets and detect
/// disconnection.
pub struct ClientIO {
    pub handle: ClientHandle,
    pub packet_tx: Sender<Vec<u8>>,
    pub bytes_rx: UnboundedReceiver<Vec<u8>>,
    pub recycle_tx: UnboundedSender<Vec<u8>>,
    pub disconnect_tx: Sender<()>,
}

/// Creates the paired I/O channels for a new game client connection.
///
/// Returns a [`ClientIO`] containing:
/// * A [`ClientHandle`] (engine side) with the packet inbox receiver, byte
///   outbox sender, a 5000-byte write queue, and the ISAAC cipher pair.
/// * The corresponding network-side senders/receivers (`packet_tx`,
///   `bytes_rx`, `disconnect_tx`) that the socket task uses.
///
/// # Arguments
/// * `isaac` - The ISAAC cipher pair negotiated during the login handshake,
///   used for opcode encoding and decoding.
///
/// # Returns
/// A fully wired [`ClientIO`] ready to be split between the engine and the
/// network I/O task.
pub fn create_io(isaac: IsaacPair) -> ClientIO {
    let (isaac_encode, isaac_decode) = (isaac.encode, isaac.decode);
    let (packet_tx, packet_rx) = mpsc::channel(INBOX_CAPACITY);
    let (bytes_tx, bytes_rx) = mpsc::unbounded_channel();
    let (recycle_tx, recycle_rx) = mpsc::unbounded_channel();
    let (disconnect_tx, disconnect_rx) = mpsc::channel(DISCONNECT_CAPACITY);

    ClientIO {
        handle: ClientHandle {
            inbox: packet_rx,
            outbox: bytes_tx,
            recycle_rx,
            buffer_pool: Vec::new(),
            write_queue: Packet::new(5000),
            read_queue: VecDeque::new(),
            pending_msg: None,
            isaac_encode,
            isaac_decode,
            disconnect_rx,
        },
        packet_tx,
        bytes_rx,
        recycle_tx,
        disconnect_tx,
    }
}
