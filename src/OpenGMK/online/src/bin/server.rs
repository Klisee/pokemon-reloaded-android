use reloaded_online::{serve, DEFAULT_PORT};
use std::net::TcpListener;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let bind = args.next().unwrap_or_else(|| format!("0.0.0.0:{DEFAULT_PORT}"));
    let listener = TcpListener::bind(&bind)?;
    eprintln!("reloaded-online-server escuchando en {bind}");
    serve(listener)
}
