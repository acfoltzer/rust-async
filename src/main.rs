use lazy_static::lazy_static;
use std::net::SocketAddr;
use std::error::Error;
use std::net::ToSocketAddrs;
use tokio::io::{AsyncRead, AsyncWrite};
use hyper::client::connect::{Connected, Connection};
use tokio::net::TcpStream;
use tokio_tls;
use hyper::{Client,Uri,Body};
use tokio_tls::TlsStream;
use std::pin::Pin;
use futures::future::TryFutureExt;
use futures::{self, Future};
use futures::task::{self, Poll};
use std::io;


pub enum UpstreamConnectorTransport {
    Tls(TlsStream<TcpStream>),
}

impl AsyncRead for UpstreamConnectorTransport {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> Poll<tokio::io::Result<usize>> {
        match Pin::into_inner(self) {
            UpstreamConnectorTransport::Tls(stream) => Pin::new(stream).poll_read(context, buf),
        }
    }
}

impl AsyncWrite for UpstreamConnectorTransport {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, tokio::io::Error>> {
        match Pin::into_inner(self) {
            UpstreamConnectorTransport::Tls(stream) => Pin::new(stream).poll_write(context, buf),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        context: &mut task::Context<'_>,
    ) -> Poll<Result<(), tokio::io::Error>> {
        match Pin::into_inner(self) {
            UpstreamConnectorTransport::Tls(stream) => Pin::new(stream).poll_flush(context),
        }
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut task::Context<'_>,
    ) -> Poll<Result<(), tokio::io::Error>> {
        match Pin::into_inner(self) {
            UpstreamConnectorTransport::Tls(stream) => Pin::new(stream).poll_shutdown(context),
        }
    }
}

impl Connection for UpstreamConnectorTransport {
    fn connected(&self) -> Connected {
        match self {
            UpstreamConnectorTransport::Tls(stream) => stream.get_ref().connected(),
        }
    }
}


#[derive(Debug, Clone)]
pub struct UpstreamTlsConnector<'a> {
    pub socket_addr: &'a SocketAddr,
    tls_cx: &'a tokio_tls::TlsConnector,
}

impl<'a> UpstreamTlsConnector<'a> {
    pub fn new(socket_addr: &'a SocketAddr, tls_cx: &'a tokio_tls::TlsConnector) -> Self {
        UpstreamTlsConnector {
            socket_addr,
            tls_cx,
        }
    }
}

lazy_static! {
        static ref TLS_CONNECTOR: tokio_tls::TlsConnector =
                    tokio_tls::TlsConnector::from(native_tls::TlsConnector::builder().build().unwrap());
}

impl<'a> hyper::service::Service<hyper::Uri> for UpstreamTlsConnector<'a> {
    type Response = TlsStream<TcpStream>;
    type Error = std::io::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a >>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _uri: Uri) -> Self::Future {
        Box::pin(
            TcpStream::connect(self.socket_addr)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                .and_then(move |stream| {
                    static DOMAIN: &str = "example.org";
                    self.tls_cx
                        /*
   Compiling tls v0.1.0 (/home/def/p/proglets/minimal-rust/tls)
error[E0495]: cannot infer an appropriate lifetime for autoref due to conflicting requirements
   --> src/main.rs:111:26
    |
111 |                         .connect(&DOMAIN, stream)
    |                          ^^^^^^^
    |
note: first, the lifetime cannot outlive the anonymous lifetime #1 defined on the method body at 103:5...
   --> src/main.rs:103:5
    |
103 | /     fn call(&mut self, _uri: Uri) -> Self::Future {
104 | |         Box::pin(
105 | |             TcpStream::connect(self.socket_addr)
106 | |                 .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
...   |
114 | |         )
115 | |     }
    | |_____^
note: ...so that reference does not outlive borrowed content
   --> src/main.rs:109:21
    |
109 |                     self.tls_cx
    |                     ^^^^^^^^^^^
    = note: but, the lifetime must be valid for the static lifetime...
note: ...so that the expression is assignable
   --> src/main.rs:104:9
    |
104 | /         Box::pin(
105 | |             TcpStream::connect(self.socket_addr)
106 | |                 .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
107 | |                 .and_then(move |stream| {
...   |
113 | |                 }),
114 | |         )
    | |_________^
    = note: expected  `std::pin::Pin<std::boxed::Box<(dyn core::future::future::Future<Output = std::result::Result<tokio_tls::TlsStream<tokio::net::tcp::stream::TcpStream>, std::io::Error>> + std::marker::Send + 'static)>>`
               found  `std::pin::Pin<std::boxed::Box<dyn core::future::future::Future<Output = std::result::Result<tokio_tls::TlsStream<tokio::net::tcp::stream::TcpStream>, std::io::Error>> + std::marker::Send>>`

error: aborting due to previous error

For more information about this error, try `rustc --explain E0495`.
error: could not compile `tls`.

To learn more, run the command again with --verbose.
                        */
                        .connect(&DOMAIN, stream)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                }),
        )
    }
}

pub enum UpstreamConnector<'a> {
    Tls(UpstreamTlsConnector<'a>),
}

impl<'a> UpstreamConnector<'a> {
    pub fn new(socket_addr: &'a SocketAddr, tls_cx: &'a tokio_tls::TlsConnector) -> Self {
        UpstreamConnector::Tls(UpstreamTlsConnector::new(socket_addr, tls_cx))
    }
}

impl<'a> Clone for UpstreamConnector<'a> {
    fn clone(&self) -> Self {
        match self {
            UpstreamConnector::Tls(UpstreamTlsConnector {
                socket_addr,
                tls_cx,
            }) => UpstreamConnector::Tls(UpstreamTlsConnector::new(*socket_addr, *tls_cx)),
        }
    }
}


impl<'a> hyper::service::Service<Uri> for UpstreamConnector<'a> {
    type Response = UpstreamConnectorTransport;
    type Error = std::io::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a >>;

    fn poll_ready(&mut self, context: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self {
            UpstreamConnector::Tls(upstream_connector) => upstream_connector.poll_ready(context),
        }
    }

    fn call(&mut self, uri: Uri) -> Self::Future {
        match self {
            UpstreamConnector::Tls(upstream_connector) => Box::pin(
                upstream_connector
                    .call(uri)
                    .map_ok(|x| UpstreamConnectorTransport::Tls(x)),
            ) as Self::Future,
        }
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut tls_builder = native_tls::TlsConnector::builder();
    tls_builder.danger_accept_invalid_hostnames(true);
    let tls_cx = tokio_tls::TlsConnector::from(tls_builder.build().unwrap());
    let addr = "151.101.1.57:443"
        .to_socket_addrs()?
        .next()
        .ok_or("failed to resolve address")?;

    let connector = UpstreamConnector::Tls(UpstreamTlsConnector::new(&addr, &tls_cx));
    let client: Client<_, Body> = Client::builder().build(connector.clone());

    let res = client.get(Uri::from_static("https://httpbin.org/ip")).await?;
    println!("status: {}", res.status());
    let buf = hyper::body::to_bytes(res).await?;
    println!("body: {:?}", buf);

    Ok(())
}
