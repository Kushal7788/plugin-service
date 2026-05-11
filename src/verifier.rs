use eyre::eyre;
use std::sync::OnceLock;
use tlsn::{
    config::{tls_commit::TlsCommitProtocolConfig, verifier::VerifierConfig},
    connection::{ConnectionInfo, ServerEphemKey, TranscriptLength},
    connection::{DnsName, ServerName},
    transcript::{ContentType, PartialTranscript, TranscriptCommitment},
    verifier::VerifierOutput,
    webpki::RootCertStore,
    Session,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::compat::TokioAsyncReadCompatExt;
use tracing::info;

static ROOT_CERT_STORE: OnceLock<RootCertStore> = OnceLock::new();

fn root_cert_store() -> RootCertStore {
    ROOT_CERT_STORE.get_or_init(RootCertStore::mozilla).clone()
}

#[derive(Debug, Clone)]
pub struct VerifierCompletion {
    pub server_name: DnsName,
    pub transcript: PartialTranscript,
    pub transcript_commitments: Vec<TranscriptCommitment>,
    pub connection_info: ConnectionInfo,
    pub server_ephemeral_key: ServerEphemKey,
}

pub async fn run_verifier<T: AsyncWrite + AsyncRead + Send + Unpin + 'static>(
    socket: T,
    max_sent_data: usize,
    max_recv_data: usize,
) -> Result<VerifierCompletion, eyre::ErrReport> {
    let session = Session::new(socket.compat());
    let (driver, mut handle) = session.split();

    let driver_task = tokio::spawn(driver);

    let verifier_config = VerifierConfig::builder()
        .root_store(root_cert_store())
        .build()
        .map_err(|e| eyre!("failed to build verifier config: {e}"))?;

    let verifier = handle
        .new_verifier(verifier_config)
        .map_err(|e| eyre!("failed to construct verifier: {e}"))?;

    let verifier = verifier
        .commit()
        .await
        .map_err(|e| eyre!("commit phase failed: {e}"))?;
    let request = verifier.request();
    let TlsCommitProtocolConfig::Mpc(mpc_config) = request.protocol() else {
        return Err(eyre!("only MPC protocol is supported"));
    };

    if mpc_config.max_sent_data() > max_sent_data {
        return Err(eyre!(
            "prover max_sent_data {} exceeded limit {}",
            mpc_config.max_sent_data(),
            max_sent_data
        ));
    }
    if mpc_config.max_recv_data() > max_recv_data {
        return Err(eyre!(
            "prover max_recv_data {} exceeded limit {}",
            mpc_config.max_recv_data(),
            max_recv_data
        ));
    }

    let verifier = verifier
        .accept()
        .await
        .map_err(|e| eyre!("accept phase failed: {e}"))?
        .run()
        .await
        .map_err(|e| eyre!("run phase failed: {e}"))?;

    let verifier = verifier
        .verify()
        .await
        .map_err(|e| eyre!("verify phase failed: {e}"))?;
    let (
        VerifierOutput {
            server_name,
            transcript,
            transcript_commitments,
        },
        verifier,
    ) = verifier
        .accept()
        .await
        .map_err(|e| eyre!("verifier accept failed: {e}"))?;
    let tls_transcript = verifier.tls_transcript().clone();

    verifier
        .close()
        .await
        .map_err(|e| eyre!("verifier close failed: {e}"))?;
    handle.close();
    driver_task
        .await
        .map_err(|e| eyre!("driver join failed: {e}"))?
        .map_err(|e| eyre!("driver runtime failed: {e}"))?;

    let server_name = server_name.ok_or_else(|| eyre!("server name not revealed"))?;
    let transcript = transcript.ok_or_else(|| eyre!("transcript not revealed"))?;
    // TLSN attestation connection info expects application data transcript lengths.
    let sent_len = tls_transcript
        .sent()
        .iter()
        .filter(|record| record.typ == ContentType::ApplicationData)
        .map(|record| record.ciphertext.len())
        .sum::<usize>();
    let recv_len = tls_transcript
        .recv()
        .iter()
        .filter(|record| record.typ == ContentType::ApplicationData)
        .map(|record| record.ciphertext.len())
        .sum::<usize>();
    let connection_info = ConnectionInfo {
        time: tls_transcript.time(),
        version: *tls_transcript.version(),
        transcript_length: TranscriptLength {
            sent: sent_len as u32,
            received: recv_len as u32,
        },
    };
    let server_ephemeral_key = tls_transcript.server_ephemeral_key().clone();
    let ServerName::Dns(dns_name) = server_name;
    info!("verifier session completed for host={dns_name}");
    Ok(VerifierCompletion {
        server_name: dns_name,
        transcript,
        transcript_commitments,
        connection_info,
        server_ephemeral_key,
    })
}
