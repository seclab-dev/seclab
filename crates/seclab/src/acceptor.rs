//! 同端口支持 HTTP 自动重定向至 HTTPS 的自定义 Acceptor 实现。

use axum_server::accept::Accept;
use axum_server::tls_rustls::{RustlsAcceptor, RustlsConfig};
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

/// 自定义 Acceptor，用于嗅探 TLS 和明文 HTTP
#[derive(Clone)]
pub struct HttpOrHttpsAcceptor {
    rustls_acceptor: Arc<RustlsAcceptor>,
}

impl HttpOrHttpsAcceptor {
    pub fn new(config: RustlsConfig) -> Self {
        Self {
            rustls_acceptor: Arc::new(RustlsAcceptor::new(config)),
        }
    }
}

impl<S> Accept<TcpStream, S> for HttpOrHttpsAcceptor
where
    S: Send + 'static,
{
    type Stream = MaybeTlsStream;
    type Service = S;
    type Future =
        Pin<Box<dyn Future<Output = Result<(Self::Stream, Self::Service), io::Error>> + Send>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let rustls_acceptor = self.rustls_acceptor.clone();
        Box::pin(async move {
            let mut buf = [0u8; 1];
            // 嗅探首字节，但不从 TCP 缓冲区移除数据
            stream.peek(&mut buf).await?;

            if buf[0] == 0x16 {
                // 如果是 TLS 握手包，交给 rustls 进行处理
                let (tls_stream, s) = rustls_acceptor
                    .accept(stream, service)
                    .await
                    .map_err(io::Error::other)?;
                Ok((MaybeTlsStream::Tls(Box::new(tls_stream)), s))
            } else {
                // 如果是普通明文 HTTP，后台处理 301 重定向后关闭连接
                tokio::spawn(async move {
                    if let Err(e) = handle_http_redirect(stream).await {
                        tracing::error!("Failed to redirect HTTP connection: {:?}", e);
                    }
                });
                // 返回一个虚拟流让 axum 侧静默处理（触发 EOF 优雅退出）
                Ok((MaybeTlsStream::Dummy, service))
            }
        })
    }
}

/// 支持明文与加密流量的包装枚举
pub enum MaybeTlsStream {
    Tls(Box<<RustlsAcceptor as Accept<TcpStream, ()>>::Stream>),
    Dummy,
}

impl AsyncRead for MaybeTlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_read(cx, buf),
            Self::Dummy => Poll::Ready(Ok(())), // 立即 EOF 退出
        }
    }
}

impl AsyncWrite for MaybeTlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_write(cx, buf),
            Self::Dummy => Poll::Ready(Ok(buf.len())),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_flush(cx),
            Self::Dummy => Poll::Ready(Ok(())),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Tls(stream) => Pin::new(stream.as_mut()).poll_shutdown(cx),
            Self::Dummy => Poll::Ready(Ok(())),
        }
    }
}

/// 读取明文 HTTP 头并重定向到对应的 HTTPS 地址
async fn handle_http_redirect(mut stream: TcpStream) -> io::Result<()> {
    let mut buf = [0u8; 1024];
    // 读取 HTTP 头部，设置 3 秒超时防止慢速攻击
    let n = match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => n,
        _ => return Ok(()),
    };

    let request_str = String::from_utf8_lossy(&buf[..n]);
    let mut lines = request_str.lines();

    if let Some(request_line) = lines.next() {
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() >= 2 {
            let path = parts[1];
            let mut host = String::new();

            for line in lines {
                if line.to_lowercase().starts_with("host:") {
                    host = line["host:".len()..].trim().to_string();
                    break;
                }
            }

            if !host.is_empty() {
                let response = format!(
                    "HTTP/1.1 301 Moved Permanently\r\n\
                    Location: https://{}{}\r\n\
                    Content-Length: 0\r\n\
                    Connection: close\r\n\r\n",
                    host, path
                );
                stream.write_all(response.as_bytes()).await?;
                let _ = stream.flush().await;
            }
        }
    }
    Ok(())
}
