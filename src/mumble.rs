use bytes::Bytes;
use futures::stream::SplitSink;
use futures::stream::SplitStream;
use futures::SinkExt;
use futures::StreamExt;
use mumble_protocol_2x::control::msgs;
use mumble_protocol_2x::control::ClientControlCodec;
use mumble_protocol_2x::control::ControlCodec;
use mumble_protocol_2x::control::ControlPacket;
use mumble_protocol_2x::voice::Clientbound;
use mumble_protocol_2x::voice::Serverbound;
use mumble_protocol_2x::voice::VoicePacket;
use mumble_protocol_2x::voice::VoicePacketPayload;
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::time::SystemTime;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time;
use tokio_native_tls::TlsConnector;
use tokio_util::codec::Decoder;
use tokio_util::codec::Framed;

#[derive(Error, Debug)]
pub enum Error {
    #[error("tokio io error: {0}")]
    Io(#[from] tokio::io::Error),
    #[error("native-tls error: {0}")]
    NativeTls(#[from] native_tls::Error),
    #[error("tokio mpsc send error: {0}")]
    MpscSend(#[from] mpsc::error::SendError<Voice>),
    #[error("failed to resolve address")]
    AddressResolution,
    #[error("connection closed")]
    ConnectionClosed,
}

pub struct Voice {
    pub user: String,
    pub audio: Bytes,
}

pub struct Client {
    write: SplitSink<
        Framed<tokio_native_tls::TlsStream<TcpStream>, ControlCodec<Serverbound, Clientbound>>,
        ControlPacket<Serverbound>,
    >,
    read: SplitStream<
        Framed<tokio_native_tls::TlsStream<TcpStream>, ControlCodec<Serverbound, Clientbound>>,
    >,
    user_by_session: HashMap<u32, String>,
}

impl Client {
    pub async fn new(
        server_host: String,
        server_port: u16,
        accept_invalid_cert: bool,
        user_name: String,
    ) -> Result<Self, Error> {
        let server_addr = (server_host.as_ref(), server_port)
            .to_socket_addrs()?
            .next()
            .ok_or(Error::AddressResolution)?;

        // let (crypt_state_sender, crypt_state_receiver) = oneshot::channel::<ClientCryptState>();

        // // Wrap crypt_state_sender in Option, so we can call it only once
        // let mut crypt_state_sender = Some(crypt_state_sender);

        let stream = TcpStream::connect(&server_addr).await?;
        println!("TCP connected..");

        // Wrap the connection in TLS
        let mut builder = native_tls::TlsConnector::builder();
        builder.danger_accept_invalid_certs(accept_invalid_cert);
        let connector: TlsConnector = builder.build()?.into();
        let tls_stream = connector.connect(&server_host, stream).await?;
        println!("TLS connected..");

        let (mut write, read) = ClientControlCodec::new().framed(tls_stream).split();

        // rust-mumble-protocol currently not supporting protobuf UDP (1.5.0)
        let mut ver_msg = msgs::Version::new();
        let version = 1 << 16 | 4 << 8 | 0;
        ver_msg.set_version_v2(version);
        write.send(ver_msg.into()).await?;

        let mut auth_msg = msgs::Authenticate::new();
        auth_msg.set_username(user_name);
        auth_msg.set_opus(true);
        write.send(auth_msg.into()).await?;

        Ok(Self {
            write,
            read,
            user_by_session: HashMap::new(),
        })
    }

    pub async fn run(&mut self, sender: mpsc::Sender<Voice>) -> Result<(), Error> {
        let mut ping_interval = time::interval(time::Duration::from_secs(20));
        loop {
            select! {
                _ = ping_interval.tick() => {
                    let mut ping_msg = msgs::Ping::new();
                    let timestamp = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                        Ok(n) => n.as_secs(),
                        Err(_) => 0u64,
                    };
                    ping_msg.set_timestamp(timestamp);
                    self.write.send(ping_msg.into()).await?;
                },
                msg = self.read.next() => {
                    match msg {
                        Some(result) => {
                            match result? {
                                ControlPacket::UDPTunnel(packet) => {
                                    match *packet {
                                        VoicePacket::Ping { timestamp } => {
                                            println!("receive ping {}", timestamp);
                                        }
                                        VoicePacket::Audio { session_id, seq_num, payload, .. } => {
                                            println!("receive audio {} {}", session_id, seq_num);
                                            match payload {
                                                VoicePacketPayload::Opus(audio, end) => {
                                                    if end {
                                                        println!("end of transmission!");
                                                    }
                                                    let user = self.user_by_session.get(&session_id).map(|u| u.clone()).unwrap_or("unknown".to_string());
                                                    sender.send(Voice { user, audio }).await?;
                                                }
                                                _ => {
                                                    println!("unsupported voice packet");
                                                }
                                            }
                                        }
                                    };
                                }
                                ControlPacket::UserState(packet) => {
                                    match (packet.session, packet.name) {
                                        (Some(session), Some(name)) => {
                                            self.user_by_session.insert(session, name);
                                        }
                                        _ => {
                                            println!("incomplete user state");
                                        }
                                    }
                                }
                                packet => {
                                    println!("receive {:?}", packet);
                                }
                            };
                        }
                        None => {
                            return Err(Error::ConnectionClosed);
                        }
                    }
                },
            }
        }
    }
}
