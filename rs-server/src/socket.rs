use crate::Socket;
use anyhow::bail;
use mpsc::{Sender, UnboundedReceiver, UnboundedSender};
use rand::RngExt;
use rs_crypto::isaac::IsaacPair;
use rs_engine::LoginRequest;
use rs_engine::{ClientIO, create_io};
use rs_io::Packet;
use rs_io::packet::RsaFrame;
use rs_protocol::{LoginResponse, LoginType};
use tokio::sync::mpsc;

/// Dispatch a connection based on the service byte.
pub async fn handshake(mut client: Socket) -> anyhow::Result<()> {
    let mut seed = Packet::new(8);
    seed.p4(rand::rng().random_range(0..0x00ffffff_u32) as i32);
    seed.p4(rand::rng().random_range(0..0xffffffff_u32) as i32);
    client.write(&seed.data).await?;

    let Some(permit) = client.guard.try_acquire(client.addr.ip()) else {
        let _ = client
            .write(&[LoginResponse::TooManyConnections as u8])
            .await;
        bail!("{} connection limit reached", client.addr);
    };

    match client.read().await? {
        None => bail!("no bytes"),
        Some(bytes) => {
            if bytes.is_empty() {
                bail!("empty bytes")
            };
            let mut buf = Packet::from(bytes);
            let login_type = LoginType::try_from(buf.g1())?;
            match login_type {
                LoginType::New | LoginType::Reconnect => {
                    let waiting = buf.g1(); // the amount of bytes in this login payload.
                    if waiting != buf.len().saturating_sub(buf.pos) as u8 {
                        let _ = client.write(&[LoginResponse::Rejected as u8]).await;
                        bail!("{} Not enough bytes to read", client.addr);
                    }
                    let version = buf.g1(); // the client version.
                    if version as u16 != client.version {
                        let _ = client.write(&[LoginResponse::RuneScapeUpdated as u8]).await;
                        bail!("{}: Invalid version: {}", client.addr, version)
                    }
                    let info = buf.g1();
                    let low_memory = (info & 0x1) != 0;
                    let crcs: Vec<i32> = (0..9).map(|_| buf.g4s()).collect();
                    if crcs
                        .iter()
                        .any(|x| !client.server_io.cache.crctable.contains(x))
                    {
                        let _ = client.write(&[LoginResponse::RuneScapeUpdated as u8]).await;
                        bail!(
                            "{}: Invalid crctable: {:?} : {:?}",
                            client.addr,
                            crcs,
                            client.server_io.cache.crctable
                        )
                    }
                    buf.rsadec(RsaFrame::Byte, client.server_io.rsa);
                    let magic = buf.g1();
                    if magic != 10 {
                        let _ = client.write(&[LoginResponse::Rejected as u8]).await;
                        bail!("{}: Invalid magic: {}", client.addr, magic)
                    }
                    let seed = [buf.g4s(), buf.g4s(), buf.g4s(), buf.g4s()];
                    let _ = buf.g4s(); // uid
                    let username = buf.gjstr(10);
                    if username.is_empty() || username.len() > 12 {
                        let _ = client
                            .write(&[LoginResponse::InvalidCredentials as u8])
                            .await;
                        bail!("{}: Invalid username: {}", client.addr, username)
                    }
                    let password = buf.gjstr(10);
                    if password.is_empty() || password.len() > 20 {
                        let _ = client
                            .write(&[LoginResponse::InvalidCredentials as u8])
                            .await;
                        bail!("{}: Invalid password", client.addr)
                    }

                    let IsaacPair { encode, decode } = IsaacPair::from_client_seeds(&seed);
                    let io = create_io(IsaacPair { encode, decode });
                    let ClientIO {
                        handle,
                        packet_tx,
                        bytes_rx,
                        recycle_tx,
                        disconnect_tx,
                    } = io;
                    if client
                        .server_io
                        .new_player_tx
                        .send(LoginRequest {
                            handle,
                            username: Box::from(username),
                            password: Box::from(password),
                            low_memory,
                            remote_addr: client.addr,
                            reconnect: matches!(login_type, LoginType::Reconnect),
                        })
                        .is_err()
                    {
                        bail!("{}: Invalid login request", client.addr)
                    }
                    network_loop(client, packet_tx, bytes_rx, recycle_tx, &disconnect_tx).await?;
                    drop(permit);
                }
            }
        }
    }
    Ok(())
}

#[allow(clippy::collapsible_match)]
pub async fn network_loop(
    mut client: Socket,
    packet_tx: Sender<Vec<u8>>,
    mut bytes_rx: UnboundedReceiver<Vec<u8>>,
    recycle_tx: UnboundedSender<Vec<u8>>,
    disconnect_tx: &Sender<()>,
) -> anyhow::Result<()> {
    let addr = client.addr;
    loop {
        tokio::select! {
            result = client.read() => match result {
                Ok(Some(bytes)) if !bytes.is_empty() => {
                    if packet_tx.try_send(bytes).is_err() {
                        disconnect_tx.send(()).await?;
                        bail!("{addr} inbox full or closed");
                    }
                }
                Ok(None) | Err(_) => {
                    disconnect_tx.send(()).await?;
                    bail!("{addr} disconnected");
                }
                _ => {}
            },
            msg = bytes_rx.recv() => match msg {
                Some(bytes) => {
                    // Return the drained buffer to the engine for reuse (TCP
                    // only; WebSocket consumes it). A send failure just means
                    // the engine dropped the client, so the buffer is freed.
                    if let Some(returned) = client.write_owned(bytes).await? {
                        let _ = recycle_tx.send(returned);
                    }
                }
                None => {
                    bail!("{addr} engine closed write channel");
                }
            },
        }
    }
}
