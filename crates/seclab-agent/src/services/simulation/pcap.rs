//! 零依赖自研 Rust 原生协程流量取证多路复用分发中心 (PcapMuxHub & Packet Sniffer)
//! 基于 Linux AF_PACKET 原始套接字接收数据包并智能裁剪链路层包头，支持 100% 零依赖取证。

use seclab_security::client::build_mtls_client;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use super::common::CleanupGuard;

/// 全局 Raw 套接字抓包数据帧
#[derive(Debug, Clone)]
pub struct RawPacketFrame {
    pub timestamp_sec: u32,
    pub timestamp_usec: u32,
    pub payload: Vec<u8>, // 已裁剪掉以太网/SLL头部的纯 IP 报文
}

/// 活跃实例的抓包取证槽
struct ForensicSlot {
    pcap_writer_tx: mpsc::Sender<RawPacketFrame>,
    cancel_tx: oneshot::Sender<()>,
    watchdog_cancel_tx: Option<oneshot::Sender<()>>,
}

/// 全局多路分发中心
pub struct PcapMuxHub {
    active_slots: Arc<Mutex<HashMap<u16, ForensicSlot>>>,
    global_listener_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

pub static GLOBAL_PCAP_HUB: OnceLock<PcapMuxHub> = OnceLock::new();

impl Default for PcapMuxHub {
    fn default() -> Self {
        Self::new()
    }
}

impl PcapMuxHub {
    pub fn global() -> &'static Self {
        GLOBAL_PCAP_HUB.get_or_init(Self::new)
    }

    pub fn new() -> Self {
        Self {
            active_slots: Arc::new(Mutex::new(HashMap::new())),
            global_listener_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// 注册一个端口的活跃抓包槽，并返回用于接收取消信号的 oneshot 接收器
    pub async fn start_capture(
        &self,
        port: u16,
        instance_id: &str,
        rule_id: &str,
        node_id: &str,
        callback_url: &str,
    ) -> anyhow::Result<String> {
        let mut slots = self.active_slots.lock().await;

        // 如果已经存在，直接返回
        if slots.contains_key(&port) {
            return Ok(format!("Capture already active on port {}", port));
        }

        let timestamp = chrono::Utc::now().timestamp_millis();
        let filename = format!("pcap_{}_{}_{}.pcap", instance_id, port, timestamp);
        let file_path = format!("/tmp/{}", filename);

        let (pcap_tx, pcap_rx) = mpsc::channel::<RawPacketFrame>(1000);
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();

        // 启动单独的 Tokio Task 异步写入 PCAP 物理文件
        let instance_id_clone = instance_id.to_string();
        let rule_id_clone = rule_id.to_string();
        let node_id_clone = node_id.to_string();
        let callback_clone = callback_url.to_string();
        let file_path_clone = file_path.clone();

        tokio::spawn(async move {
            if let Err(err) = run_pcap_writer(
                file_path_clone,
                pcap_rx,
                cancel_rx,
                port,
                instance_id_clone,
                rule_id_clone,
                node_id_clone,
                callback_clone,
            )
            .await
            {
                error!("[PCAP Writer] Error for port {}: {:?}", port, err);
            }
        });

        let (w_cancel_tx, w_cancel_rx) = oneshot::channel::<()>();

        // 插入抓包槽
        slots.insert(
            port,
            ForensicSlot {
                pcap_writer_tx: pcap_tx,
                cancel_tx,
                watchdog_cancel_tx: Some(w_cancel_tx),
            },
        );

        info!(
            "[PcapMuxHub] Registered forensic capture slot for port {}",
            port
        );

        // 启动 5 分钟 (300 秒) Watchdog 超时防爆机制，到期自动调用 stop_capture
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(300)) => {
                    warn!("[WATCHDOG] Self-contained Rust capture on port {} reached safety timeout limit. Auto-stopping...", port);
                    if PcapMuxHub::global().stop_capture(port).await {
                        info!("[WATCHDOG] Safety timer triggered stop successfully for port {}.", port);
                    }
                }
                _ = w_cancel_rx => {
                    info!("[WATCHDOG] Safety timer for port {} successfully cancelled due to manual stop.", port);
                }
            }
        });

        // 启动全局网卡监听协程 (如果尚未拉起)
        let mut handle_lock = self.global_listener_handle.lock().await;
        if handle_lock.is_none() {
            let slots_clone = self.active_slots.clone();
            let handle = tokio::spawn(async move {
                if let Err(err) = run_global_raw_socket_listener(slots_clone).await {
                    error!(
                        "[PcapMuxHub] Global listener encountered fatal error: {:?}",
                        err
                    );
                }
            });
            *handle_lock = Some(handle);
            info!("[PcapMuxHub] Global raw socket packet sniffer started.");
        }

        Ok(filename)
    }

    /// 停止一个端口的活跃抓包槽并立即回传
    pub async fn stop_capture(&self, port: u16) -> bool {
        let mut slots = self.active_slots.lock().await;
        if let Some(slot) = slots.remove(&port) {
            let _ = slot.cancel_tx.send(());
            if let Some(w_cancel) = slot.watchdog_cancel_tx {
                let _ = w_cancel.send(());
            }
            info!(
                "[PcapMuxHub] Signalled cancel for port {} capture slot.",
                port
            );

            // 如果没有更多的活跃抓包槽，关闭全局监听协程以释放资源
            if slots.is_empty() {
                let mut handle_lock = self.global_listener_handle.lock().await;
                if let Some(handle) = handle_lock.take() {
                    handle.abort();
                    info!(
                        "[PcapMuxHub] Global raw socket listener aborted as no active capture slots remain."
                    );
                }
            }
            return true;
        }
        false
    }
}

/// 异步将捕获到的包帧写入物理 PCAP 文件中，并在优雅结束后将其回传控制端
#[allow(clippy::too_many_arguments)]
async fn run_pcap_writer(
    file_path: String,
    mut pcap_rx: mpsc::Receiver<RawPacketFrame>,
    mut cancel_rx: oneshot::Receiver<()>,
    port: u16,
    instance_id: String,
    rule_id: String,
    node_id: String,
    callback_url: String,
) -> anyhow::Result<()> {
    info!(
        "[PCAP Writer] Initializing output file '{}' for instance {} on port {}",
        file_path, instance_id, port
    );

    let mut file = File::create(&file_path).await?;

    // 1. 写入 PCAP 全局首部 (Global Header, 24 字节)
    // Magic Number (4字节) = 0xa1b2c3d4 (微秒分辨率)
    // Major (2字节) = 2, Minor (2字节) = 4
    // TimeZone (4字节) = 0, SigFigs (4字节) = 0
    // SnapLen (4字节) = 65535
    // LinkType (4字节) = 101 (LINKTYPE_RAW, 纯 IP 报文，Wireshark 完美自适应无需链路层头部)
    let mut global_header = Vec::with_capacity(24);
    global_header.extend_from_slice(&0xa1b2c3d4u32.to_ne_bytes());
    global_header.extend_from_slice(&2u16.to_ne_bytes());
    global_header.extend_from_slice(&4u16.to_ne_bytes());
    global_header.extend_from_slice(&0i32.to_ne_bytes());
    global_header.extend_from_slice(&0u32.to_ne_bytes());
    global_header.extend_from_slice(&65535u32.to_ne_bytes());
    global_header.extend_from_slice(&101u32.to_ne_bytes());

    file.write_all(&global_header).await?;
    file.flush().await?;

    let pcap_path_buf = std::path::PathBuf::from(&file_path);
    // 物理文件防线清理 Guard，离开作用域时 100% 自愈擦除物理临时文件，零垃圾残留
    let _guard = CleanupGuard {
        path: pcap_path_buf.clone(),
    };

    let mut is_cancelled = false;

    loop {
        tokio::select! {
            _ = &mut cancel_rx => {
                info!("[PCAP Writer] Stop signal received for port {}.", port);
                is_cancelled = true;
                break;
            }
            frame_opt = pcap_rx.recv() => {
                let Some(frame) = frame_opt else {
                    break;
                };

                let len = frame.payload.len() as u32;

                // 2. 写入 Record Header (16 字节)
                // Timestamp Sec (4字节), Timestamp Usec (4字节)
                // Captured Len (4字节), Original Len (4字节)
                let mut record_header = Vec::with_capacity(16);
                record_header.extend_from_slice(&frame.timestamp_sec.to_ne_bytes());
                record_header.extend_from_slice(&frame.timestamp_usec.to_ne_bytes());
                record_header.extend_from_slice(&len.to_ne_bytes());
                record_header.extend_from_slice(&len.to_ne_bytes());

                file.write_all(&record_header).await?;
                file.write_all(&frame.payload).await?;
            }
        }
    }

    // 优雅刷盘退出
    file.flush().await?;
    drop(file);

    // 只有在主动或超时优雅关闭，并且文件非空时才进行回传控制端
    if is_cancelled {
        let file_bytes = tokio::fs::read(&pcap_path_buf).await?;
        if !file_bytes.is_empty() {
            let has_packets = file_bytes.len() > 24;
            let filename = pcap_path_buf
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string();

            // 智能反查 client_ip/client_port 以兼容后台对账审计，若在常驻抓包时，可设置为实例监听端口或特定默认源
            let pcap_callback_url = if callback_url.ends_with("/log") {
                let base = &callback_url[..callback_url.len() - 4];
                format!("{}/pcap", base)
            } else {
                format!("{}/pcap", callback_url)
            };
            let pcap_callback_url = crate::config::adjust_callback_url(&pcap_callback_url);

            let client = build_mtls_client("seclab");

            if let Ok(c) = client {
                let mut form = reqwest::multipart::Form::new()
                    .text("nodeId", node_id)
                    .text("ruleId", rule_id)
                    .text("clientIp", "0.0.0.0") // 常驻模式捕获网卡全局，对账日志统一用端口映射
                    .text("clientPort", port.to_string());

                if has_packets {
                    let part =
                        reqwest::multipart::Part::bytes(file_bytes).file_name(filename.clone());
                    form = form.part("pcapFile", part);
                    info!(
                        "[PCAP Writer] Uploading captured raw PCAP '{}' ({} bytes) for port {}",
                        filename,
                        filename.len(),
                        port
                    );
                } else {
                    form = form.text("isEmpty", "true");
                    info!(
                        "[PCAP Writer] PCAP is empty (<= 24 bytes) for port {}. Skipping bytes upload.",
                        port
                    );
                }

                match c.post(&pcap_callback_url).multipart(form).send().await {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            info!(
                                "[PCAP Writer] PCAP state successfully reported for port {}",
                                port
                            );
                        } else {
                            error!("[PCAP Writer] Report failed with status: {}", resp.status());
                        }
                    }
                    Err(err) => {
                        error!("[PCAP Writer] Post error: {:?}", err);
                    }
                }
            }
        }
    }

    Ok(())
}

/// 解析数据包，识别以太网和 Linux SLL 头部，并智能剥离出纯 IPv4 IP 报文，提取 TCP 源/目的端口
fn parse_ipv4_and_ports(packet: &[u8]) -> Option<(u16, u16, usize)> {
    if packet.len() < 20 {
        return None;
    }

    let mut ip_start = None;

    // 1. 尝试以太网头 (14 字节)
    if packet.len() >= 14 + 20 {
        let proto = u16::from_be_bytes([packet[12], packet[13]]);
        let ver_ihl = packet[14];
        let version = ver_ihl >> 4;
        if proto == 0x0800 && version == 4 {
            ip_start = Some(14);
        }
    }

    // 2. 尝试 Linux SLL (Cooked) 头 (16 字节)
    if ip_start.is_none() && packet.len() >= 16 + 20 {
        let proto = u16::from_be_bytes([packet[14], packet[15]]);
        let ver_ihl = packet[16];
        let version = ver_ihl >> 4;
        if proto == 0x0800 && version == 4 {
            ip_start = Some(16);
        }
    }

    let offset = ip_start?;
    let ip_packet = &packet[offset..];

    let protocol = ip_packet[9];
    if protocol != 6 {
        // TCP Protocol
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

/// Linux 原生 AF_PACKET 原始套接字嗅探协程
async fn run_global_raw_socket_listener(
    active_slots: Arc<Mutex<HashMap<u16, ForensicSlot>>>,
) -> anyhow::Result<()> {
    // 仅在 Linux 上运行
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::io::AsRawFd;

        let fd = unsafe {
            libc::socket(
                libc::PF_PACKET,
                libc::SOCK_RAW,
                (libc::ETH_P_ALL as u16).to_be() as i32,
            )
        };

        if fd < 0 {
            let err = std::io::Error::last_os_error();
            error!(
                "[Raw Sniffer] Failed to open PF_PACKET socket (requires cap_net_raw or root): {:?}",
                err
            );
            return Err(err.into());
        }

        // 设置为非阻塞模式，以便完美融合 Tokio AsyncFd
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags >= 0 {
            let _ = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        }

        let async_fd = tokio::io::unix::AsyncFd::new(fd)?;
        info!("[Raw Sniffer] Raw socket listening successfully configured on AF_PACKET.");

        let mut buf = vec![0u8; 65535];

        loop {
            // 等待可读
            let mut guard = async_fd.readable().await?;

            // 尝试读取物理网卡帧
            match guard.try_io(|inner| {
                let res = unsafe {
                    libc::recv(
                        inner.as_raw_fd(),
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                        0,
                    )
                };
                if res < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(res as usize)
                }
            }) {
                Ok(Ok(bytes_read)) => {
                    let packet = &buf[..bytes_read];

                    // 智能解析端口并提取纯 IP 包偏移
                    if let Some((src_port, dst_port, ip_offset)) = parse_ipv4_and_ports(packet) {
                        let slots = active_slots.lock().await;

                        // 关联匹配：只要该 TCP 流量的源端口或目的端口属于注册抓包槽，就多路复制分发进去
                        let matched_port = if slots.contains_key(&src_port) {
                            Some(src_port)
                        } else if slots.contains_key(&dst_port) {
                            Some(dst_port)
                        } else {
                            None
                        };

                        if let Some((target_port, slot)) =
                            matched_port.and_then(|port| slots.get(&port).map(|s| (port, s)))
                        {
                            let now = chrono::Utc::now();
                            let frame = RawPacketFrame {
                                timestamp_sec: now.timestamp() as u32,
                                timestamp_usec: now.timestamp_subsec_micros(),
                                payload: packet[ip_offset..].to_vec(),
                            };
                            // 快速往队列分发，如溢出则记录警告但绝不阻塞网卡主嗅探协程
                            if let Err(err) = slot.pcap_writer_tx.try_send(frame) {
                                warn!(
                                    "[Raw Sniffer] Drop packet on port {}: {:?}",
                                    target_port, err
                                );
                            }
                        }
                    }
                }
                Ok(Err(ref e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    continue;
                }
                Ok(Err(e)) => {
                    error!("[Raw Sniffer] Read error: {:?}", e);
                    break;
                }
                Err(_would_block) => {
                    continue;
                }
            }
        }

        unsafe { libc::close(fd) };
    }

    // 非 Linux 平台桩逻辑，防止编译挂掉
    #[cfg(not(target_os = "linux"))]
    {
        info!("[Raw Sniffer] Non-Linux platform mockup. Idle loop...");
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
    }

    Ok(())
}
