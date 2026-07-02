//! 协议仿真运行器模块：按协议拆分 Agent 侧仿真引擎。

mod common;
mod ftp;
mod http;
mod imap;
mod mail_common;
mod pcap;
mod pop3;
mod rdp;
mod redis;
mod smtp;
mod ssh;

pub use common::{CleanupGuard, SimLogDraft};
pub use ftp::start_ftp_simulation;
pub use http::start_http_simulation;
pub use imap::start_imap_simulation;
pub use pcap::{PcapMuxHub, RawPacketFrame};
pub use pop3::start_pop3_simulation;
pub use rdp::start_rdp_simulation;
pub use redis::start_redis_simulation;
pub use seclab_contracts::simulation::{
    RedisCommandResponse, SimFtpConfig, SimHttpConfig, SimImapConfig, SimPop3Config, SimRdpConfig,
    SimRedisConfig, SimSmtpConfig, SimSshConfig,
};
pub use smtp::start_smtp_simulation;
pub use ssh::start_ssh_simulation;
