//! Mocked internal web service around regslop. No real git and no real model: runs are
//! simulated, all state is in memory.

use std::net::SocketAddr;

mod api;
mod content;
mod sim;
mod state;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("regslop service (mock) on http://{addr}");
    axum::serve(listener, api::router(state::seeded())).await
}
