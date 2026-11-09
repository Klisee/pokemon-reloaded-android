//! Classification and Spanish wording for Zibelan/OS connection failures.
//!
//! The transport reports failures as plain text because the game-side adapter keeps the
//! same `String` status for both backends. This module centralises the mapping so a
//! timeout, a refused socket, a lost route, a DNS error, a protocol mismatch, a full
//! server, a peer leaving or a rate limit never leaks raw English OS text to the UI.
//!
//! It has no socket dependency: every entry point works from an [`std::io::Error`], an
//! [`std::io::ErrorKind`], a textual reason or one protocol line.

use std::{
    fmt,
    io::{self, ErrorKind},
};

/// Why a Zibelan connection attempt or session failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionFailure {
    /// The TCP connect or handshake timed out.
    Timeout,
    /// The server refused the connection.
    Refused,
    /// The peer reset, aborted or closed the socket unexpectedly.
    Reset,
    /// There is no network route to the server.
    NoRoute,
    /// The host name could not be resolved.
    Dns,
    /// The server speaks an incompatible protocol or version.
    Protocol,
    /// `FULL`/`WFULL`: the lobby or world has no free slot.
    Full,
    /// `PEER_LEFT`: the opponent abandoned the session.
    PeerLeft,
    /// The battle checksums diverged between the two clients.
    Desync,
    /// The server throttled or rate-limited the connection.
    RateLimited,
    /// A failure that does not match a known category.
    Other,
}

impl ConnectionFailure {
    /// Classify a socket error reported while connecting or reading.
    ///
    /// Some platforms report protocol failures as [`ErrorKind::Other`]; when that
    /// happens the OS message is inspected before falling back to [`Self::Other`].
    pub fn from_io_error(error: &io::Error) -> Self {
        let by_kind = Self::from_error_kind(error.kind());
        if by_kind == Self::Other {
            Self::from_reason(&error.to_string())
        } else {
            by_kind
        }
    }

    /// Classify an [`io::ErrorKind`]. Unknown socket failures stay [`Self::Other`].
    pub fn from_error_kind(kind: ErrorKind) -> Self {
        match kind {
            ErrorKind::TimedOut | ErrorKind::WouldBlock => Self::Timeout,
            ErrorKind::ConnectionRefused | ErrorKind::PermissionDenied => Self::Refused,
            ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof
            | ErrorKind::NotConnected => Self::Reset,
            ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable | ErrorKind::AddrNotAvailable => {
                Self::NoRoute
            },
            ErrorKind::ResourceBusy => Self::RateLimited,
            _ => Self::Other,
        }
    }

    /// Classify a textual transport reason, keeping both English libc wording and the
    /// Spanish messages used by this crate.
    pub fn from_reason(reason: &str) -> Self {
        let lower = reason.to_ascii_lowercase();
        if contains_any(
            &lower,
            &["desync", "desincroniz", "fuera de sincron", "checksum distinto"],
        ) {
            Self::Desync
        } else if contains_any(
            &lower,
            &[
                "version",
                "versión",
                "protocol",
                "protocolo",
                "incompatible",
                "incompat",
                "wrong build",
                "build mismatch",
                "version mismatch",
                "actualiza",
            ],
        ) {
            Self::Protocol
        } else if contains_any(
            &lower,
            &[
                "rate limit",
                "rate-limit",
                "ratelimit",
                "rate limited",
                "too many",
                "too fast",
                "slow down",
                "flood",
                "throttle",
                "demasiadas",
                "demasiado rápido",
                "demasiado rapido",
                "límite",
                "limite",
                "limitó",
                "limito",
                "espera antes",
            ],
        ) {
            Self::RateLimited
        } else if contains_any(&lower, &["full", "lleno", "llena"]) {
            Self::Full
        } else if contains_any(&lower, &["peer_left", "peer-left", "peer left"])
            || (lower.contains("rival")
                && contains_any(&lower, &["salio", "salió", "abandon", "desconect"]))
        {
            Self::PeerLeft
        } else if contains_any(
            &lower,
            &[
                "timed out",
                "timeout",
                "timed_out",
                "tiempo de espera",
                "tiempo de conexión",
                "tiempo de conexion",
                "did not properly respond",
                "temporarily unavailable",
                "no responde",
                "sin respuesta",
            ],
        ) {
            Self::Timeout
        } else if contains_any(
            &lower,
            &[
                "dns",
                "resolve",
                "resolver",
                "lookup",
                "name or service not known",
                "name resolution",
                "no such host",
                "nodename nor servname",
                "no address associated with hostname",
                "getaddrinfo",
                "nombre o servicio desconocido",
                "resolución",
                "resolucion",
                "dirección inválida",
                "direccion invalida",
                "no resolvió",
                "no resolvio",
            ],
        ) {
            Self::Dns
        } else if contains_any(
            &lower,
            &[
                "no route",
                "network is unreachable",
                "network unreachable",
                "host is unreachable",
                "host unreachable",
                "unreachable network",
                "unreachable host",
                "sin ruta",
                "no hay ruta",
            ],
        ) {
            Self::NoRoute
        } else if contains_any(
            &lower,
            &["connection refused", "conexión rechazada", "conexion rechazada", "rechazó", "rechazo"],
        ) {
            Self::Refused
        } else if contains_any(
            &lower,
            &[
                "connection reset",
                "reset by peer",
                "reinició",
                "reinicio",
                "broken pipe",
                "connection aborted",
                "forcibly closed",
                "cerró la conexión",
                "cerro la conexion",
                "closed the connection",
                "se perdió la conexión",
                "se perdio la conexion",
                "desconect",
            ],
        ) {
            Self::Reset
        } else {
            Self::Other
        }
    }

    /// Recognise a server line that terminates the connection for a known reason.
    ///
    /// `FULL`, `WFULL` and `PEER_LEFT` normally reach the typed codec; this helper also
    /// accepts their textual form, plus common rate-limit and version-rejection lines.
    /// Ordinary unknown lines return `None` so they keep being ignored.
    pub fn from_server_line(line: &[u8]) -> Option<Self> {
        let text = String::from_utf8_lossy(line);
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let lower = text.to_ascii_lowercase();
        let command = lower.split_whitespace().next().unwrap_or("").trim_end_matches(':');

        match command {
            "full" | "wfull" => return Some(Self::Full),
            "peer_left" | "peer-left" | "peerleft" => return Some(Self::PeerLeft),
            "desync" | "desincronizacion" | "desincronización" | "checksum_mismatch" | "checksum-mismatch" => {
                return Some(Self::Desync)
            },
            "timeout" | "timed_out" | "timed-out" | "no_response" | "no-response" | "noresp" | "no_resp"
            | "sin_respuesta" | "sin-respuesta" => return Some(Self::Timeout),
            "rate" | "rate_limit" | "rate-limit" | "ratelimit" | "limit" | "limited" | "too_fast" | "too-fast"
            | "toomany" | "too_many" | "slow_down" | "slow-down" | "flood" | "throttle" | "busy" => {
                return Some(Self::RateLimited)
            },
            _ => {},
        }

        let version_rejection = contains_any(
            &lower,
            &[
                "version",
                "versión",
                "protocol",
                "protocolo",
                "incompatible",
                "incompat",
                "wrong build",
                "build mismatch",
                "version mismatch",
                "actualiza",
            ],
        );
        if version_rejection {
            return Some(Self::Protocol);
        }
        if contains_any(&lower, &["peer_left", "peer-left", "peer left"]) {
            return Some(Self::PeerLeft);
        }
        if contains_any(&lower, &["desync", "desincroniz"]) {
            return Some(Self::Desync);
        }
        if contains_any(&lower, &["full", "lleno", "llena"]) {
            return Some(Self::Full);
        }
        if contains_any(&lower, &["no responde", "sin respuesta", "did not properly respond"]) {
            return Some(Self::Timeout);
        }
        if contains_any(
            &lower,
            &[
                "rate limit",
                "rate-limit",
                "ratelimit",
                "rate limited",
                "too many",
                "too fast",
                "slow down",
                "flood",
                "throttle",
                "demasiadas",
                "límite",
                "limite",
                "limitó",
                "limito",
            ],
        ) {
            return Some(Self::RateLimited);
        }
        None
    }

    /// Clear Spanish message shown in the online status and session panels.
    pub fn message(self) -> &'static str {
        match self {
            Self::Timeout => "El servidor no responde; se agotó el tiempo de espera.",
            Self::Refused => "El servidor rechazó la conexión.",
            Self::Reset => "Se perdió la conexión con el servidor.",
            Self::NoRoute => "No se pudo llegar al servidor: no hay ruta de red.",
            Self::Dns => "No se pudo resolver la dirección del servidor (DNS).",
            Self::Protocol => "La versión del servidor no es compatible; actualiza el juego.",
            Self::Full => "El servidor está lleno; inténtalo más tarde.",
            Self::PeerLeft => "El rival se desconectó de la partida.",
            Self::Desync => "El combate se desincronizó; la partida online se cerró.",
            Self::RateLimited => "El servidor limitó la conexión; espera un momento antes de reintentar.",
            Self::Other => "No se pudo conectar con el servidor.",
        }
    }

    /// Clear Spanish message for `FULL`/`WFULL`.
    pub fn full_message(world: bool) -> &'static str {
        if world {
            "El mundo está lleno; inténtalo más tarde."
        } else {
            "La sala está llena; inténtalo más tarde."
        }
    }
}

impl fmt::Display for ConnectionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_errors_map_to_clear_categories() {
        for (kind, failure) in [
            (ErrorKind::TimedOut, ConnectionFailure::Timeout),
            (ErrorKind::WouldBlock, ConnectionFailure::Timeout),
            (ErrorKind::ConnectionRefused, ConnectionFailure::Refused),
            (ErrorKind::ConnectionReset, ConnectionFailure::Reset),
            (ErrorKind::ConnectionAborted, ConnectionFailure::Reset),
            (ErrorKind::BrokenPipe, ConnectionFailure::Reset),
            (ErrorKind::UnexpectedEof, ConnectionFailure::Reset),
            (ErrorKind::NotConnected, ConnectionFailure::Reset),
            (ErrorKind::NetworkUnreachable, ConnectionFailure::NoRoute),
            (ErrorKind::HostUnreachable, ConnectionFailure::NoRoute),
            (ErrorKind::AddrNotAvailable, ConnectionFailure::NoRoute),
            (ErrorKind::ResourceBusy, ConnectionFailure::RateLimited),
            (ErrorKind::Other, ConnectionFailure::Other),
        ] {
            assert_eq!(ConnectionFailure::from_error_kind(kind), failure, "{kind:?}");
            assert_eq!(ConnectionFailure::from_io_error(&io::Error::from(kind)), failure, "{kind:?}");
        }
    }

    #[test]
    fn textual_reasons_cover_every_required_failure() {
        for (reason, failure) in [
            ("connection timed out", ConnectionFailure::Timeout),
            ("Connection refused", ConnectionFailure::Refused),
            ("Connection reset by peer", ConnectionFailure::Reset),
            ("Network is unreachable", ConnectionFailure::NoRoute),
            ("No route to host", ConnectionFailure::NoRoute),
            ("A socket operation was attempted to an unreachable network", ConnectionFailure::NoRoute),
            ("Failed to lookup address information: Name or service not known", ConnectionFailure::Dns),
            ("Temporary failure in name resolution", ConnectionFailure::Dns),
            ("dirección inválida (host): failed to lookup address", ConnectionFailure::Dns),
            ("An existing connection was forcibly closed by the remote host", ConnectionFailure::Reset),
            ("se perdió la conexión con el servidor", ConnectionFailure::Reset),
            ("versión incompatible", ConnectionFailure::Protocol),
            ("el mundo esta lleno", ConnectionFailure::Full),
            ("el rival abandono la partida", ConnectionFailure::PeerLeft),
            ("el rival se desconectó de la partida", ConnectionFailure::PeerLeft),
            ("el servidor no responde", ConnectionFailure::Timeout),
            ("sin respuesta del servidor", ConnectionFailure::Timeout),
            ("el combate se desincronizó", ConnectionFailure::Desync),
            ("el servidor limitó la conexión", ConnectionFailure::RateLimited),
        ] {
            assert_eq!(ConnectionFailure::from_reason(reason), failure, "{reason}");
        }
    }

    #[test]
    fn server_lines_recognise_rate_limits_and_version_rejections() {
        for line in [
            b"RATE_LIMIT 30".as_slice(),
            b"limit".as_slice(),
            b"TOO_FAST".as_slice(),
            b"ERROR too many connections".as_slice(),
            b"ERROR rate limited".as_slice(),
        ] {
            assert_eq!(
                ConnectionFailure::from_server_line(line),
                Some(ConnectionFailure::RateLimited),
                "{}",
                String::from_utf8_lossy(line)
            );
        }
        for line in [
            b"ERROR version incompatible".as_slice(),
            b"ERROR protocol mismatch".as_slice(),
            b"FATAL: actualiza el juego".as_slice(),
        ] {
            assert_eq!(
                ConnectionFailure::from_server_line(line),
                Some(ConnectionFailure::Protocol),
                "{}",
                String::from_utf8_lossy(line)
            );
        }
        assert_eq!(ConnectionFailure::from_server_line(b"FULL"), Some(ConnectionFailure::Full));
        assert_eq!(ConnectionFailure::from_server_line(b"WFULL"), Some(ConnectionFailure::Full));
        assert_eq!(
            ConnectionFailure::from_server_line(b"ERROR mundo lleno"),
            Some(ConnectionFailure::Full)
        );
        assert_eq!(
            ConnectionFailure::from_server_line(b"PEER_LEFT"),
            Some(ConnectionFailure::PeerLeft)
        );
        assert_eq!(
            ConnectionFailure::from_server_line(b"NO_RESPONSE"),
            Some(ConnectionFailure::Timeout)
        );
        assert_eq!(
            ConnectionFailure::from_server_line(b"ERROR server no responde"),
            Some(ConnectionFailure::Timeout)
        );
        assert_eq!(
            ConnectionFailure::from_server_line(b"DESYNC"),
            Some(ConnectionFailure::Desync)
        );
        assert_eq!(ConnectionFailure::from_server_line(b"WHAT 1"), None);
        assert_eq!(ConnectionFailure::from_server_line(b""), None);
    }

    #[test]
    fn every_category_has_a_clear_spanish_message() {
        for (failure, expected) in [
            (ConnectionFailure::Timeout, "no responde"),
            (ConnectionFailure::Refused, "rechazó"),
            (ConnectionFailure::Reset, "conexión"),
            (ConnectionFailure::NoRoute, "ruta de red"),
            (ConnectionFailure::Dns, "DNS"),
            (ConnectionFailure::Protocol, "versión"),
            (ConnectionFailure::Full, "lleno"),
            (ConnectionFailure::PeerLeft, "desconectó"),
            (ConnectionFailure::Desync, "desincronizó"),
            (ConnectionFailure::RateLimited, "limitó"),
            (ConnectionFailure::Other, "No se pudo conectar"),
        ] {
            let message = failure.message();
            assert!(message.contains(expected), "{failure:?}: {message}");
            assert_eq!(failure.to_string(), message);
        }
        assert!(ConnectionFailure::full_message(true).contains("mundo"));
        assert!(ConnectionFailure::full_message(false).contains("sala"));
    }
}
