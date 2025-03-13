use bytes::Bytes;
use core::slice::SlicePattern;
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
use opus::Channels;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::SystemTime;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::select;
use tokio::sync::mpsc;
use tokio::time;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::client::danger::HandshakeSignatureValid;
use tokio_rustls::rustls::client::danger::ServerCertVerified;
use tokio_rustls::rustls::client::danger::ServerCertVerifier;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::ClientConfig;
use tokio_rustls::rustls::SignatureScheme;
use tokio_rustls::TlsConnector;
use tokio_util::codec::Decoder;
use tokio_util::codec::Framed;

#[derive(Error, Debug)]
pub enum Error {
    #[error("tokio io error: {0}")]
    Io(#[from] tokio::io::Error),
    #[error("tokio mpsc send error: {0}")]
    MpscSend(#[from] mpsc::error::TrySendError<Voice>),
    #[error("invalid dns name error: {0}")]
    InvalidDnsName(#[from] tokio_rustls::rustls::pki_types::InvalidDnsNameError),
    #[error("opus error: {0}")]
    Opus(#[from] opus::Error),
    #[error("failed to resolve address")]
    AddressResolution,
    #[error("connection closed")]
    ConnectionClosed,
}

#[derive(Debug)]
struct NoCertificateVerification;

impl ServerCertVerifier for NoCertificateVerification {
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }

    fn verify_server_cert(
        &self,
        _: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: tokio_rustls::rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, tokio_rustls::rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, tokio_rustls::rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn requires_raw_public_keys(&self) -> bool {
        false
    }

    fn root_hint_subjects(&self) -> Option<&[tokio_rustls::rustls::DistinguishedName]> {
        None
    }
}

const SAMPLE_RATE: u32 = 48000;
const MAX_AUDIO_MILLISEC: usize = 60;
const CHANNEL_COUNT: Channels = Channels::Mono;

const AUDIO_PAYLOAD_UNIT_MILLISEC: u32 = 10;
const AUDIO_PAYLOAD_N: u32 = 2;

pub struct Voice {
    pub user: String,
    pub sample_rate: u32,
    pub audio: Vec<i16>,
}

struct AudioEncoder {
    encoder: opus::Encoder,
    send_buffer: Vec<i16>,
    seq_num: u64,
    packet_duration: u32,
    packet_samples: usize,
    audio_encoded: Vec<u8>,
}

impl AudioEncoder {
    fn new(sample_rate: u32) -> Result<Self, Error> {
        let packet_duration = AUDIO_PAYLOAD_UNIT_MILLISEC * AUDIO_PAYLOAD_N;
        let packet_samples = (sample_rate * packet_duration / 1000) as usize;
        Ok(Self {
            encoder: opus::Encoder::new(
                sample_rate,
                opus::Channels::Mono,
                opus::Application::Voip,
            )?,
            send_buffer: Vec::<i16>::new(),
            seq_num: 1,
            packet_duration,
            packet_samples,
            audio_encoded: vec![0; packet_samples],
        })
    }

    fn is_empty(&self) -> bool {
        self.send_buffer.is_empty()
    }

    fn push(&mut self, data: &[i16]) {
        self.send_buffer.extend_from_slice(data);
    }

    fn next_packet(&mut self) -> Result<ControlPacket<Serverbound>, Error> {
        if self.send_buffer.len() < self.packet_samples {
            self.send_buffer.resize(self.packet_samples, 0);
        }
        let frame = &self.send_buffer[..self.packet_samples];
        let encoded_len = self.encoder.encode(frame, &mut self.audio_encoded)?;

        let is_end = self.send_buffer.len() <= self.packet_duration as usize;
        let payload = VoicePacketPayload::Opus(
            Bytes::copy_from_slice(&self.audio_encoded[..encoded_len]),
            is_end,
        );
        let audio = VoicePacket::Audio {
            _dst: PhantomData,
            target: 0,
            session_id: (),
            seq_num: self.seq_num,
            payload,
            position_info: None,
        };
        let packet = ControlPacket::UDPTunnel(Box::new(audio));

        self.send_buffer.drain(..self.packet_samples);
        self.seq_num += AUDIO_PAYLOAD_N as u64;

        Ok(packet)
    }
}

pub struct Client {
    write: SplitSink<
        Framed<TlsStream<TcpStream>, ControlCodec<Serverbound, Clientbound>>,
        ControlPacket<Serverbound>,
    >,
    read: SplitStream<Framed<TlsStream<TcpStream>, ControlCodec<Serverbound, Clientbound>>>,
    user_by_session: HashMap<u32, String>,
    current_session: Option<u32>,
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

        // https://github.com/rustls/rustls/issues/1938
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(config));
        let domain = server_host.try_into()?;
        let tls_stream = connector.connect(domain, stream).await?;

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
            current_session: None,
        })
    }

    async fn receive_audio(
        &mut self,
        sender: &mpsc::Sender<Voice>,
        session_id: u32,
        seq_num: u64,
        payload: VoicePacketPayload,
    ) -> Result<(), Error> {
        println!("receive audio {} {}", session_id, seq_num);
        let mut decoder = opus::Decoder::new(SAMPLE_RATE, CHANNEL_COUNT)?;
        const BUFFER_SIZE: usize =
            (SAMPLE_RATE as usize) * MAX_AUDIO_MILLISEC / 1000 * (CHANNEL_COUNT as usize);
        let mut output = [0i16; BUFFER_SIZE];
        match payload {
            VoicePacketPayload::Opus(audio, end) => {
                if end {
                    println!("end of transmission!");
                }
                let user = self
                    .user_by_session
                    .get(&session_id)
                    .map(|u| u.clone())
                    .unwrap_or("unknown".to_string());
                let size = decoder.decode(audio.as_slice(), &mut output, false)?;
                sender
                    .try_send(Voice {
                        user,
                        sample_rate: SAMPLE_RATE,
                        audio: output[..size].to_vec(),
                    })
                    .or_else(|e| match e {
                        mpsc::error::TrySendError::Full(_) => {
                            println!("voice queue is full");
                            Ok(())
                        }
                        e => Err(e),
                    })?;
            }
            _ => {
                println!("unsupported voice packet");
            }
        }
        Ok(())
    }

    pub async fn run(
        &mut self,
        sender: mpsc::Sender<Voice>,
        receiver: &mut mpsc::Receiver<Voice>,
    ) -> Result<(), Error> {
        let mut encoder = AudioEncoder::new(24000)?;

        let mut ping_interval = time::interval(time::Duration::from_secs(20));
        let mut send_interval = time::interval(time::Duration::from_millis(
            (AUDIO_PAYLOAD_UNIT_MILLISEC * AUDIO_PAYLOAD_N) as u64,
        ));

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
                }
                _ = send_interval.tick() => {
                    if !encoder.is_empty() {
                        self.write.send(encoder.next_packet()?).await?;
                    }
                }
                Some(voice) = receiver.recv() => {
                    encoder.push(&voice.audio);
                    for _ in 0..5 {
                        if encoder.is_empty() {
                            break;
                        }
                        self.write.send(encoder.next_packet()?).await?;
                    }
                }
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
                                            self.receive_audio(&sender, session_id, seq_num, payload).await?;
                                        }
                                    };
                                }
                                ControlPacket::UserState(packet) => {
                                    if let (Some(session), Some(name)) = (packet.session, packet.name) {
                                        self.user_by_session.insert(session, name);
                                    } else {
                                        println!("incomplete user state");
                                    }
                                }
                                ControlPacket::ServerSync(packet) => {
                                    if let Some(session) = packet.session {
                                        self.current_session = Some(session);
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
                }
            }
        }
    }
}
