//! `hello`, then `pair` or `auth`, then a verified `welcome` (`docs/PROTOCOL.md` 5).

use swayconnect_protocol::limits::PROTOCOL_VERSION;
use swayconnect_protocol::proof::{
    Context, RandomError, client_proof, device_key, pair_proof, random_bytes, verify_server_proof,
};
use swayconnect_protocol::uri::PairingUri;
use swayconnect_protocol::{Auth, DeviceId, Message, Pair, Welcome};

use super::channel::Channel;
use super::error::ClientError;
use super::profile::Profile;
use super::session::{Event, Session};

/// Where `client_nonce` comes from. Tests inject the nonces of the conformance vectors.
pub type NonceSource = fn() -> Result<[u8; 32], RandomError>;

/// Opens connections. One value serves any number of them.
#[derive(Debug, Clone)]
pub struct Connector {
    client: Option<String>,
    nonce_source: NonceSource,
}

impl Connector {
    /// `client` is the informational `client` member of `pair` and `auth`.
    pub fn new(client: Option<String>) -> Connector {
        Connector {
            client,
            nonce_source: random_bytes::<32>,
        }
    }

    #[must_use]
    pub fn with_nonce_source(mut self, nonce_source: NonceSource) -> Connector {
        self.nonce_source = nonce_source;
        self
    }

    /// Runs the pairing flow up to the pending step (spec 5.4 steps 2 to 6).
    pub async fn pair(
        &self,
        uri: &PairingUri,
        device_name: &str,
        platform: Option<&str>,
    ) -> Result<PendingPairing, ClientError> {
        let (mut channel, context) = self.begin(&uri.host, uri.port, uri.fp).await?;
        let pair = Pair {
            protocol: PROTOCOL_VERSION,
            nonce: context.client_nonce,
            proof: pair_proof(&uri.secret, &context),
            device_name: device_name.to_owned(),
            platform: platform.map(str::to_owned),
            client: self.client.clone(),
        };
        channel.write_message(&Message::Pair(pair)).await?;
        let welcome = read_welcome(&mut channel).await?;
        let key = device_key(&uri.secret, &context);
        check_welcome(&welcome, &key, &context, &welcome.device_id)?;
        let confirm_code = welcome
            .confirm_code
            .clone()
            .ok_or(ClientError::BadWelcome("confirm_code"))?;
        let profile = Profile {
            host: uri.host.clone(),
            port: uri.port,
            fp: context.fp,
            device_id: welcome.device_id.clone(),
            key,
            server_name: welcome.server.name.clone(),
        };
        Ok(PendingPairing {
            session: Session::new(channel, welcome),
            confirm_code,
            profile,
        })
    }

    /// Authenticates with a stored profile (spec 5.5).
    pub async fn connect(&self, profile: &Profile) -> Result<Session, ClientError> {
        let (mut channel, context) = self.begin(&profile.host, profile.port, profile.fp).await?;
        let auth = Auth {
            protocol: PROTOCOL_VERSION,
            device_id: profile.device_id.clone(),
            nonce: context.client_nonce,
            proof: client_proof(&profile.key, &context, &profile.device_id),
            client: self.client.clone(),
        };
        channel.write_message(&Message::Auth(auth)).await?;
        let welcome = read_welcome(&mut channel).await?;
        check_welcome(&welcome, &profile.key, &context, &profile.device_id)?;
        Ok(Session::new(channel, welcome))
    }

    /// Connects, reads `hello` and settles the version. Sends nothing.
    async fn begin(
        &self,
        host: &str,
        port: u16,
        pinned: [u8; 32],
    ) -> Result<(Channel, Context), ClientError> {
        let (mut channel, fp) = Channel::open(host, port, pinned).await?;
        let Ok(Message::Hello(hello)) = Message::decode(&channel.read_frame().await?) else {
            return Err(ClientError::Protocol(
                "the first frame is not a valid `hello`",
            ));
        };
        let range = hello.protocol;
        if !(range.min..=range.max).contains(&PROTOCOL_VERSION) {
            return Err(ClientError::NoCommonVersion {
                min: range.min,
                max: range.max,
            });
        }
        let context = Context {
            fp,
            server_nonce: hello.nonce,
            client_nonce: (self.nonce_source)()?,
        };
        Ok((channel, context))
    }
}

async fn read_welcome(channel: &mut Channel) -> Result<Welcome, ClientError> {
    match Message::decode(&channel.read_frame().await?) {
        Ok(Message::Welcome(welcome)) => Ok(welcome),
        Ok(Message::Reject(reject)) => Err(ClientError::Rejected {
            code: reject.code,
            message: reject.message,
        }),
        Ok(_) | Err(_) => Err(ClientError::Protocol(
            "the answer to the first frame is not a valid `welcome` or `reject`",
        )),
    }
}

/// Spec 5.1 rule 4. The proof comes first: nothing else in `welcome` is trusted before it.
/// `device_id` is the id the proof must cover: the stored one after `auth`.
fn check_welcome(
    welcome: &Welcome,
    key: &[u8; 32],
    context: &Context,
    device_id: &DeviceId,
) -> Result<(), ClientError> {
    if !verify_server_proof(key, context, device_id, &welcome.server_proof) {
        return Err(ClientError::BadWelcome("server_proof"));
    }
    if welcome.device_id != *device_id {
        return Err(ClientError::BadWelcome("device_id"));
    }
    if welcome.protocol != PROTOCOL_VERSION {
        return Err(ClientError::BadWelcome("protocol"));
    }
    Ok(())
}

/// A pairing that the owner has not confirmed yet. The credential stays inside until
/// `pairing.confirmed` arrives; dropping this value discards it (spec 5.4 step 11).
pub struct PendingPairing {
    session: Session,
    confirm_code: String,
    profile: Profile,
}

impl PendingPairing {
    /// Six digits to show to the user, who compares them with the code on the desktop.
    pub fn confirm_code(&self) -> &str {
        &self.confirm_code
    }

    /// Waits for the owner's answer, keeping the connection alive. Returns the credential to
    /// store on yes. The caller then connects again with [`Connector::connect`].
    pub async fn confirmed(mut self) -> Result<Profile, ClientError> {
        while self.session.next_event().await? != Event::PairingConfirmed {}
        // The server has stored the device already, so a failed close must not lose the
        // credential.
        let _close_failure = self.session.close().await;
        Ok(self.profile)
    }
}
