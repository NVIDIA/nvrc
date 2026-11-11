// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 NVIDIA CORPORATION

//! Services supplied by measured extensions, started after GPU setup and before
//! handing PID 1 to kata-agent. Only roots returned by mount_all are consumed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, ErrorKind};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::FileTypeExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

use crate::{execute, nvrc::NVRC};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const SERVICES: &str = "etc/nvrc/services.d";

#[derive(Default)]
struct Service {
    path: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    wait_socket: Option<PathBuf>,
    timeout_secs: Option<u64>,
}

// Values are literal text after the first '='. Only arg may repeat. This is a
// line-based service format, not TOML or shell syntax.
fn parse(content: &str) -> Result<Service> {
    let mut service = Service::default();
    let mut seen = BTreeSet::new();
    for line in content
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid("expected key=value"))?;
        if key != "arg" && !seen.insert(key) {
            return Err(invalid(format!("duplicate key {key}")).into());
        }
        match key {
            "path" => service.path = value.into(),
            "arg" => service.args.push(value.into()),
            "wait_socket" => service.wait_socket = Some(value.into()),
            "timeout_secs" => service.timeout_secs = Some(value.parse()?),
            _ if key.starts_with("env.") => {
                service.env.insert(key[4..].into(), value.into());
            }
            _ => return Err(invalid(format!("unknown key {key}")).into()),
        }
    }
    Ok(service)
}

struct PlannedService {
    name: String,
    executable: PathBuf,
    service: Service,
}

impl PlannedService {
    fn command(&self) -> Result<Command> {
        let program = self
            .executable
            .to_str()
            .ok_or_else(|| invalid("non-UTF8 executable"))?;
        let mut command = execute::command(program, &self.service.args);
        command.envs(&self.service.env);
        Ok(command)
    }
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message.into())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn contained(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = path.canonicalize()?;
    if !path.starts_with(root) {
        return Err(invalid(format!("path escapes extension: {}", path.display())).into());
    }
    Ok(path)
}

fn plan(roots: &[PathBuf]) -> Result<Vec<PlannedService>> {
    let mut roots = roots.to_vec();
    roots.sort();
    let mut names = BTreeSet::new();
    let mut plan = Vec::new();
    for root in roots {
        let root = root.canonicalize()?;
        let extension = root
            .file_name()
            .and_then(|s| s.to_str())
            .filter(|s| valid_id(s))
            .ok_or_else(|| invalid("invalid extension name"))?;
        let directory = root.join(SERVICES);
        match fs::symlink_metadata(&directory) {
            Err(e) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        let mut files = fs::read_dir(contained(&root, &directory)?)?
            .map(|e| e.map(|e| e.path()))
            .collect::<io::Result<Vec<_>>>()?;
        files.sort();
        for file in files
            .into_iter()
            .filter(|p| p.extension().is_some_and(|s| s == "conf"))
        {
            let id = file
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|s| valid_id(s))
                .ok_or_else(|| invalid("invalid service filename"))?;
            let file = contained(&root, &file)?;
            let service = parse(&fs::read_to_string(&file)?)
                .map_err(|e| invalid(format!("{}: {e}", file.display())))?;
            let name = format!("{extension}/{id}");
            if !names.insert(name.clone()) {
                return Err(invalid(format!("duplicate service {name}")).into());
            }
            let relative = Path::new(&service.path);
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(
                    invalid(format!("{name}: expected extension-relative executable")).into(),
                );
            }
            let executable = contained(&root, &root.join(relative))?;
            if !executable.is_file() {
                return Err(invalid(format!("{name}: executable is not a file")).into());
            }
            match (&service.wait_socket, service.timeout_secs) {
                (Some(socket), Some(1..=300))
                    if socket.starts_with("/run")
                        && socket
                            .components()
                            .all(|c| matches!(c, Component::RootDir | Component::Normal(_))) => {}
                (None, None) => {}
                _ => {
                    return Err(invalid(format!(
                        "{name}: socket requires /run path and timeout 1..300"
                    ))
                    .into())
                }
            }
            if service
                .env
                .iter()
                .any(|(k, v)| k.is_empty() || k.contains(['=', '\0']) || v.contains('\0'))
                || service.args.iter().any(|a| a.contains('\0'))
            {
                return Err(invalid(format!("{name}: invalid argument or environment")).into());
            }
            plan.push(PlannedService {
                name,
                executable,
                service,
            });
        }
    }
    Ok(plan)
}

fn wait_ready(
    mut try_wait: impl FnMut() -> io::Result<Option<ExitStatus>>,
    socket: Option<&Path>,
    timeout: Duration,
    mut is_socket: impl FnMut(&Path) -> io::Result<bool>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = try_wait()? {
            return Err(invalid(format!("service exited during startup: {status}")).into());
        }
        let Some(socket) = socket else { return Ok(()) };
        match is_socket(socket) {
            Ok(true) => return Ok(()),
            Ok(_) => return Err(invalid("readiness path is not a Unix socket").into()),
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(ErrorKind::TimedOut, "service readiness timed out").into());
        }
        // poll sleeps on PID 1's single thread without introducing a worker.
        match nix::poll::poll(&mut [], 20u16) {
            Ok(_) | Err(nix::errno::Errno::EINTR) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

trait ServiceProcess {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>>;
    fn stop(&mut self);
    fn exit_fd(&self) -> io::Result<OwnedFd>;
    fn track(self, init: &mut NVRC, name: &str);
}

impl ServiceProcess for Child {
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.try_wait()
    }

    fn stop(&mut self) {
        let _ = self.kill();
        let _ = self.wait();
    }

    fn exit_fd(&self) -> io::Result<OwnedFd> {
        // A pidfd survives PID recycling and remains readable after kata-agent
        // reaps the service. CLOEXEC closes the parent copy at agent handoff.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, self.id(), 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
    }

    fn track(self, init: &mut NVRC, name: &str) {
        init.track_daemon(name, self);
    }
}

/// Start every declared service. The measured extension opts in by shipping a
/// definition; no writable rootfs or workload-controlled definitions are read.
pub fn start(init: &mut NVRC, roots: &[PathBuf]) -> Result<Vec<OwnedFd>> {
    start_with(init, roots, |planned| Ok(planned.command()?.spawn()?))
}

fn start_with<P: ServiceProcess>(
    init: &mut NVRC,
    roots: &[PathBuf],
    mut spawn: impl FnMut(&PlannedService) -> Result<P>,
) -> Result<Vec<OwnedFd>> {
    let mut exits = Vec::new();
    for planned in plan(roots)? {
        let Service {
            wait_socket,
            timeout_secs,
            ..
        } = &planned.service;
        if let Some(socket) = wait_socket {
            match fs::symlink_metadata(socket) {
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
                Ok(_) => {
                    return Err(
                        invalid(format!("{}: readiness path already exists", planned.name)).into(),
                    )
                }
            }
        }
        log::info!("starting extension service {}", planned.name);
        let mut child = spawn(&planned)?;
        if let Err(error) = wait_ready(
            || child.try_wait(),
            wait_socket.as_deref(),
            Duration::from_secs(timeout_secs.unwrap_or(0)),
            |path| fs::symlink_metadata(path).map(|metadata| metadata.file_type().is_socket()),
        ) {
            child.stop();
            return Err(invalid(format!("{}: {error}", planned.name)).into());
        }
        let fd = child.exit_fd().inspect_err(|_| child.stop())?;
        exits.push(fd);
        child.track(init, &planned.name);
    }
    Ok(exits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_arguments_and_environment() {
        let service = parse(
            "# helper\npath=bin/helper\narg=--name\narg=two words = literal\narg=\nenv.NAME=a=b\n",
        )
        .unwrap();
        assert_eq!(service.path, "bin/helper");
        assert_eq!(service.args, ["--name", "two words = literal", ""]);
        assert_eq!(service.env["NAME"], "a=b");
    }

    #[test]
    fn rejects_unknown_duplicate_and_malformed_fields() {
        for content in [
            "unknown=value",
            "path=a\npath=b",
            "env.NAME=a\nenv.NAME=b",
            "path",
            "timeout_secs=no",
        ] {
            assert!(parse(content).is_err(), "{content}");
        }
    }
    use std::os::unix::{fs::symlink, process::ExitStatusExt};

    const SPEC: &str = "path=bin/helper\n";

    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::Builder::new()
            .prefix("extension-")
            .tempdir()
            .unwrap();
        fs::create_dir_all(dir.path().join(SERVICES)).unwrap();
        fs::create_dir(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin/helper"), "test executable").unwrap();
        fs::write(dir.path().join(SERVICES).join("helper.conf"), SPEC).unwrap();
        dir
    }

    #[test]
    fn discovers_and_validates_before_spawning() {
        let dir = fixture();
        let planned = plan(&[dir.path().into()]).unwrap();
        assert_eq!(planned.len(), 1);
        assert_eq!(
            planned[0].executable,
            dir.path().join("bin/helper").canonicalize().unwrap()
        );
        assert!(plan(&[dir.path().into(), dir.path().into()]).is_err());
    }

    #[test]
    fn declared_helper_command_preserves_arguments_and_environment() {
        let dir = fixture();
        fs::write(
            dir.path().join(SERVICES).join("helper.conf"),
            format!("{SPEC}arg=--name\narg=two words = literal\narg=\nenv.NAME=a=b\n"),
        )
        .unwrap();
        let planned = plan(&[dir.path().into()]).unwrap();
        let command = planned[0].command().unwrap();
        assert_eq!(command.get_program(), planned[0].executable);
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--name", "two words = literal", ""]
        );
        assert!(command
            .get_envs()
            .any(|(key, value)| { key == "NAME" && value == Some(std::ffi::OsStr::new("a=b")) }));
    }

    #[derive(Default)]
    struct FakeProcess {
        events: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
        status: Option<ExitStatus>,
        monitor_fails: bool,
    }

    impl ServiceProcess for FakeProcess {
        fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            Ok(self.status)
        }

        fn stop(&mut self) {
            self.events.borrow_mut().push("stop".into());
        }

        fn exit_fd(&self) -> io::Result<OwnedFd> {
            self.events.borrow_mut().push("monitor".into());
            match self.monitor_fails {
                true => Err(io::Error::from(ErrorKind::PermissionDenied)),
                false => fs::File::open("/dev/null").map(Into::into),
            }
        }

        fn track(self, _: &mut NVRC, name: &str) {
            self.events.borrow_mut().push(format!("track {name}"));
        }
    }

    #[test]
    fn starts_services_in_order_and_retains_exit_descriptors() {
        let dir = fixture();
        fs::write(dir.path().join(SERVICES).join("aaa.conf"), SPEC).unwrap();
        let planned = plan(&[dir.path().into()]).unwrap();
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let exits = start_with(&mut NVRC::default(), &[dir.path().into()], |service| {
            events.borrow_mut().push(format!("spawn {}", service.name));
            Ok(FakeProcess {
                events: events.clone(),
                ..Default::default()
            })
        })
        .unwrap();
        assert_eq!(exits.len(), 2);
        assert_eq!(
            *events.borrow(),
            planned
                .iter()
                .flat_map(|service| [
                    format!("spawn {}", service.name),
                    "monitor".into(),
                    format!("track {}", service.name),
                ])
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn startup_failures_stop_the_child_without_tracking_or_continuing() {
        let dir = fixture();
        fs::write(dir.path().join(SERVICES).join("later.conf"), SPEC).unwrap();
        for (status, monitor_fails, expected) in [
            (Some(ExitStatus::from_raw(0)), false, vec!["spawn", "stop"]),
            (
                Some(ExitStatus::from_raw(3 << 8)),
                false,
                vec!["spawn", "stop"],
            ),
            (None, true, vec!["spawn", "monitor", "stop"]),
        ] {
            let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let result = start_with(&mut NVRC::default(), &[dir.path().into()], |_| {
                events.borrow_mut().push("spawn".into());
                Ok(FakeProcess {
                    events: events.clone(),
                    status,
                    monitor_fails,
                })
            });
            assert!(result.is_err());
            assert_eq!(*events.borrow(), expected);
        }
        assert!(
            start_with::<FakeProcess>(&mut NVRC::default(), &[dir.path().into()], |_| {
                Err(io::Error::from(ErrorKind::PermissionDenied).into())
            })
            .is_err()
        );
    }

    #[test]
    fn rejects_existing_readiness_path_before_spawning() {
        let dir = fixture();
        fs::write(
            dir.path().join(SERVICES).join("helper.conf"),
            format!("{SPEC}wait_socket=/run\ntimeout_secs=1\n"),
        )
        .unwrap();
        assert!(
            start_with::<FakeProcess>(&mut NVRC::default(), &[dir.path().into()], |_| {
                panic!("must reject the existing readiness path before spawning")
            })
            .is_err()
        );
    }

    #[test]
    fn absent_definitions_are_a_noop() {
        let dir = tempfile::Builder::new()
            .prefix("extension-")
            .tempdir()
            .unwrap();
        assert!(start(&mut NVRC::default(), &[dir.path().into()])
            .unwrap()
            .is_empty());
        fs::create_dir_all(dir.path().join(SERVICES)).unwrap();
        fs::write(dir.path().join(SERVICES).join("README"), "ignored").unwrap();
        assert!(start(&mut NVRC::default(), &[dir.path().into()])
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_bad_contracts_and_traversal() {
        let dir = fixture();
        for bad in [
            format!("{SPEC}path=bin/helper\n"),
            SPEC.replace("bin/helper", "../helper"),
            SPEC.replace("bin/helper", "/bin/true"),
            format!("unknown=true\n{SPEC}"),
            format!("{SPEC}wait_socket=/tmp/test\ntimeout_secs=1\n"),
            format!("{SPEC}wait_socket=/run/test\ntimeout_secs=0\n"),
            format!("{SPEC}wait_socket=/run/test\ntimeout_secs=301\n"),
            format!("{SPEC}wait_socket=/run/test\n"),
            format!("{SPEC}timeout_secs=1\n"),
            format!("{SPEC}env.=value\n"),
            format!("{SPEC}env.NAME=bad\0value\n"),
            format!("{SPEC}arg=bad\0value\n"),
            SPEC.replace("bin/helper", "bin"),
            SPEC.replace("bin/helper", ""),
        ] {
            fs::write(dir.path().join(SERVICES).join("helper.conf"), bad).unwrap();
            assert!(plan(&[dir.path().into()]).is_err());
        }
    }

    #[test]
    fn validates_all_extensions_before_starting_any_service() {
        let dir = tempfile::tempdir().unwrap();
        let roots: Vec<_> = ["aaa", "zzz"].map(|name| dir.path().join(name)).into();
        for root in &roots {
            fs::create_dir_all(root.join(SERVICES)).unwrap();
            fs::create_dir(root.join("bin")).unwrap();
            fs::write(root.join("bin/helper"), "helper").unwrap();
            fs::write(root.join(SERVICES).join("helper.conf"), SPEC).unwrap();
        }
        fs::write(roots[1].join(SERVICES).join("helper.conf"), "unknown=value").unwrap();
        assert!(
            start_with::<FakeProcess>(&mut NVRC::default(), &roots, |_| {
                panic!("must validate the last extension before starting the first")
            })
            .is_err()
        );
        fs::write(roots[1].join(SERVICES).join("helper.conf"), SPEC).unwrap();
        let events = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let reversed: Vec<_> = roots.into_iter().rev().collect();
        let exits = start_with(&mut NVRC::default(), &reversed, |_| {
            Ok(FakeProcess {
                events: events.clone(),
                ..Default::default()
            })
        })
        .unwrap();
        assert_eq!(exits.len(), 2);
        assert_eq!(
            *events.borrow(),
            ["monitor", "track aaa/helper", "monitor", "track zzz/helper",]
        );
    }

    #[test]
    fn rejects_service_definitions_outside_the_owning_extension() {
        let dir = fixture();
        let outside = tempfile::tempdir().unwrap();
        let definition = outside.path().join("outside.conf");
        fs::write(&definition, SPEC).unwrap();
        symlink(&definition, dir.path().join(SERVICES).join("escape.conf")).unwrap();
        assert!(plan(&[dir.path().into()]).is_err());
        let root = tempfile::Builder::new()
            .prefix("extension-")
            .tempdir()
            .unwrap();
        fs::create_dir_all(root.path().join("etc/nvrc")).unwrap();
        symlink(outside.path(), root.path().join(SERVICES)).unwrap();
        assert!(plan(&[root.path().into()]).is_err());
    }

    #[test]
    fn rejects_invalid_service_names_and_service_directories() {
        let dir = fixture();
        fs::write(dir.path().join(SERVICES).join("bad.name.conf"), SPEC).unwrap();
        assert!(plan(&[dir.path().into()]).is_err());
        let root = tempfile::Builder::new()
            .prefix("extension-")
            .tempdir()
            .unwrap();
        fs::create_dir_all(root.path().join("etc")).unwrap();
        fs::write(root.path().join("etc/nvrc"), "not a directory").unwrap();
        assert!(plan(&[root.path().into()]).is_err());
    }

    #[test]
    fn rejects_symlinks_outside_extension() {
        let dir = fixture();
        symlink("/bin/sh", dir.path().join("bin/escape")).unwrap();
        fs::write(
            dir.path().join(SERVICES).join("helper.conf"),
            SPEC.replace("bin/helper", "bin/escape"),
        )
        .unwrap();
        assert!(plan(&[dir.path().into()]).is_err());
    }

    #[test]
    fn readiness_checks_socket_type_timeout_and_exit() {
        let socket = Path::new("/run/ready.sock");
        let timeout = wait_ready(
            || Ok(None),
            Some(socket),
            Duration::ZERO,
            |_| Err(io::Error::from(ErrorKind::NotFound)),
        )
        .unwrap_err();
        assert_eq!(
            timeout.downcast_ref::<io::Error>().unwrap().kind(),
            ErrorKind::TimedOut
        );
        assert!(wait_ready(|| Ok(None), Some(socket), Duration::ZERO, |_| Ok(true)).is_ok());
        assert!(wait_ready(|| Ok(None), Some(socket), Duration::ZERO, |_| Ok(false)).is_err());
        assert!(wait_ready(|| Ok(None), None, Duration::ZERO, |_| unreachable!()).is_ok());
        for status in [0, 3 << 8] {
            assert!(wait_ready(
                || Ok(Some(ExitStatus::from_raw(status))),
                Some(socket),
                Duration::ZERO,
                |_| unreachable!(),
            )
            .is_err());
        }
        assert!(wait_ready(
            || Err(io::Error::from(ErrorKind::PermissionDenied)),
            None,
            Duration::ZERO,
            |_| unreachable!(),
        )
        .is_err());
        assert!(wait_ready(
            || Ok(None),
            Some(socket),
            Duration::ZERO,
            |_| Err(io::Error::from(ErrorKind::PermissionDenied)),
        )
        .is_err());
    }
}
