use serde::Deserialize;
use std::{
    ffi::OsString,
    path::PathBuf,
    process::{Command, Output},
    sync::Mutex,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ServerEndpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LaunchOptions {
    pub(crate) client_path: PathBuf,
    pub(crate) allow_multiple: bool,
    pub(crate) show_items_with_alt: bool,
    pub(crate) skip_exchange_alerts: bool,
    pub(crate) skip_intro: bool,
    pub(crate) skip_notice: bool,
    pub(crate) server: Option<ServerEndpoint>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleOperation {
    Load,
    Unload,
    Launch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LifecycleOutcome {
    pub(crate) operation: LifecycleOperation,
    pub(crate) pid: u32,
    pub(crate) changed: bool,
    pub(crate) darpc_loaded: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManagementError {
    pub(crate) code: String,
    pub(crate) message: String,
    pub(crate) pid: Option<u32>,
}

impl ManagementError {
    fn new(code: impl Into<String>, message: impl Into<String>, pid: Option<u32>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            pid,
        }
    }
}

pub(crate) trait LifecycleControl: Send + Sync {
    fn load(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError>;
    fn unload(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError>;
    fn launch(&self, options: &LaunchOptions) -> Result<LifecycleOutcome, ManagementError>;
}

pub(crate) struct LoaderControl {
    loader_path: PathBuf,
    dll_path: PathBuf,
    operation_lock: Mutex<()>,
}

impl LoaderControl {
    #[must_use]
    pub(crate) fn new(loader_path: PathBuf, dll_path: PathBuf) -> Self {
        Self {
            loader_path,
            dll_path,
            operation_lock: Mutex::new(()),
        }
    }

    fn invoke(
        &self,
        operation: LifecycleOperation,
        arguments: &[OsString],
    ) -> Result<LifecycleOutcome, ManagementError> {
        let _operation = self
            .operation_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let output = Command::new(&self.loader_path)
            .arg("--json")
            .args(arguments)
            .output()
            .map_err(|error| {
                ManagementError::new(
                    "loader_unavailable",
                    format!(
                        "failed to run loader `{}`: {error}",
                        self.loader_path.display()
                    ),
                    None,
                )
            })?;
        parse_output(operation, output)
    }

    fn launch_arguments(&self, options: &LaunchOptions) -> Vec<OsString> {
        let mut arguments = vec![OsString::from("launch")];
        if options.allow_multiple {
            arguments.push(OsString::from("--allow-multiple"));
        }
        if options.show_items_with_alt {
            arguments.push(OsString::from("--show-items-with-alt"));
        }
        if let Some(server) = options.server.as_ref() {
            arguments.push(OsString::from("--server"));
            arguments.push(OsString::from(format!("{}:{}", server.host, server.port)));
        }
        if options.skip_intro {
            arguments.push(OsString::from("--skip-intro"));
        }
        if options.skip_notice {
            arguments.push(OsString::from("--skip-notice"));
        }
        if options.skip_exchange_alerts {
            arguments.push(OsString::from("--skip-exchange-alerts"));
        }
        arguments.push(options.client_path.as_os_str().to_owned());
        arguments.push(self.dll_path.as_os_str().to_owned());
        arguments
    }
}

impl LifecycleControl for LoaderControl {
    fn load(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError> {
        self.invoke(
            LifecycleOperation::Load,
            &[
                OsString::from("attach"),
                OsString::from(pid.to_string()),
                self.dll_path.as_os_str().to_owned(),
            ],
        )
    }

    fn unload(&self, pid: u32) -> Result<LifecycleOutcome, ManagementError> {
        self.invoke(
            LifecycleOperation::Unload,
            &[
                OsString::from("detach"),
                OsString::from(pid.to_string()),
                self.dll_path.as_os_str().to_owned(),
            ],
        )
    }

    fn launch(&self, options: &LaunchOptions) -> Result<LifecycleOutcome, ManagementError> {
        self.invoke(LifecycleOperation::Launch, &self.launch_arguments(options))
    }
}

#[derive(Deserialize)]
struct LoaderOutput {
    ok: bool,
    command: Option<String>,
    pid: Option<u32>,
    changed: Option<bool>,
    darpc_loaded: Option<bool>,
    error: Option<LoaderError>,
}

#[derive(Deserialize)]
struct LoaderError {
    kind: String,
    message: String,
}

fn parse_output(
    operation: LifecycleOperation,
    output: Output,
) -> Result<LifecycleOutcome, ManagementError> {
    let parsed: LoaderOutput = serde_json::from_slice(&output.stdout).map_err(|error| {
        ManagementError::new(
            "invalid_loader_response",
            format!("loader returned malformed JSON: {error}"),
            None,
        )
    })?;

    if parsed.ok && output.status.success() {
        let command = parsed.command.as_deref().unwrap_or_default();
        let expected_command = match operation {
            LifecycleOperation::Load => "attach",
            LifecycleOperation::Unload => "detach",
            LifecycleOperation::Launch => "launch",
        };
        if command != expected_command {
            return Err(ManagementError::new(
                "invalid_loader_response",
                format!("loader reported command `{command}`, expected `{expected_command}`"),
                parsed.pid,
            ));
        }
        return Ok(LifecycleOutcome {
            operation,
            pid: parsed.pid.ok_or_else(|| {
                ManagementError::new(
                    "invalid_loader_response",
                    "loader success omitted the process ID",
                    None,
                )
            })?,
            changed: parsed.changed.ok_or_else(|| {
                ManagementError::new(
                    "invalid_loader_response",
                    "loader success omitted the changed flag",
                    parsed.pid,
                )
            })?,
            darpc_loaded: parsed.darpc_loaded.ok_or_else(|| {
                ManagementError::new(
                    "invalid_loader_response",
                    "loader success omitted the loaded-state flag",
                    parsed.pid,
                )
            })?,
        });
    }

    let error = parsed.error.ok_or_else(|| {
        ManagementError::new(
            "invalid_loader_response",
            format!(
                "loader exited with {} without a structured error",
                output.status
            ),
            parsed.pid,
        )
    })?;
    Err(ManagementError::new(error.kind, error.message, parsed.pid))
}

#[cfg(test)]
mod tests {
    use super::{LaunchOptions, LoaderControl, ServerEndpoint};
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn launch_arguments_expose_only_supported_options() {
        let control = LoaderControl::new(PathBuf::from("loader.exe"), PathBuf::from("darpc.dll"));
        let arguments = control.launch_arguments(&LaunchOptions {
            client_path: PathBuf::from("Darkages.exe"),
            allow_multiple: true,
            show_items_with_alt: true,
            skip_exchange_alerts: true,
            skip_intro: true,
            skip_notice: true,
            server: Some(ServerEndpoint {
                host: "127.0.0.1".into(),
                port: 2610,
            }),
        });

        assert_eq!(
            arguments,
            [
                "launch",
                "--allow-multiple",
                "--show-items-with-alt",
                "--server",
                "127.0.0.1:2610",
                "--skip-intro",
                "--skip-notice",
                "--skip-exchange-alerts",
                "Darkages.exe",
                "darpc.dll",
            ]
            .map(OsString::from)
        );
        assert!(!arguments.iter().any(|argument| argument == "--"));
    }
}
