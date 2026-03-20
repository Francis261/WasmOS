use crate::gui::GuiSubsystem;
use crate::network::NetworkSubsystem;
use crate::runtime::{AbiSelection, ProgramLaunchRequest};
use crate::scheduler::Scheduler;
use crate::vfs::VirtualFileSystem;
use anyhow::{Result, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{Mutex, RwLock};

pub struct Shell {
    scheduler: Arc<Scheduler>,
    vfs: Arc<RwLock<VirtualFileSystem>>,
    network: Arc<NetworkSubsystem>,
    gui: Arc<GuiSubsystem>,
    logs: Mutex<Vec<String>>,
    cwd: Mutex<String>,
    snapshot_path: String,
    startup_script_path: String,
    package_registry_path: String,
    package_catalog_path: String,
    package_hosts_path: String,
    command_aliases: Mutex<BTreeMap<String, String>>,
    package_registry: Mutex<BTreeMap<String, InstalledPackage>>,
    package_hosts: Mutex<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstalledPackage {
    name: String,
    module_path: String,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PackageCatalogEntry {
    module_path: String,
    dependencies: Vec<String>,
}

impl Shell {
    pub fn new(
        scheduler: Arc<Scheduler>,
        vfs: Arc<RwLock<VirtualFileSystem>>,
        network: Arc<NetworkSubsystem>,
        gui: Arc<GuiSubsystem>,
    ) -> Self {
        Self {
            scheduler,
            vfs,
            network,
            gui,
            logs: Mutex::new(Vec::new()),
            cwd: Mutex::new("/".to_string()),
            snapshot_path: ".wasmos_vfs_snapshot.json".to_string(),
            startup_script_path: ".wasmosrc".to_string(),
            package_registry_path: ".wasmos_packages.json".to_string(),
            package_catalog_path: ".wasmos_pkg_catalog.json".to_string(),
            package_hosts_path: ".wasmos_pkg_hosts.json".to_string(),
            command_aliases: Mutex::new(BTreeMap::new()),
            package_registry: Mutex::new(BTreeMap::new()),
            package_hosts: Mutex::new(vec![]),
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        self.boot().await?;
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();
        let mut stdout = io::stdout();

        loop {
            let prompt = self.render_prompt().await;
            stdout.write_all(prompt.as_bytes()).await?;
            stdout.flush().await?;

            let Some(line) = lines.next_line().await? else {
                break;
            };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if line == "exit" {
                break;
            }
            let response = match self.handle_command(line).await {
                Ok(response) => response,
                Err(error) => format!("error: {error}"),
            };
            stdout.write_all(response.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
        }

        Ok(())
    }

    async fn handle_command(&self, line: &str) -> Result<String> {
        let mut command_line = line.to_string();
        if let Some(expanded) = self.expand_alias(&command_line).await {
            command_line = expanded;
        }
        self.logs.lock().await.push(command_line.clone());
        let mut parts = command_line.split_whitespace();
        match parts.next().unwrap_or_default() {
            "echo" => Ok(command_line
                .strip_prefix("echo")
                .unwrap_or_default()
                .trim()
                .to_string()),
            "def" => {
                let name = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("def <name> <command...>"))?;
                let expansion = command_line
                    .splitn(3, ' ')
                    .nth(2)
                    .ok_or_else(|| anyhow::anyhow!("def <name> <command...>"))?
                    .trim()
                    .to_string();
                self.command_aliases
                    .lock()
                    .await
                    .insert(name.to_string(), expansion);
                Ok(format!("registered command alias: {name}"))
            }
            "spawn" => {
                let module_path = parts.next().unwrap_or_default().to_string();
                let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                let task_id = self
                    .scheduler
                    .spawn(ProgramLaunchRequest {
                        name: module_path.clone(),
                        module_path,
                        args,
                        env: BTreeMap::new(),
                        abi: AbiSelection::default(),
                    })
                    .await?;
                Ok(format!("spawned task {task_id}"))
            }
            "run" => {
                let module_path = parts.next().unwrap_or_default().to_string();
                let args = parts.map(ToString::to_string).collect::<Vec<_>>();
                let task_id = self
                    .scheduler
                    .spawn(ProgramLaunchRequest {
                        name: module_path.clone(),
                        module_path,
                        args,
                        env: BTreeMap::new(),
                        abi: AbiSelection::default(),
                    })
                    .await?;
                self.scheduler.run_ready_tasks(4).await?;
                Ok(format!("spawned task {task_id} and executed 4 rounds"))
            }
            "ps" => Ok(format!("{:?}", self.scheduler.list_tasks().await)),
            "resume" => {
                let task_id = parse_u64(parts.next(), "resume <task_id>")?;
                let resumed = self.scheduler.run_task_once(task_id).await?;
                Ok(if resumed {
                    format!("resumed task {task_id}")
                } else {
                    format!("task {task_id} is not in the ready queue")
                })
            }
            "kill" => {
                let task_id = parse_u64(parts.next(), "kill <task_id>")?;
                let killed = self.scheduler.kill(task_id).await;
                Ok(if killed {
                    format!("killed task {task_id}")
                } else {
                    format!("task {task_id} not found")
                })
            }
            "ls" => {
                let first = parts.next().unwrap_or(".");
                let (long, target) = if first == "-l" {
                    (true, parts.next().unwrap_or("."))
                } else {
                    (false, first)
                };
                let path = self.resolve_path(target).await;
                let entries = self.vfs.read().await.list_dir_for_task(0, path)?;
                if long {
                    Ok(entries
                        .into_iter()
                        .map(|entry| format!("{:?}\t{}\t{}", entry.kind, entry.len, entry.path))
                        .collect::<Vec<_>>()
                        .join("\n"))
                } else {
                    Ok(entries
                        .into_iter()
                        .map(|entry| entry.path.rsplit('/').next().unwrap_or("").to_string())
                        .collect::<Vec<_>>()
                        .join("  "))
                }
            }
            "cat" => {
                let path = parts.next().ok_or_else(|| anyhow::anyhow!("cat <path>"))?;
                let path = self.resolve_path(path).await;
                let bytes = self.vfs.read().await.read_file(path)?;
                Ok(String::from_utf8_lossy(&bytes).to_string())
            }
            "write" => {
                let mut split = line.splitn(3, ' ');
                let _cmd = split.next();
                let raw_path = split
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("write <path> <content>"))?;
                let path = self.resolve_path(raw_path).await;
                let content = split.next().unwrap_or_default();
                self.vfs
                    .write()
                    .await
                    .write_file(path, content.as_bytes().to_vec())?;
                self.persist_vfs_snapshot().await?;
                Ok(format!("wrote {} bytes", content.len()))
            }
            "rm" => {
                let path = parts.next().ok_or_else(|| anyhow::anyhow!("rm <path>"))?;
                let path = self.resolve_path(path).await;
                self.vfs.write().await.delete_for_task(0, &path)?;
                self.persist_vfs_snapshot().await?;
                Ok(format!("deleted {path}"))
            }
            "mkdir" => {
                let path = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("mkdir <path>"))?;
                let path = self.resolve_path(path).await;
                self.vfs.write().await.create_dir_for_task(0, &path)?;
                self.persist_vfs_snapshot().await?;
                Ok(format!("created {path}"))
            }
            "mount" => {
                let virtual_path = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("mount <virtual_path> <host_path>"))?;
                let virtual_path = self.resolve_path(virtual_path).await;
                let host_path = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("mount <virtual_path> <host_path>"))?;
                self.vfs
                    .write()
                    .await
                    .map_host_path(virtual_path.to_string(), host_path)?;
                self.persist_vfs_snapshot().await?;
                Ok(format!("mapped {virtual_path} -> {host_path}"))
            }
            "cd" => {
                let destination = self.resolve_path(parts.next().unwrap_or("/")).await;
                self.vfs.read().await.list_dir_for_task(0, &destination)?;
                *self.cwd.lock().await = destination.clone();
                Ok(format!("cwd: {destination}"))
            }
            "pwd" => Ok(self.cwd.lock().await.clone()),
            "pkg" => self.handle_pkg_command(parts.collect()).await,
            "Teditor" | "teditor" => {
                let path = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Teditor <file-path>"))?;
                let path = self.resolve_path(path).await;
                self.open_text_editor(path).await
            }
            "htop" => self.render_resource_overview().await,
            "bg" => {
                let module_path = parts
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("bg <module.wasm>"))?;
                let task_id = self
                    .scheduler
                    .spawn(ProgramLaunchRequest {
                        name: module_path.to_string(),
                        module_path: module_path.to_string(),
                        args: parts.map(ToString::to_string).collect(),
                        env: BTreeMap::new(),
                        abi: AbiSelection::default(),
                    })
                    .await?;
                Ok(format!("background task {task_id} started"))
            }
            "fg" => {
                let task_id = parse_u64(parts.next(), "fg <task_id>")?;
                let resumed = self.scheduler.run_task_once(task_id).await?;
                Ok(if resumed {
                    format!("foreground task {task_id} resumed")
                } else {
                    format!("task {task_id} not ready")
                })
            }
            "logs" => {
                let logs = self.logs.lock().await.clone();
                Ok(logs.join("\n"))
            }
            "net" => self.handle_net_command(parts.collect()).await,
            "window" => self.handle_window_command(parts.collect()).await,
            "sched" => Ok(format!("scheduler mode: {:?}", self.scheduler.mode())),
            "tick" => {
                self.scheduler.tick().await?;
                Ok("advanced scheduler by one tick".to_string())
            }
            "runloop" => {
                let rounds = parts
                    .next()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(8);
                self.scheduler.run_ready_tasks(rounds).await?;
                Ok(format!("executed {rounds} scheduling rounds"))
            }
            "runq" => {
                let rounds = parts
                    .next()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(8);
                self.scheduler.run_ready_tasks(rounds).await?;
                Ok(format!("executed {rounds} scheduling rounds"))
            }
            "wake" => {
                let channel = parts.next().unwrap_or_default();
                self.scheduler.notify_io(channel).await;
                Ok(format!("woke tasks waiting on {channel}"))
            }
            unknown => {
                if let Some(package) = self.package_registry.lock().await.get(unknown).cloned() {
                    let task_id = self
                        .scheduler
                        .spawn(ProgramLaunchRequest {
                            name: package.name.clone(),
                            module_path: package.module_path,
                            args: parts.map(ToString::to_string).collect(),
                            env: BTreeMap::new(),
                            abi: AbiSelection::default(),
                        })
                        .await?;
                    self.scheduler.run_ready_tasks(4).await?;
                    Ok(format!(
                        "executed package {} as task {task_id}",
                        package.name
                    ))
                } else {
                    Ok(format!("unknown command: {unknown}"))
                }
            }
        }
    }

    async fn handle_net_command(&self, args: Vec<&str>) -> Result<String> {
        if args.first().copied() != Some("policy") {
            bail!("net policy <show|allow|deny|capability> ...")
        }
        match args.get(1).copied().unwrap_or_default() {
            "show" => {
                let task_id = parse_u64(args.get(2).copied(), "net policy show <task_id>")?;
                let policy = self.network.policy(task_id).await;
                Ok(format!("{:?}", policy))
            }
            "allow" => {
                let task_id = parse_u64(args.get(2).copied(), "net policy allow <task_id> <host>")?;
                let host = args
                    .get(3)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("host required"))?;
                let mut policy = self.network.policy(task_id).await;
                if !policy.allowed_hosts.iter().any(|entry| entry == host) {
                    policy.allowed_hosts.push(host.to_string());
                }
                self.network.set_policy(task_id, policy).await;
                Ok(format!("allowed host {host} for task {task_id}"))
            }
            "deny" => {
                let task_id = parse_u64(args.get(2).copied(), "net policy deny <task_id> <host>")?;
                let host = args
                    .get(3)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("host required"))?;
                let mut policy = self.network.policy(task_id).await;
                if !policy.denied_hosts.iter().any(|entry| entry == host) {
                    policy.denied_hosts.push(host.to_string());
                }
                self.network.set_policy(task_id, policy).await;
                Ok(format!("denied host {host} for task {task_id}"))
            }
            "capability" => {
                let task_id = parse_u64(
                    args.get(2).copied(),
                    "net policy capability <task_id> <http|ws|tcp|remote> <on|off>",
                )?;
                let capability = args.get(3).copied().unwrap_or_default();
                let enabled = matches!(args.get(4).copied().unwrap_or("off"), "on" | "true" | "1");
                let mut policy = self.network.policy(task_id).await;
                match capability {
                    "http" => policy.allow_http = enabled,
                    "ws" => policy.allow_websocket = enabled,
                    "tcp" => policy.allow_tcp = enabled,
                    "remote" => policy.allow_remote = enabled,
                    _ => bail!("unknown capability: {capability}"),
                }
                self.network.set_policy(task_id, policy).await;
                Ok(format!("set {capability}={enabled} for task {task_id}"))
            }
            _ => bail!("net policy <show|allow|deny|capability> ..."),
        }
    }

    async fn handle_window_command(&self, args: Vec<&str>) -> Result<String> {
        match args.first().copied().unwrap_or_default() {
            "list" => {
                let host = self.gui.host_kind();
                let windows = self.gui.list_windows().await;
                let listing = windows
                    .into_iter()
                    .map(|window| {
                        format!(
                            "id={} task={} {}x{} title=\"{}\"",
                            window.id,
                            window.owner_task_id,
                            window.width,
                            window.height,
                            window.title
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(format!("host={host:?}\n{listing}"))
            }
            _ => bail!("window list"),
        }
    }

    async fn handle_pkg_command(&self, args: Vec<&str>) -> Result<String> {
        match args.first().copied().unwrap_or_default() {
            "host" => match args.get(1).copied().unwrap_or_default() {
                "list" => {
                    let hosts = self.package_hosts.lock().await;
                    if hosts.is_empty() {
                        Ok("no package hosts configured".to_string())
                    } else {
                        Ok(hosts.join("\n"))
                    }
                }
                "add" => {
                    let host = args
                        .get(2)
                        .copied()
                        .ok_or_else(|| anyhow::anyhow!("pkg host add <url>"))?;
                    let mut hosts = self.package_hosts.lock().await;
                    if !hosts.iter().any(|entry| entry == host) {
                        hosts.push(host.to_string());
                    }
                    drop(hosts);
                    self.persist_package_hosts().await?;
                    Ok(format!("added package host {host}"))
                }
                "remove" => {
                    let host = args
                        .get(2)
                        .copied()
                        .ok_or_else(|| anyhow::anyhow!("pkg host remove <url>"))?;
                    self.package_hosts
                        .lock()
                        .await
                        .retain(|entry| entry != host);
                    self.persist_package_hosts().await?;
                    Ok(format!("removed package host {host}"))
                }
                _ => bail!("pkg host <add|remove|list> ..."),
            },
            "list" => {
                let installed = self.package_registry.lock().await;
                if installed.is_empty() {
                    return Ok("no packages installed".to_string());
                }
                Ok(installed
                    .values()
                    .map(|pkg| format!("{} -> {}", pkg.name, pkg.module_path))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "install" => {
                let package = args
                    .get(1)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("pkg install <program>"))?;
                let mut catalog = self.load_package_catalog().unwrap_or_default();
                if !catalog.contains_key(package) {
                    for host in self.package_hosts.lock().await.clone() {
                        if let Ok(remote) = self.fetch_remote_catalog(&host).await {
                            catalog.extend(remote);
                        }
                    }
                }
                let mut planned = Vec::new();
                self.resolve_dependencies(&catalog, package, &mut planned)?;
                {
                    let mut installed = self.package_registry.lock().await;
                    for name in planned {
                        let entry = catalog
                            .get(&name)
                            .ok_or_else(|| anyhow::anyhow!("catalog entry missing for {name}"))?;
                        installed.insert(
                            name.clone(),
                            InstalledPackage {
                                name: name.clone(),
                                module_path: entry.module_path.clone(),
                                dependencies: entry.dependencies.clone(),
                            },
                        );
                    }
                }
                self.persist_package_registry().await?;
                Ok(format!("installed {package}"))
            }
            "remove" => {
                let package = args
                    .get(1)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("pkg remove <program>"))?;
                {
                    let installed = self.package_registry.lock().await;
                    let dependents = installed
                        .values()
                        .filter(|pkg| pkg.dependencies.iter().any(|dep| dep == package))
                        .map(|pkg| pkg.name.clone())
                        .collect::<Vec<_>>();
                    if !dependents.is_empty() {
                        bail!(
                            "cannot remove {package}; required by: {}",
                            dependents.join(", ")
                        );
                    }
                }
                let removed = self.package_registry.lock().await.remove(package);
                if removed.is_none() {
                    bail!("package `{package}` is not installed");
                }
                self.persist_package_registry().await?;
                Ok(format!("removed {package}"))
            }
            _ => bail!("pkg <install|remove|list> ..."),
        }
    }
}

impl Shell {
    async fn boot(&self) -> Result<()> {
        self.load_vfs_snapshot().await?;
        self.load_package_registry().await?;
        self.load_package_hosts().await?;
        self.run_startup_script().await
    }

    async fn run_startup_script(&self) -> Result<()> {
        let script = match fs::read_to_string(&self.startup_script_path) {
            Ok(script) => script,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        for line in script.lines() {
            let command = line.trim();
            if command.is_empty() || command.starts_with('#') {
                continue;
            }
            if let Err(error) = self.handle_command(command).await {
                self.logs
                    .lock()
                    .await
                    .push(format!("startup error for `{command}`: {error}"));
            }
        }
        Ok(())
    }

    async fn render_prompt(&self) -> String {
        let cwd = self.cwd.lock().await.clone();
        let display = self.truncated_path(&cwd);
        if cwd == "/" {
            "\u{1b}[1;32mwasmos@user\u{1b}[0m:$ ".to_string()
        } else {
            format!("\u{1b}[1;32mwasmos@user\u{1b}[0m:\u{1b}[1;34m[{display}]\u{1b}[0m$ ")
        }
    }

    fn truncated_path(&self, cwd: &str) -> String {
        let segments = cwd
            .trim_matches('/')
            .split('/')
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>();
        if segments.len() <= 3 {
            return segments.join("/");
        }
        format!("{}/.../{}", segments[0], segments[segments.len() - 1])
    }

    async fn expand_alias(&self, line: &str) -> Option<String> {
        let mut parts = line.split_whitespace();
        let command = parts.next()?;
        let alias = self.command_aliases.lock().await.get(command).cloned()?;
        let args = parts.collect::<Vec<_>>();
        Some(if args.is_empty() {
            alias
        } else {
            format!("{alias} {}", args.join(" "))
        })
    }

    fn load_package_catalog(&self) -> Result<BTreeMap<String, PackageCatalogEntry>> {
        let payload = fs::read_to_string(&self.package_catalog_path)?;
        let catalog: BTreeMap<String, PackageCatalogEntry> = serde_json::from_str(&payload)?;
        Ok(catalog)
    }

    async fn fetch_remote_catalog(
        &self,
        host: &str,
    ) -> Result<BTreeMap<String, PackageCatalogEntry>> {
        let base = host.trim_end_matches('/');
        let url = format!("{base}/packages.json");
        let client = Client::new();
        let response = client.get(url).send().await?;
        let response = response.error_for_status()?;
        let payload = response.text().await?;
        Ok(serde_json::from_str(&payload)?)
    }

    fn resolve_dependencies(
        &self,
        catalog: &BTreeMap<String, PackageCatalogEntry>,
        name: &str,
        planned: &mut Vec<String>,
    ) -> Result<()> {
        if planned.iter().any(|entry| entry == name) {
            return Ok(());
        }
        let package = catalog
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("package `{name}` not found in catalog"))?;
        for dependency in &package.dependencies {
            self.resolve_dependencies(catalog, dependency, planned)?;
        }
        planned.push(name.to_string());
        Ok(())
    }

    async fn resolve_path(&self, input: &str) -> String {
        let trimmed = input.trim();
        if trimmed.starts_with('/') {
            normalize_path(trimmed)
        } else {
            let cwd = self.cwd.lock().await.clone();
            if trimmed.is_empty() || trimmed == "." {
                normalize_path(&cwd)
            } else {
                normalize_path(&format!("{}/{}", cwd.trim_end_matches('/'), trimmed))
            }
        }
    }

    async fn load_vfs_snapshot(&self) -> Result<()> {
        self.vfs
            .write()
            .await
            .load_snapshot(&self.snapshot_path)
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn persist_vfs_snapshot(&self) -> Result<()> {
        self.vfs
            .read()
            .await
            .save_snapshot(&self.snapshot_path)
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    async fn load_package_registry(&self) -> Result<()> {
        let payload = match fs::read_to_string(&self.package_registry_path) {
            Ok(payload) => payload,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.package_registry.lock().await.clear();
                return Ok(());
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        let packages: BTreeMap<String, InstalledPackage> = serde_json::from_str(&payload)?;
        *self.package_registry.lock().await = packages;
        Ok(())
    }

    async fn persist_package_registry(&self) -> Result<()> {
        let payload = serde_json::to_string_pretty(&*self.package_registry.lock().await)?;
        fs::write(&self.package_registry_path, payload)?;
        Ok(())
    }

    async fn load_package_hosts(&self) -> Result<()> {
        let payload = match fs::read_to_string(&self.package_hosts_path) {
            Ok(payload) => payload,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                *self.package_hosts.lock().await = Vec::new();
                return Ok(());
            }
            Err(error) => return Err(anyhow::anyhow!(error.to_string())),
        };
        let hosts: Vec<String> = serde_json::from_str(&payload)?;
        *self.package_hosts.lock().await = hosts;
        Ok(())
    }

    async fn persist_package_hosts(&self) -> Result<()> {
        let payload = serde_json::to_string_pretty(&*self.package_hosts.lock().await)?;
        fs::write(&self.package_hosts_path, payload)?;
        Ok(())
    }

    async fn render_resource_overview(&self) -> Result<String> {
        let tasks = self.scheduler.list_tasks().await;
        let mem = fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|line| line.starts_with("VmRSS:"))
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "VmRSS: unavailable".to_string());
        Ok(format!("tasks: {}\n{}", tasks.len(), mem))
    }

    async fn open_text_editor(&self, mut path: String) -> Result<String> {
        let mut buffer = self
            .vfs
            .read()
            .await
            .read_file(path.clone())
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .unwrap_or_default();
        let mut clipboard = String::new();
        let stdin = BufReader::new(io::stdin());
        let mut lines = stdin.lines();
        let mut stdout = io::stdout();
        stdout
            .write_all(
                b"Teditor mode. Commands: :w, :q, :wq, :rename <file>, :select_all, :paste, :help\n",
            )
            .await?;
        loop {
            stdout.write_all(b"teditor> ").await?;
            stdout.flush().await?;
            let Some(line) = lines.next_line().await? else {
                break;
            };
            let line = line.trim_end();
            if line == ":q" {
                return Ok(format!("closed editor for {path}"));
            } else if line == ":w" {
                self.vfs
                    .write()
                    .await
                    .write_file(path.clone(), buffer.clone().into_bytes())?;
                self.persist_vfs_snapshot().await?;
                stdout.write_all(b"saved\n").await?;
            } else if line == ":wq" {
                self.vfs
                    .write()
                    .await
                    .write_file(path.clone(), buffer.clone().into_bytes())?;
                self.persist_vfs_snapshot().await?;
                return Ok(format!("saved and closed {path}"));
            } else if line.starts_with(":rename ") {
                let next = line.trim_start_matches(":rename ").trim();
                path = self.resolve_path(next).await;
                stdout
                    .write_all(format!("renamed target to {path}\n").as_bytes())
                    .await?;
            } else if line == ":select_all" {
                clipboard = buffer.clone();
                stdout.write_all(b"selected all\n").await?;
            } else if line == ":paste" {
                if !clipboard.is_empty() {
                    if !buffer.ends_with('\n') && !buffer.is_empty() {
                        buffer.push('\n');
                    }
                    buffer.push_str(&clipboard);
                }
            } else if line == ":help" {
                stdout
                    .write_all(
                        b":w save, :q quit, :wq save+quit, :rename <file>, :select_all, :paste\n",
                    )
                    .await?;
            } else {
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(line);
            }
        }
        Ok(format!("editor ended for {path}"))
    }
}

fn normalize_path(path: &str) -> String {
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            value => components.push(value),
        }
    }
    if components.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", components.join("/"))
    }
}

fn parse_u64(value: Option<&str>, usage: &str) -> Result<u64> {
    let Some(value) = value else {
        bail!("{usage}");
    };
    Ok(value.parse::<u64>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::GuiSubsystem;
    use crate::host::HostBridge;
    use crate::network::NetworkSubsystem;
    use crate::runtime::WasmRuntime;
    use crate::scheduler::Scheduler;
    use crate::vfs::VirtualFileSystem;

    fn shell_fixture() -> Shell {
        let host = Arc::new(HostBridge::detect());
        let vfs = Arc::new(RwLock::new(VirtualFileSystem::new()));
        let network = Arc::new(NetworkSubsystem::new(host.clone()));
        let gui = Arc::new(GuiSubsystem::new(host.clone()));
        let runtime = Arc::new(
            WasmRuntime::new(host, vfs.clone(), network.clone(), gui.clone())
                .expect("runtime should initialize"),
        );
        let scheduler = Arc::new(Scheduler::new(runtime));
        let mut shell = Shell::new(scheduler, vfs, network, gui);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        shell.snapshot_path = format!("/tmp/wasmos_test_snapshot_{nonce}.json");
        shell.package_registry_path = format!("/tmp/wasmos_test_pkg_registry_{nonce}.json");
        shell.package_catalog_path = format!("/tmp/wasmos_test_pkg_catalog_{nonce}.json");
        shell.package_hosts_path = format!("/tmp/wasmos_test_pkg_hosts_{nonce}.json");
        shell.startup_script_path = format!("/tmp/wasmos_test_startup_{nonce}.rc");
        shell
    }

    #[tokio::test]
    async fn cd_updates_cwd_and_relative_paths_work() {
        let shell = shell_fixture();
        shell
            .handle_command("mkdir /data")
            .await
            .expect("mkdir should succeed");
        shell
            .handle_command("cd /data")
            .await
            .expect("cd should succeed");
        shell
            .handle_command("write note.txt hello")
            .await
            .expect("relative write should succeed");
        let output = shell
            .handle_command("cat /data/note.txt")
            .await
            .expect("cat");
        assert_eq!(output, "hello");
        let pwd = shell.handle_command("pwd").await.expect("pwd should work");
        assert_eq!(pwd, "/data");
    }

    #[tokio::test]
    async fn package_install_list_remove_cycle() {
        let shell = shell_fixture();
        std::fs::write(
            &shell.package_catalog_path,
            r#"{
  "core-utils": { "module_path": "/pkgs/core_utils.wasm", "dependencies": [] },
  "text-editor": { "module_path": "/pkgs/text_editor.wasm", "dependencies": ["core-utils"] }
}"#,
        )
        .expect("catalog should write");

        let install = shell
            .handle_command("pkg install text-editor")
            .await
            .expect("install should work");
        assert!(install.contains("installed text-editor"));

        let list = shell
            .handle_command("pkg list")
            .await
            .expect("list should work");
        assert!(list.contains("core-utils"));
        assert!(list.contains("text-editor"));

        let remove_dep = shell.handle_command("pkg remove core-utils").await;
        assert!(
            remove_dep.is_err(),
            "dependency-protected remove should fail"
        );

        shell
            .handle_command("pkg remove text-editor")
            .await
            .expect("remove should work");
        shell
            .handle_command("pkg remove core-utils")
            .await
            .expect("dependency cleared remove should work");
    }
}
