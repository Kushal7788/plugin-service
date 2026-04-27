mod app;
mod artifact;
mod config;
mod handlers;
mod policy;
mod session;
mod state;
mod types;
mod verifier;
mod ws_bridge;

#[tokio::main]
async fn main() {
    app::run().await;
}
