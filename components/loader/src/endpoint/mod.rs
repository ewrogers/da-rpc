use crate::error::{ErrorKind, LoaderError, Result};
use std::{
    ffi::{OsStr, OsString},
    net::{Ipv4Addr, SocketAddr, ToSocketAddrs},
    path::Path,
};

const DEFAULT_SERVER_PORT: u16 = 2610;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServerEndpoint {
    host: String,
    port: u16,
}

impl ServerEndpoint {
    pub(crate) fn parse(value: &OsStr) -> Result<Self> {
        let value = value
            .to_str()
            .ok_or_else(|| invalid_endpoint("server endpoint must be valid Unicode"))?;

        if value.is_empty() || value.trim() != value {
            return Err(invalid_endpoint("server host must not be empty or padded"));
        }

        if value.matches(':').count() > 1 {
            return Err(invalid_endpoint(
                "server endpoint must be an IPv4 address or hostname with an optional port",
            ));
        }

        let (host, port) = match value.rsplit_once(':') {
            Some((host, port)) => {
                if host.is_empty() || port.is_empty() {
                    return Err(invalid_endpoint("server host and port must not be empty"));
                }

                let port = port.parse::<u16>().map_err(|_| {
                    invalid_endpoint("server port must be an integer from 1 through 65535")
                })?;

                (host, port)
            }
            None => (value, DEFAULT_SERVER_PORT),
        };

        if port == 0 {
            return Err(invalid_endpoint(
                "server port must be an integer from 1 through 65535",
            ));
        }

        Ok(Self {
            host: host.to_owned(),
            port,
        })
    }

    pub(crate) fn prepend_to(
        self,
        executable: &Path,
        arguments: Vec<OsString>,
    ) -> Result<Vec<OsString>> {
        let address = self.resolve_ipv4()?;

        if arguments.is_empty() {
            eprintln!(
                "Client command line: \"{}\" {address} {}",
                executable.display(),
                self.port
            );
        } else {
            eprintln!(
                "Client endpoint command prefix: \"{}\" {address} {} ({} additional argument(s) omitted)",
                executable.display(),
                self.port,
                arguments.len()
            );
        }

        let mut resolved = Vec::with_capacity(arguments.len() + 2);
        resolved.push(OsString::from(address.to_string()));
        resolved.push(OsString::from(self.port.to_string()));
        resolved.extend(arguments);
        Ok(resolved)
    }

    fn resolve_ipv4(&self) -> Result<Ipv4Addr> {
        if let Ok(address) = self.host.parse::<Ipv4Addr>() {
            eprintln!("Using server endpoint {address}:{}", self.port);
            return Ok(address);
        }

        let address = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|error| {
                LoaderError::new(
                    ErrorKind::LaunchFailed,
                    format!("failed to resolve server `{}`: {error}", self.host),
                )
            })?
            .find_map(|address| match address {
                SocketAddr::V4(address) => Some(*address.ip()),
                SocketAddr::V6(_) => None,
            })
            .ok_or_else(|| {
                LoaderError::new(
                    ErrorKind::LaunchFailed,
                    format!("server `{}` did not resolve to an IPv4 address", self.host),
                )
            })?;

        eprintln!("Resolved server {} to {address}:{}", self.host, self.port);
        Ok(address)
    }
}

fn invalid_endpoint(message: impl Into<String>) -> LoaderError {
    LoaderError::new(ErrorKind::InvalidArguments, message)
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_SERVER_PORT, ServerEndpoint};
    use std::ffi::{OsStr, OsString};

    #[test]
    fn parses_optional_port() {
        assert_eq!(
            ServerEndpoint::parse(OsStr::new("da0.kru.com")).unwrap(),
            ServerEndpoint {
                host: "da0.kru.com".to_owned(),
                port: DEFAULT_SERVER_PORT,
            }
        );
        assert_eq!(
            ServerEndpoint::parse(OsStr::new("127.0.0.1:3000")).unwrap(),
            ServerEndpoint {
                host: "127.0.0.1".to_owned(),
                port: 3000,
            }
        );
    }

    #[test]
    fn rejects_invalid_endpoints() {
        for value in ["", ":2610", "host:", "host:0", "host:nope", "::1"] {
            ServerEndpoint::parse(OsStr::new(value))
                .expect_err("invalid endpoint unexpectedly parsed");
        }
    }

    #[test]
    fn prepends_resolved_endpoint_arguments() {
        let endpoint = ServerEndpoint::parse(OsStr::new("127.0.0.1:3000")).unwrap();
        let arguments = endpoint
            .prepend_to(
                std::path::Path::new("Darkages.exe"),
                vec![OsString::from("existing")],
            )
            .unwrap();

        assert_eq!(
            arguments,
            ["127.0.0.1", "3000", "existing"].map(OsString::from)
        );
    }
}
