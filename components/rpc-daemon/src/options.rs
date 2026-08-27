use std::{
    collections::BTreeSet,
    ffi::OsString,
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
};

const DEFAULT_PORT: u16 = 2626;
pub(crate) const USAGE: &str = concat!(
    "usage: darpcd [--pid <pid> ...] [--port <port> | --listen <ipv4[:port]>] ",
    "[--auto-load] [--managed] ",
    "[--loader-path <path>] [--dll-path <path>] [--maps-path <path>]\n       ",
    "darpcd --print-openapi"
);

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Options {
    pub(crate) pids: Vec<u32>,
    pub(crate) listen: SocketAddrV4,
    pub(crate) auto_load: bool,
    pub(crate) managed: bool,
    pub(crate) loader_path: Option<PathBuf>,
    pub(crate) dll_path: Option<PathBuf>,
    pub(crate) maps_path: Option<PathBuf>,
    pub(crate) print_openapi: bool,
}

pub(crate) fn parse_options(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Options, String> {
    let mut arguments = arguments.into_iter();
    let mut pids = Vec::new();
    let mut unique = BTreeSet::new();
    let mut port = None;
    let mut listen = None;
    let mut auto_load = false;
    let mut managed = false;
    let mut loader_path = None;
    let mut dll_path = None;
    let mut maps_path = None;
    let mut print_openapi = false;

    while let Some(option) = arguments.next() {
        if option == "--pid" {
            let value = arguments
                .next()
                .ok_or_else(|| "--pid requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "PID must be valid Unicode".to_owned())?;
            let pid: u32 = value
                .parse()
                .map_err(|_| "PID must be an unsigned 32-bit integer".to_owned())?;
            if pid == 0 {
                return Err("PID must be greater than zero".into());
            }
            if !unique.insert(pid) {
                return Err(format!("PID {pid} was provided more than once"));
            }
            pids.push(pid);
        } else if option == "--port" {
            if port.is_some() {
                return Err("--port may be provided only once".into());
            }
            let value = arguments
                .next()
                .ok_or_else(|| "--port requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "port must be valid Unicode".to_owned())?;
            let parsed: u16 = value
                .parse()
                .map_err(|_| "port must be an integer from 1 through 65535".to_owned())?;
            if parsed == 0 {
                return Err("port must be greater than zero".into());
            }
            port = Some(parsed);
        } else if option == "--listen" {
            if listen.is_some() {
                return Err("--listen may be provided only once".into());
            }
            let value = arguments
                .next()
                .ok_or_else(|| "--listen requires a value".to_owned())?;
            let value = value
                .to_str()
                .ok_or_else(|| "listen address must be valid Unicode".to_owned())?;
            listen = Some(parse_listen_address(value)?);
        } else if option == "--auto-load" {
            if auto_load {
                return Err("--auto-load may be provided only once".into());
            }
            auto_load = true;
        } else if option == "--managed" {
            if managed {
                return Err("--managed may be provided only once".into());
            }
            managed = true;
        } else if option == "--loader-path" {
            parse_path_option(&mut arguments, &mut loader_path, "--loader-path")?;
        } else if option == "--dll-path" {
            parse_path_option(&mut arguments, &mut dll_path, "--dll-path")?;
        } else if option == "--maps-path" {
            parse_path_option(&mut arguments, &mut maps_path, "--maps-path")?;
        } else if option == "--print-openapi" {
            if print_openapi {
                return Err("--print-openapi may be provided only once".into());
            }
            print_openapi = true;
        } else {
            return Err(format!("unknown option `{}`", option.to_string_lossy()));
        }
    }

    if print_openapi
        && (!pids.is_empty()
            || port.is_some()
            || listen.is_some()
            || auto_load
            || managed
            || loader_path.is_some()
            || dll_path.is_some()
            || maps_path.is_some())
    {
        return Err("--print-openapi cannot be combined with server options".into());
    }
    if port.is_some() && listen.is_some() {
        return Err("--port and --listen cannot be combined".into());
    }

    Ok(Options {
        pids,
        listen: listen.unwrap_or_else(|| {
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, port.unwrap_or(DEFAULT_PORT))
        }),
        auto_load,
        managed,
        loader_path,
        dll_path,
        maps_path,
        print_openapi,
    })
}

fn parse_listen_address(value: &str) -> Result<SocketAddrV4, String> {
    if let Ok(address) = value.parse::<SocketAddrV4>() {
        if address.port() == 0 {
            return Err("listen port must be greater than zero".into());
        }
        return Ok(address);
    }
    value
        .parse::<Ipv4Addr>()
        .map(|address| SocketAddrV4::new(address, DEFAULT_PORT))
        .map_err(|_| "listen address must be an IPv4 address with an optional port".to_owned())
}

fn parse_path_option(
    arguments: &mut impl Iterator<Item = OsString>,
    destination: &mut Option<PathBuf>,
    option: &str,
) -> Result<(), String> {
    if destination.is_some() {
        return Err(format!("{option} may be provided only once"));
    }
    let value = arguments
        .next()
        .ok_or_else(|| format!("{option} requires a value"))?;
    if value.is_empty() {
        return Err(format!("{option} requires a nonempty path"));
    }
    *destination = Some(PathBuf::from(value));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PORT, Options, parse_options};
    use std::{
        ffi::OsString,
        net::{Ipv4Addr, SocketAddrV4},
    };

    fn default_listen() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, DEFAULT_PORT)
    }

    fn arguments(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_repeated_pid_options_in_order() {
        assert_eq!(
            parse_options(arguments(&["--pid", "42", "--pid", "7"])).unwrap(),
            Options {
                pids: vec![42, 7],
                listen: default_listen(),
                auto_load: false,
                managed: false,
                loader_path: None,
                dll_path: None,
                maps_path: None,
                print_openapi: false,
            }
        );
        assert_eq!(
            parse_options(arguments(&["--port", "3000", "--pid", "42"])).unwrap(),
            Options {
                pids: vec![42],
                listen: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 3000),
                auto_load: false,
                managed: false,
                loader_path: None,
                dll_path: None,
                maps_path: None,
                print_openapi: false,
            }
        );
    }

    #[test]
    fn parses_discovery_and_management_options() {
        assert_eq!(
            parse_options(arguments(&[
                "--loader-path",
                "tools/loader.exe",
                "--dll-path",
                "tools/darpc.dll",
                "--maps-path",
                "C:\\Dark Ages\\Maps",
            ]))
            .unwrap(),
            Options {
                pids: Vec::new(),
                listen: default_listen(),
                auto_load: false,
                managed: false,
                loader_path: Some("tools/loader.exe".into()),
                dll_path: Some("tools/darpc.dll".into()),
                maps_path: Some("C:\\Dark Ages\\Maps".into()),
                print_openapi: false,
            }
        );
        assert!(parse_options(Vec::<OsString>::new()).is_ok());
        assert!(
            parse_options(arguments(&["--auto-load"]))
                .unwrap()
                .auto_load
        );
        assert!(
            parse_options(arguments(&["--print-openapi"]))
                .unwrap()
                .print_openapi
        );
    }

    #[test]
    fn managed_mode_is_disabled_by_default_and_can_be_enabled() {
        assert!(!parse_options(Vec::<OsString>::new()).unwrap().managed);
        assert!(parse_options(arguments(&["--managed"])).unwrap().managed);
    }

    #[test]
    fn managed_mode_rejects_duplicates_and_openapi_export() {
        assert!(parse_options(arguments(&["--managed", "--managed"])).is_err());
        assert!(parse_options(arguments(&["--managed", "--print-openapi"])).is_err());
    }

    #[test]
    fn parses_explicit_listen_addresses() {
        assert_eq!(
            parse_options(arguments(&["--listen", "0.0.0.0:2620"]))
                .unwrap()
                .listen,
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 2620)
        );
        assert_eq!(
            parse_options(arguments(&["--listen", "192.168.1.5"]))
                .unwrap()
                .listen,
            SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 5), DEFAULT_PORT)
        );
    }

    #[test]
    fn rejects_invalid_targets_and_ports() {
        assert!(parse_options(arguments(&["--pid", "0"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--pid", "7"])).is_err());
        assert!(parse_options(arguments(&["--pids", "7,8"])).is_err());
        assert!(parse_options(arguments(&["--client-path", "Darkages.exe"])).is_err());
        assert!(parse_options(arguments(&["--maps-path", ""])).is_err());
        assert!(parse_options(arguments(&["--auto-load", "--auto-load"])).is_err());
        assert!(parse_options(arguments(&["--print-openapi", "--port", "2626"])).is_err());
        assert!(parse_options(arguments(&["--print-openapi", "--print-openapi"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port", "0"])).is_err());
        assert!(parse_options(arguments(&["--pid", "7", "--port", "65536"])).is_err());
        assert!(parse_options(arguments(&["--listen"])).is_err());
        assert!(parse_options(arguments(&["--listen", "localhost:2626"])).is_err());
        assert!(parse_options(arguments(&["--listen", "0.0.0.0:0"])).is_err());
        assert!(parse_options(arguments(&["--listen", "0.0.0.0:2626", "--port", "2626"])).is_err());
        assert!(
            parse_options(arguments(&["--listen", "0.0.0.0", "--listen", "127.0.0.1"])).is_err()
        );
        assert!(parse_options(arguments(&["--print-openapi", "--listen", "127.0.0.1"])).is_err());
        assert!(
            parse_options(arguments(&[
                "--pid", "7", "--port", "2626", "--port", "2627"
            ]))
            .is_err()
        );
        assert!(
            parse_options(arguments(&[
                "--loader-path",
                "first.exe",
                "--loader-path",
                "second.exe",
            ]))
            .is_err()
        );
        assert!(
            parse_options(arguments(&[
                "--maps-path",
                "first",
                "--maps-path",
                "second",
            ]))
            .is_err()
        );
    }
}
