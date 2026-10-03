//! Exercises the real SSH packet loop through a delayed in-memory transport.
use russh::{
    Channel, ChannelId, ChannelMsg, client,
    keys::{PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate},
    server,
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::{sleep, timeout},
};

fn key() -> PrivateKey {
    let mut seed = [0; 32];
    getrandom::fill(&mut seed).unwrap();
    let pair = russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&seed);
    PrivateKey::new(
        russh::keys::ssh_key::private::KeypairData::Ed25519(pair),
        "ephemeral-test",
    )
    .unwrap()
}
struct Verify {
    key: PublicKey,
    exchanges: Arc<AtomicUsize>,
}
impl client::Handler for Verify {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(key.certificate().is_none() && key.public_key().key_data() == self.key.key_data())
    }
    async fn kex_done(
        &mut self,
        _: Option<&[u8]>,
        _: &russh::Names,
        _: &mut client::Session,
    ) -> Result<(), Self::Error> {
        self.exchanges.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
struct Echo {
    key: PublicKey,
}
impl server::Handler for Echo {
    type Error = russh::Error;
    async fn auth_publickey(
        &mut self,
        _: &str,
        key: &PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if key.key_data() == self.key.key_data() {
            server::Auth::Accept
        } else {
            server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            }
        })
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    async fn data(
        &mut self,
        id: ChannelId,
        data: &[u8],
        s: &mut server::Session,
    ) -> Result<(), Self::Error> {
        s.data(id, data.to_vec())?;
        Ok(())
    }
}
struct Fixture {
    client: client::Handle<Verify>,
    delay: Arc<AtomicU64>,
    exchanges: Arc<AtomicUsize>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
async fn fixture(deadline: Duration) -> Fixture {
    let host = key();
    let identity = key();
    let expected = host.public_key().clone();
    let (client_io, bridge_client) = tokio::io::duplex(65536);
    let (bridge_server, server_io) = tokio::io::duplex(65536);
    let (mut from_client, mut to_client) = tokio::io::split(bridge_client);
    let (mut from_server, mut to_server) = tokio::io::split(bridge_server);
    let delay = Arc::new(AtomicU64::new(0));
    let delayed = delay.clone();
    let outgoing = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut from_client, &mut to_server).await;
    });
    let incoming = tokio::spawn(async move {
        let mut bytes = vec![0; 65536];
        while let Ok(n) = from_server.read(&mut bytes).await {
            if n == 0 {
                break;
            }
            let ms = delayed.swap(0, Ordering::SeqCst);
            if ms > 0 {
                sleep(Duration::from_millis(ms)).await
            }
            if to_client.write_all(&bytes[..n]).await.is_err() {
                break;
            }
        }
    });
    let echo = Echo {
        key: identity.public_key().clone(),
    };
    let server = tokio::spawn(async move {
        if let Ok(s) = server::run_stream(
            Arc::new(server::Config {
                keys: vec![host],
                ..Default::default()
            }),
            server_io,
            echo,
        )
        .await
        {
            let _ = s.await;
        }
    });
    let exchanges = Arc::new(AtomicUsize::new(0));
    let config = client::Config {
        keepalive_interval: Some(Duration::from_millis(100)),
        keepalive_max: 2,
        key_exchange_timeout: Some(deadline),
        ..Default::default()
    };
    let mut client = client::connect_stream(
        Arc::new(config),
        client_io,
        Verify {
            key: expected,
            exchanges: exchanges.clone(),
        },
    )
    .await
    .unwrap();
    assert!(
        client
            .authenticate_publickey("qa", PrivateKeyWithHashAlg::new(Arc::new(identity), None))
            .await
            .unwrap()
            .success()
    );
    Fixture {
        client,
        delay,
        exchanges,
        tasks: vec![outgoing, incoming, server],
    }
}
#[tokio::test]
async fn delayed_rekey_outlives_keepalive_window_and_preserves_data() {
    timeout(Duration::from_secs(10), async {
        let f = fixture(Duration::from_secs(2)).await;
        let mut channel = f.client.channel_open_session().await.unwrap();
        f.delay.store(650, Ordering::SeqCst);
        f.client.rekey_soon().await.unwrap();
        channel
            .data_bytes(b"same encrypted channel".to_vec())
            .await
            .unwrap();
        let mut bytes = Vec::new();
        while bytes.len() < 22 {
            if let Some(ChannelMsg::Data { data }) = channel.wait().await {
                bytes.extend_from_slice(&data)
            } else {
                panic!("channel ended")
            }
        }
        assert_eq!(bytes, b"same encrypted channel");
        assert!(f.exchanges.load(Ordering::SeqCst) >= 2);
        // Normal keepalives must resume after NEWKEYS, including idle periods.
        sleep(Duration::from_millis(700)).await;
        assert!(!f.client.is_closed());
        f.client
            .disconnect(russh::Disconnect::ByApplication, "test finished", "")
            .await
            .unwrap();
        for task in f.tasks {
            task.abort();
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn stalled_rekey_has_its_own_absolute_deadline() {
    timeout(Duration::from_secs(5), async {
        let f = fixture(Duration::from_millis(300)).await;
        f.delay.store(1500, Ordering::SeqCst);
        f.client.rekey_soon().await.unwrap();
        let result = f.client.await;
        assert!(
            matches!(result, Err(russh::Error::KeyExchangeTimeout)),
            "{result:?}"
        );
        for task in f.tasks {
            task.abort();
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn missing_normal_keepalive_still_disconnects() {
    timeout(Duration::from_secs(5), async {
        let f = fixture(Duration::from_secs(2)).await;
        f.delay.store(1500, Ordering::SeqCst);
        let result = f.client.await;
        assert!(
            matches!(result, Err(russh::Error::KeepaliveTimeout)),
            "{result:?}"
        );
        for task in f.tasks {
            task.abort();
        }
    })
    .await
    .unwrap();
}
