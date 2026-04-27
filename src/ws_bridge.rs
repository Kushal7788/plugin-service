use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, warn};

pub(crate) async fn bridge_websocket_to_duplex(
    session_id: String,
    socket: WebSocket,
    duplex: tokio::io::DuplexStream,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (mut duplex_reader, mut duplex_writer) = tokio::io::split(duplex);

    let inbound = async {
        while let Some(message_result) = ws_rx.next().await {
            match message_result {
                Ok(Message::Binary(bytes)) => {
                    if duplex_writer.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Text(text)) => {
                    if duplex_writer.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(_) => {}
                Err(error) => {
                    let error_text = error.to_string();
                    if error_text.contains("without closing handshake") {
                        info!(
                            "session={} websocket closed without handshake (treating as normal shutdown): {}",
                            session_id, error_text
                        );
                    } else {
                        warn!(
                            "session={} websocket inbound bridge error: {}",
                            session_id, error_text
                        );
                    }
                    break;
                }
            }
        }
        let _ = duplex_writer.shutdown().await;
    };

    let outbound = async {
        let mut buf = vec![0_u8; 16 * 1024];
        loop {
            match duplex_reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(size) => {
                    if ws_tx
                        .send(Message::Binary(buf[..size].to_vec()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(error) => {
                    warn!(
                        "session={} websocket outbound bridge error: {}",
                        session_id, error
                    );
                    break;
                }
            }
        }
        let _ = ws_tx.send(Message::Close(None)).await;
    };

    tokio::join!(inbound, outbound);
}
