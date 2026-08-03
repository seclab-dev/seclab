//! 套件工作负载流量取证服务：基于 Linux AF_PACKET 原始套接字捕获端口流量并生成 PCAP。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{error, info, warn};

const PCAP_MAX_DURATION_SECS: u64 = 300;

#[cfg(target_os = "linux")]
struct InterfaceListener {
    interface: String,
    async_fd: tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>,
}

#[derive(Debug, Clone)]
struct RawPacketFrame {
    timestamp_sec: u32,
    timestamp_usec: u32,
    payload: Vec<u8>,
}

struct PcapSlot {
    suite_instance_id: Option<String>,
    workload_id: Option<String>,
    pcap_writer_tx: mpsc::Sender<RawPacketFrame>,
    cancel_tx: oneshot::Sender<()>,
    done_rx: oneshot::Receiver<anyhow::Result<Vec<u8>>>,
    watchdog_cancel_tx: Option<oneshot::Sender<()>>,
}

/// Agent 内部共享的原生抓包分发中心。
pub struct PcapMuxHub {
    active_slots: Arc<Mutex<HashMap<String, PcapSlot>>>,
    port_index: Arc<Mutex<HashMap<u16, String>>>,
    global_listener_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

static GLOBAL_PCAP_HUB: OnceLock<PcapMuxHub> = OnceLock::new();

impl PcapMuxHub {
    /// 返回进程内全局抓包分发中心。
    pub fn global() -> &'static Self {
        GLOBAL_PCAP_HUB.get_or_init(Self::new)
    }

    fn new() -> Self {
        Self {
            active_slots: Arc::new(Mutex::new(HashMap::new())),
            port_index: Arc::new(Mutex::new(HashMap::new())),
            global_listener_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// 启动指定宿主机端口的抓包任务。
    pub async fn start_capture(&self, capture_id: String, port: u16) -> anyhow::Result<()> {
        self.start_capture_for_workload(capture_id, port, None, None)
            .await
    }

    /// 启动指定工作负载宿主机端口的抓包任务。
    pub async fn start_capture_for_workload(
        &self,
        capture_id: String,
        port: u16,
        suite_instance_id: Option<String>,
        workload_id: Option<String>,
    ) -> anyhow::Result<()> {
        let mut port_index = self.port_index.lock().await;
        if port_index.contains_key(&port) {
            anyhow::bail!("capture already active on port {port}");
        }

        let (pcap_tx, pcap_rx) = mpsc::channel::<RawPacketFrame>(1000);
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = oneshot::channel::<anyhow::Result<Vec<u8>>>();
        let file_path = capture_file_path(&capture_id);

        tokio::spawn(async move {
            let result = run_pcap_writer(file_path, pcap_rx, cancel_rx).await;
            let _ = done_tx.send(result);
        });

        let (watchdog_cancel_tx, watchdog_cancel_rx) = oneshot::channel::<()>();
        let capture_id_for_watchdog = capture_id.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(PCAP_MAX_DURATION_SECS)) => {
                    warn!(capture_id = %capture_id_for_watchdog, port, "pcap capture reached timeout limit");
                    let _ = PcapMuxHub::global().stop_capture(&capture_id_for_watchdog).await;
                }
                _ = watchdog_cancel_rx => {}
            }
        });

        self.active_slots.lock().await.insert(
            capture_id.clone(),
            PcapSlot {
                suite_instance_id,
                workload_id,
                pcap_writer_tx: pcap_tx,
                cancel_tx,
                done_rx,
                watchdog_cancel_tx: Some(watchdog_cancel_tx),
            },
        );
        port_index.insert(port, capture_id.clone());
        drop(port_index);

        if let Err(err) = self.ensure_global_listener().await {
            if let Some(slot) = self.active_slots.lock().await.remove(&capture_id) {
                let _ = slot.cancel_tx.send(());
                if let Some(watchdog_cancel_tx) = slot.watchdog_cancel_tx {
                    let _ = watchdog_cancel_tx.send(());
                }
            }
            self.port_index.lock().await.remove(&port);
            return Err(err);
        }
        Ok(())
    }

    /// 停止指定抓包任务并返回完整 PCAP 字节。
    pub async fn stop_capture(&self, capture_id: &str) -> anyhow::Result<Vec<u8>> {
        let Some(slot) = self.active_slots.lock().await.remove(capture_id) else {
            anyhow::bail!("pcap capture not found: {capture_id}");
        };
        self.port_index
            .lock()
            .await
            .retain(|_, current_capture_id| current_capture_id != capture_id);
        let _ = slot.cancel_tx.send(());
        if let Some(watchdog_cancel_tx) = slot.watchdog_cancel_tx {
            let _ = watchdog_cancel_tx.send(());
        }

        let bytes = slot
            .done_rx
            .await
            .context("pcap writer stopped before returning capture data")??;

        if self.active_slots.lock().await.is_empty()
            && let Some(handle) = self.global_listener_handle.lock().await.take()
        {
            handle.abort();
            info!("pcap raw socket listener stopped because no captures remain");
        }

        Ok(bytes)
    }

    /// 仅在抓包任务属于指定套件实例和工作负载时停止并返回数据。
    pub async fn stop_capture_owned(
        &self,
        capture_id: &str,
        suite_instance_id: &str,
        workload_id: &str,
    ) -> anyhow::Result<Vec<u8>> {
        {
            let slots = self.active_slots.lock().await;
            let Some(slot) = slots.get(capture_id) else {
                anyhow::bail!("pcap capture not found: {capture_id}");
            };
            if slot.suite_instance_id.as_deref() != Some(suite_instance_id)
                || slot.workload_id.as_deref() != Some(workload_id)
            {
                anyhow::bail!("pcap capture does not belong to suite workload");
            }
        }
        self.stop_capture(capture_id).await
    }

    /// 停止指定套件实例下的所有抓包任务，返回已停止的抓包 ID。
    pub async fn stop_captures_for_suite_instance(&self, suite_instance_id: &str) -> Vec<String> {
        let captures = {
            let slots = self.active_slots.lock().await;
            slots
                .iter()
                .filter(|(_, slot)| slot.suite_instance_id.as_deref() == Some(suite_instance_id))
                .map(|(capture_id, slot)| (capture_id.clone(), slot.workload_id.clone()))
                .collect::<Vec<_>>()
        };

        let mut stopped = Vec::new();
        for (capture_id, workload_id) in captures {
            match self.stop_capture(&capture_id).await {
                Ok(_) => {
                    info!(
                        capture_id = %capture_id,
                        workload_id = workload_id.as_deref().unwrap_or_default(),
                        "stopped suite workload pcap capture during cleanup"
                    );
                    stopped.push(capture_id);
                }
                Err(err) => {
                    warn!(capture_id = %capture_id, error = %err, "failed to stop suite workload pcap capture during cleanup")
                }
            }
        }
        stopped
    }

    async fn ensure_global_listener(&self) -> anyhow::Result<()> {
        let mut handle_lock = self.global_listener_handle.lock().await;
        if handle_lock.is_some() {
            return Ok(());
        }
        let active_slots = self.active_slots.clone();
        let port_index = self.port_index.clone();
        let listeners = prepare_interface_listeners().await?;
        let handle = tokio::spawn(async move {
            if let Err(err) =
                run_global_raw_socket_listener(active_slots, port_index, listeners).await
            {
                error!(error = %err, "pcap raw socket listener failed");
            }
        });
        *handle_lock = Some(handle);
        Ok(())
    }
}

async fn run_pcap_writer(
    file_path: PathBuf,
    mut pcap_rx: mpsc::Receiver<RawPacketFrame>,
    mut cancel_rx: oneshot::Receiver<()>,
) -> anyhow::Result<Vec<u8>> {
    let mut file = File::create(&file_path)
        .await
        .with_context(|| format!("failed to create pcap file {}", file_path.display()))?;
    file.write_all(&pcap_global_header()).await?;
    file.flush().await?;

    loop {
        tokio::select! {
            _ = &mut cancel_rx => {
                break;
            }
            frame_opt = pcap_rx.recv() => {
                let Some(frame) = frame_opt else {
                    break;
                };
                let len = frame.payload.len() as u32;
                file.write_all(&frame.timestamp_sec.to_ne_bytes()).await?;
                file.write_all(&frame.timestamp_usec.to_ne_bytes()).await?;
                file.write_all(&len.to_ne_bytes()).await?;
                file.write_all(&len.to_ne_bytes()).await?;
                file.write_all(&frame.payload).await?;
            }
        }
    }

    file.flush().await?;
    drop(file);
    let bytes = tokio::fs::read(&file_path)
        .await
        .with_context(|| format!("failed to read pcap file {}", file_path.display()))?;
    let _ = tokio::fs::remove_file(&file_path).await;
    Ok(bytes)
}

fn pcap_global_header() -> [u8; 24] {
    let mut header = [0u8; 24];
    header[0..4].copy_from_slice(&0xa1b2c3d4u32.to_ne_bytes());
    header[4..6].copy_from_slice(&2u16.to_ne_bytes());
    header[6..8].copy_from_slice(&4u16.to_ne_bytes());
    header[8..12].copy_from_slice(&0i32.to_ne_bytes());
    header[12..16].copy_from_slice(&0u32.to_ne_bytes());
    header[16..20].copy_from_slice(&65535u32.to_ne_bytes());
    header[20..24].copy_from_slice(&101u32.to_ne_bytes());
    header
}

fn capture_file_path(capture_id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{capture_id}.pcap"))
}

fn parse_ipv4_and_ports(packet: &[u8]) -> Option<(u16, u16, usize)> {
    if packet.len() < 20 {
        return None;
    }

    let mut ip_start = None;
    if packet.len() >= 34 {
        let proto = u16::from_be_bytes([packet[12], packet[13]]);
        let version = packet[14] >> 4;
        if proto == 0x0800 && version == 4 {
            ip_start = Some(14);
        }
    }

    if ip_start.is_none() && packet.len() >= 36 {
        let proto = u16::from_be_bytes([packet[14], packet[15]]);
        let version = packet[16] >> 4;
        if proto == 0x0800 && version == 4 {
            ip_start = Some(16);
        }
    }

    let offset = ip_start?;
    let ip_packet = &packet[offset..];
    if ip_packet.len() < 20 || ip_packet[9] != 6 {
        return None;
    }
    let ihl = (ip_packet[0] & 0x0f) as usize * 4;
    if ip_packet.len() < ihl + 4 {
        return None;
    }

    let tcp_packet = &ip_packet[ihl..];
    let src_port = u16::from_be_bytes([tcp_packet[0], tcp_packet[1]]);
    let dst_port = u16::from_be_bytes([tcp_packet[2], tcp_packet[3]]);
    Some((src_port, dst_port, offset))
}

async fn run_global_raw_socket_listener(
    active_slots: Arc<Mutex<HashMap<String, PcapSlot>>>,
    port_index: Arc<Mutex<HashMap<u16, String>>>,
    listeners: Vec<InterfaceListener>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let interfaces = listeners
            .iter()
            .map(|listener| listener.interface.clone())
            .collect::<Vec<_>>();
        info!(
            interfaces = ?interfaces,
            "pcap raw socket listener selected host interfaces"
        );

        let mut join_set = JoinSet::new();
        for listener in listeners {
            let active_slots = active_slots.clone();
            let port_index = port_index.clone();
            join_set.spawn(async move {
                run_interface_raw_socket_listener(listener, active_slots, port_index).await
            });
        }

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => error!(error = %err, "pcap interface listener failed"),
                Err(err) => error!(error = %err, "pcap interface listener task failed"),
            }
        }
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = active_slots;
        let _ = port_index;
        let _ = listeners;
        anyhow::bail!("pcap capture is only supported on Linux");
    }
}

#[cfg(target_os = "linux")]
async fn prepare_interface_listeners() -> anyhow::Result<Vec<InterfaceListener>> {
    let interfaces = capture_interfaces().await?;
    if interfaces.is_empty() {
        anyhow::bail!("no host network interface is eligible for pcap capture");
    }

    let mut listeners = Vec::new();
    let mut failures = Vec::new();
    for interface in interfaces {
        match open_interface_raw_socket(&interface) {
            Ok(async_fd) => listeners.push(InterfaceListener {
                interface,
                async_fd,
            }),
            Err(err) => failures.push(format!("{interface}: {err:#}")),
        }
    }

    if listeners.is_empty() {
        anyhow::bail!(
            "failed to bind pcap raw socket to host interfaces: {}",
            failures.join("; ")
        );
    }
    for failure in failures {
        warn!(failure, "pcap skipped host interface");
    }
    Ok(listeners)
}

#[cfg(not(target_os = "linux"))]
async fn prepare_interface_listeners() -> anyhow::Result<Vec<()>> {
    anyhow::bail!("pcap capture is only supported on Linux");
}

#[cfg(target_os = "linux")]
async fn capture_interfaces() -> anyhow::Result<Vec<String>> {
    let mut entries = tokio::fs::read_dir("/sys/class/net")
        .await
        .context("failed to read network interfaces")?;
    let mut interfaces = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_excluded_capture_interface(&name) {
            continue;
        }
        let operstate = tokio::fs::read_to_string(format!("/sys/class/net/{name}/operstate"))
            .await
            .unwrap_or_default();
        if !matches!(operstate.trim(), "up" | "unknown") {
            continue;
        }
        interfaces.push(name);
    }
    interfaces.sort();
    Ok(interfaces)
}

#[cfg(target_os = "linux")]
fn is_excluded_capture_interface(name: &str) -> bool {
    name == "lo"
        || name.starts_with("docker")
        || name.starts_with("br-")
        || name.starts_with("veth")
        || name.starts_with("cni")
        || name.starts_with("flannel")
        || name.starts_with("virbr")
        || name.starts_with("kube")
        || name.starts_with("tun")
        || name.starts_with("tap")
}

#[cfg(target_os = "linux")]
fn open_interface_raw_socket(
    interface: &str,
) -> anyhow::Result<tokio::io::unix::AsyncFd<std::os::fd::OwnedFd>> {
    use std::ffi::CString;
    use std::mem;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    let raw_fd = unsafe {
        libc::socket(
            libc::PF_PACKET,
            libc::SOCK_RAW,
            (libc::ETH_P_ALL as u16).to_be() as i32,
        )
    };
    if raw_fd < 0 {
        return Err(std::io::Error::last_os_error()).context(
            "failed to open PF_PACKET socket; agent requires cap_net_raw or root for pcap",
        );
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };

    let interface_c = CString::new(interface).context("invalid interface name")?;
    let if_index = unsafe { libc::if_nametoindex(interface_c.as_ptr()) };
    if if_index == 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to resolve interface index for {interface}"));
    }

    let mut addr: libc::sockaddr_ll = unsafe { mem::zeroed() };
    addr.sll_family = libc::AF_PACKET as libc::c_ushort;
    addr.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
    addr.sll_ifindex = if_index as i32;
    let bind_result = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &addr as *const libc::sockaddr_ll as *const libc::sockaddr,
            mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t,
        )
    };
    if bind_result < 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("failed to bind raw socket to {interface}"));
    }

    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags >= 0 {
        let _ = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    }

    tokio::io::unix::AsyncFd::new(fd).context("failed to register raw socket with tokio")
}

#[cfg(target_os = "linux")]
async fn run_interface_raw_socket_listener(
    listener: InterfaceListener,
    active_slots: Arc<Mutex<HashMap<String, PcapSlot>>>,
    port_index: Arc<Mutex<HashMap<u16, String>>>,
) -> anyhow::Result<()> {
    use std::os::fd::AsRawFd;

    let InterfaceListener {
        interface,
        async_fd,
    } = listener;
    let mut buf = vec![0u8; 65535];

    loop {
        let mut guard = async_fd.readable().await?;
        match guard.try_io(|inner| {
            let result = unsafe {
                libc::recv(
                    inner.as_raw_fd(),
                    buf.as_mut_ptr() as *mut libc::c_void,
                    buf.len(),
                    0,
                )
            };
            if result < 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(result as usize)
            }
        }) {
            Ok(Ok(bytes_read)) => {
                if let Some((src_port, dst_port, ip_offset)) =
                    parse_ipv4_and_ports(&buf[..bytes_read])
                {
                    let capture_id = {
                        let port_index = port_index.lock().await;
                        port_index
                            .get(&src_port)
                            .or_else(|| port_index.get(&dst_port))
                            .cloned()
                    };
                    if let Some(capture_id) = capture_id {
                        let now = chrono::Utc::now();
                        let frame = RawPacketFrame {
                            timestamp_sec: now.timestamp() as u32,
                            timestamp_usec: now.timestamp_subsec_micros(),
                            payload: buf[ip_offset..bytes_read].to_vec(),
                        };
                        let slots = active_slots.lock().await;
                        if let Some(slot) = slots.get(&capture_id) {
                            let _ = slot.pcap_writer_tx.try_send(frame);
                        }
                    }
                }
            }
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Ok(Err(err)) => {
                return Err(err).with_context(|| format!("failed to read from {interface}"));
            }
            Err(_) => {}
        }
    }
}
