use crate::{core::*, model::NodeSecret, realtime::AgentLink};
use base64::{Engine, engine::general_purpose::STANDARD};
use russh::{
    ChannelMsg, client,
    keys::{
        Algorithm, HashAlg, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate,
    },
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    time::timeout,
};
use zeroize::Zeroizing;
pub type Client = client::Handle<Verifier>;
pub struct Verifier {
    pub expected: Option<Vec<u8>>,
    pub inspected: Arc<Mutex<Option<PublicKey>>>,
}
impl client::Handler for Verifier {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        if key.certificate().is_some() {
            return Ok(false);
        }
        let public = key.public_key();
        let bytes = public.to_bytes().map_err(|_| russh::Error::UnknownKey)?;
        *self.inspected.lock().unwrap() = Some(public);
        Ok(self.expected.as_ref().is_some_and(|v| v == &bytes))
    }
}
fn algorithms(key: Option<&PublicKey>) -> Vec<Algorithm> {
    if let Some(k) = key {
        if matches!(k.algorithm(), Algorithm::Rsa { .. }) {
            vec![
                Algorithm::Rsa {
                    hash: Some(HashAlg::Sha512),
                },
                Algorithm::Rsa {
                    hash: Some(HashAlg::Sha256),
                },
            ]
        } else {
            vec![k.algorithm()]
        }
    } else {
        vec![
            Algorithm::Ed25519,
            Algorithm::Ecdsa {
                curve: russh::keys::EcdsaCurve::NistP256,
            },
            Algorithm::Ecdsa {
                curve: russh::keys::EcdsaCurve::NistP384,
            },
            Algorithm::Ecdsa {
                curve: russh::keys::EcdsaCurve::NistP521,
            },
            Algorithm::Rsa {
                hash: Some(HashAlg::Sha512),
            },
            Algorithm::Rsa {
                hash: Some(HashAlg::Sha256),
            },
        ]
    }
}
pub async fn connect<T: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: T,
    pin: Option<&str>,
    inspected: Arc<Mutex<Option<PublicKey>>>,
) -> Result<Client, &'static str> {
    let raw = pin
        .map(|p| STANDARD.decode(p).map_err(|_| "host key invalid"))
        .transpose()?;
    let key = raw
        .as_ref()
        .map(|v| PublicKey::from_bytes(v).map_err(|_| "host key invalid"))
        .transpose()?;
    let mut cfg = client::Config {
        window_size: 256 * 1024,
        maximum_packet_size: 32 * 1024,
        channel_buffer_size: 16,
        inactivity_timeout: Some(Duration::from_secs(660)),
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        ..Default::default()
    };
    cfg.preferred.key = algorithms(key.as_ref()).into();
    timeout(
        Duration::from_secs(12),
        client::connect_stream(
            Arc::new(cfg),
            stream,
            Verifier {
                expected: raw,
                inspected,
            },
        ),
    )
    .await
    .map_err(|_| "SSH connection timeout")?
    .map_err(|_| "SSH handshake failed")
}
pub async fn inspect(address: std::net::SocketAddr) -> Result<PublicKey, &'static str> {
    let observed = Arc::new(Mutex::new(None));
    let stream = timeout(
        Duration::from_secs(8),
        tokio::net::TcpStream::connect(address),
    )
    .await
    .map_err(|_| "SSH connection timeout")?
    .map_err(|_| "SSH connection failed")?;
    let _ = timeout(
        Duration::from_secs(8),
        connect(stream, None, observed.clone()),
    )
    .await;
    let key = observed.lock().unwrap().clone();
    key.ok_or("SSH host key unavailable")
}
pub async fn agent_client(
    app: &App,
    link: Arc<AgentLink>,
    node_id: &str,
    user: &str,
    secret: &NodeSecret,
) -> Result<Arc<Client>, &'static str> {
    let raw = app.unseal(&format!("{node_id}:ssh"), &secret.ssh_key)?;
    let key = PrivateKey::from_openssh(raw.as_slice()).map_err(|_| "SSH identity invalid")?;
    // Managed identities have always been Ed25519. RSA support is used only to verify public host signatures.
    // No RSA private-key signing or decryption can be reached through this application.
    if key.algorithm() != Algorithm::Ed25519 {
        return Err("unsupported managed SSH identity");
    }
    let stream = link.open().await?;
    let mut client = connect(stream, Some(&secret.host_key), Arc::new(Mutex::new(None))).await?;
    let ok = timeout(
        Duration::from_secs(10),
        client.authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), None)),
    )
    .await
    .map_err(|_| "SSH authentication timeout")?
    .map_err(|_| "SSH authentication failed")?;
    if !ok.success() {
        return Err("SSH authentication failed");
    }
    Ok(Arc::new(client))
}
pub async fn password_client(
    address: std::net::SocketAddr,
    user: &str,
    password: &str,
    pin: &str,
) -> Result<Arc<Client>, &'static str> {
    let stream = timeout(
        Duration::from_secs(12),
        tokio::net::TcpStream::connect(address),
    )
    .await
    .map_err(|_| "SSH connection timeout")?
    .map_err(|_| "SSH connection failed")?;
    let mut client = connect(stream, Some(pin), Arc::new(Mutex::new(None))).await?;
    let ok = timeout(
        Duration::from_secs(10),
        client.authenticate_password(user, password),
    )
    .await
    .map_err(|_| "SSH authentication timeout")?
    .map_err(|_| "SSH authentication failed")?;
    if !ok.success() {
        return Err("SSH authentication failed");
    }
    Ok(Arc::new(client))
}
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub async fn exec(
    client: &Client,
    command: &str,
    input: &[u8],
    limit: usize,
) -> Result<String, &'static str> {
    let channel = client
        .channel_open_session()
        .await
        .map_err(|_| "SSH channel unavailable")?;
    channel
        .exec(true, command)
        .await
        .map_err(|_| "SSH command unavailable")?;
    let (mut reader, writer) = channel.split();
    let feed = async {
        if !input.is_empty() {
            writer.data(input).await.map_err(|_| "SSH input failed")?;
        }
        writer.eof().await.map_err(|_| "SSH input failed")
    };
    let receive = async {
        let mut out = Vec::new();
        let mut status = None;
        let mut accepted = false;
        while let Some(m) = reader.wait().await {
            match m {
                ChannelMsg::Success => accepted = true,
                ChannelMsg::Failure => return Err("SSH command rejected"),
                ChannelMsg::Data { data } | ChannelMsg::ExtendedData { data, .. } => {
                    if out.len() + data.len() > limit {
                        return Err("SSH output limit reached");
                    }
                    out.extend(data.as_ref());
                }
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        if status != Some(0) || !accepted {
            return Err("remote command failed");
        }
        String::from_utf8(out).map_err(|_| "remote output invalid")
    };
    let result = tokio::try_join!(feed, receive).map(|(_, output)| output);
    let _ = writer.close().await;
    result
}
pub async fn remote(
    client: &Client,
    user: &str,
    password: &str,
    command: &str,
    input: &[u8],
) -> Result<String, &'static str> {
    if user == "root" {
        return exec(client, command, input, 32768).await;
    }
    let marker = format!("vistart-{}", token());
    let wrapper = format!(
        "IFS= read -r vistart_line || exit 97; if [ \"$vistart_line\" != {} ]; then IFS= read -r vistart_line || exit 97; fi; [ \"$vistart_line\" = {} ] || exit 97; unset vistart_line; {command}",
        quote(&marker),
        quote(&marker)
    );
    let command = format!("sudo -k -S -p '' sh -c {}", quote(&wrapper));
    let mut payload = Zeroizing::new(format!("{password}\n{marker}\n").into_bytes());
    payload.extend_from_slice(input);
    exec(client, &command, &payload, 32768).await
}
pub fn create_key(comment: &str) -> Result<(Zeroizing<Vec<u8>>, String), &'static str> {
    let mut seed = Zeroizing::new([0u8; 32]);
    getrandom::fill(seed.as_mut()).map_err(|_| "random unavailable")?;
    let pair = russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&seed);
    let key = PrivateKey::new(
        russh::keys::ssh_key::private::KeypairData::Ed25519(pair),
        comment,
    )
    .map_err(|_| "SSH key generation failed")?;
    let public = key
        .public_key()
        .to_openssh()
        .map_err(|_| "SSH key serialization failed")?;
    let private = key
        .to_openssh(russh::keys::ssh_key::LineEnding::LF)
        .map_err(|_| "SSH key serialization failed")?;
    Ok((Zeroizing::new(private.as_bytes().to_vec()), public))
}
