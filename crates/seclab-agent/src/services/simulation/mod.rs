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
pub use ftp::{SimFtpConfig, start_ftp_simulation};
pub use http::{ExploitPathConfig, SimHttpConfig, start_http_simulation};
pub use imap::{SimImapConfig, start_imap_simulation};
pub use pcap::{PcapMuxHub, RawPacketFrame};
pub use pop3::{SimPop3Config, start_pop3_simulation};
pub use rdp::{SimRdpConfig, start_rdp_simulation};
pub use redis::{RedisCommandResponse, SimRedisConfig, start_redis_simulation};
pub use smtp::{SimSmtpConfig, start_smtp_simulation};
pub use ssh::{SimSshConfig, start_ssh_simulation};
