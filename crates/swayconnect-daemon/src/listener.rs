//! Accept loop, peer filter and TLS accept (`docs/DESIGN.md` 4.6 steps 1 and 2).

use std::net::SocketAddr;
use std::time::Duration;

use swayconnect_protocol::limits::{ALPN_ID, TLS_HANDSHAKE_DEADLINE_MS};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinError, JoinSet};
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;

use crate::security::ConnectionPermit;
use crate::session::{self, Shared, stopped};

const TLS_HANDSHAKE_DEADLINE: Duration = Duration::from_millis(TLS_HANDSHAKE_DEADLINE_MS);
const ACCEPT_RETRY_DELAY: Duration = Duration::from_millis(100);
/// How long open sessions get to send `bye` after the daemon is told to stop.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Serves one listen address until the daemon stops, then waits for its sessions.
pub async fn run(listener: TcpListener, shared: Shared) {
    let acceptor = TlsAcceptor::from(shared.security.tls_config());
    let mut shutdown = shared.shutdown.clone();
    let mut sessions = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) => match shared.security.admit(peer.ip()) {
                    Ok(permit) => {
                        let connection = Connection { stream, peer, permit };
                        sessions.spawn(connection.serve(acceptor.clone(), shared.clone()));
                    }
                    // Dropping the stream closes it before TLS, with no frame (spec 14).
                    // `admit` has logged the refusal.
                    Err(_refusal) => {}
                },
                Err(error) => {
                    tracing::warn!(%error, "accept failed");
                    tokio::time::sleep(ACCEPT_RETRY_DELAY).await;
                }
            },
            Some(ended) = sessions.join_next() => log_panic(ended),
            () = stopped(&mut shutdown) => break,
        }
    }
    drop(listener);
    let drain = async {
        while let Some(ended) = sessions.join_next().await {
            log_panic(ended);
        }
    };
    if timeout(SHUTDOWN_GRACE, drain).await.is_err() {
        // Dropping the set aborts them.
        tracing::warn!(open = sessions.len(), "sessions did not close in time");
    }
}

fn log_panic(ended: Result<(), JoinError>) {
    if let Err(error) = ended {
        tracing::error!(%error, "session task failed");
    }
}

/// An admitted TCP connection before TLS.
struct Connection {
    stream: TcpStream,
    peer: SocketAddr,
    permit: ConnectionPermit,
}

impl Connection {
    async fn serve(mut self, acceptor: TlsAcceptor, shared: Shared) {
        let mut shutdown = shared.shutdown.clone();
        let peer = self.peer;
        let handshake = timeout(TLS_HANDSHAKE_DEADLINE, acceptor.accept(self.stream));
        let tls = tokio::select! {
            accepted = handshake => match accepted {
                Ok(Ok(tls)) => tls,
                Ok(Err(error)) => return tracing::debug!(%peer, %error, "TLS handshake failed"),
                Err(_) => return tracing::debug!(%peer, "TLS handshake deadline passed"),
            },
            () = self.permit.evicted() => return,
            () = stopped(&mut shutdown) => return,
        };
        // rustls completes a handshake in which the client offered no ALPN at all.
        if tls.get_ref().1.alpn_protocol() != Some(ALPN_ID) {
            return tracing::debug!(%peer, "client did not offer the ALPN protocol");
        }
        session::serve(tls, peer, self.permit, shared).await;
    }
}
