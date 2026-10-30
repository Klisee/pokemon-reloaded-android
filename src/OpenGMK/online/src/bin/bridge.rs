use reloaded_online::bridge::run;
use std::net::TcpListener;

fn main() -> std::io::Result<()> {
    let mut args = std::env::args().skip(1);
    let listen = args.next().unwrap_or_else(|| "0.0.0.0:7778".into());
    let upstream = args.next().unwrap_or_else(|| "64.177.48.215:5000".into());
    let listener = TcpListener::bind(&listen)?;
    run(listener, upstream);
    Ok(())
}
