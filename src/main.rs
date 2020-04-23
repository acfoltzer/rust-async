use futures::future::TryFutureExt;
use futures::task::{self, Poll};
use futures::{self, Future};
use hyper::client::connect::{Connected, Connection};
use hyper::{Body, Client, Uri};
use lazy_static::lazy_static;
use std::error::Error;
use std::io;
use std::net::SocketAddr;
use std::net::ToSocketAddrs;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_tls;
use tokio_tls::TlsStream;

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
pub struct UpstreamTlsConnector<'a: 'b, 'b> {
    pub socket_addr: SocketAddr,
    tls_cx: tokio_tls::TlsConnector,
    phantom1: std::marker::PhantomData<&'a Self>,
    phantom2: std::marker::PhantomData<&'b Self>,
}

impl<'a: 'b, 'b> UpstreamTlsConnector<'a, 'b> {
    pub fn new(socket_addr: SocketAddr, tls_cx: tokio_tls::TlsConnector) -> Self {
        UpstreamTlsConnector {
            socket_addr,
            tls_cx,
            phantom1: std::marker::PhantomData,
            phantom2: std::marker::PhantomData,
        }
    }
}

lazy_static! {
    static ref TLS_CONNECTOR: tokio_tls::TlsConnector =
        tokio_tls::TlsConnector::from(native_tls::TlsConnector::builder().build().unwrap());
}

impl<'a: 'b, 'b> UpstreamTlsConnector<'a, 'b> {
    fn gimme(&'a self) -> &'a tokio_tls::TlsConnector {
        &self.tls_cx
    }
}

impl<'a: 'b, 'b> hyper::service::Service<hyper::Uri> for UpstreamTlsConnector<'a, 'b> {
    type Response = TlsStream<TcpStream>;
    type Error = std::io::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'b>>;

    fn poll_ready(&mut self, _: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call<'c>(&mut self, _uri: Uri) -> Self::Future {
        fn handler<'a: 'b, 'b, S>(
            stream: S,
            tls_cx: &'a tokio_tls::TlsConnector,
        ) -> Pin<Box<dyn Future<Output = Result<TlsStream<S>, std::io::Error>> + Send + 'b>>
        where
            S: AsyncRead + AsyncWrite + std::marker::Unpin + Send + 'a,
        {
            static DOMAIN: &str = "example.org";
            Box::pin(
                tls_cx
                    .connect(&DOMAIN, stream)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e)),
            )
        }

        Box::pin(
            TcpStream::connect(self.socket_addr)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))
                .and_then(move |stream| handler(stream, self.gimme())),
        )
    }
}

pub enum UpstreamConnector<'a: 'b, 'b> {
    Tls(UpstreamTlsConnector<'a, 'b>),
}

impl<'a: 'b, 'b> UpstreamConnector<'a, 'b> {
    pub fn new(socket_addr: &'a SocketAddr, tls_cx: &'a tokio_tls::TlsConnector) -> Self {
        UpstreamConnector::Tls(UpstreamTlsConnector::new(
            socket_addr.clone(),
            tls_cx.clone(),
        ))
    }
}

impl<'a: 'b, 'b> Clone for UpstreamConnector<'a, 'b> {
    fn clone(&self) -> Self {
        match self {
            UpstreamConnector::Tls(UpstreamTlsConnector {
                socket_addr,
                tls_cx,
                phantom1: std::marker::PhantomData,
                phantom2: std::marker::PhantomData,
            }) => UpstreamConnector::Tls(UpstreamTlsConnector::new(
                socket_addr.clone(),
                tls_cx.clone(),
            )),
        }
    }
}

impl<'a: 'b, 'b> hyper::service::Service<Uri> for UpstreamConnector<'a, 'b> {
    type Response = UpstreamConnectorTransport;
    type Error = std::io::Error;

    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'a>>;

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

    let connector = UpstreamConnector::Tls(UpstreamTlsConnector::new(addr, tls_cx));
    let client: Client<_, Body> = Client::builder().build(connector.clone());

    let res = client
        .get(Uri::from_static("https://httpbin.org/ip"))
        .await?;
    println!("status: {}", res.status());
    let buf = hyper::body::to_bytes(res).await?;
    println!("body: {:?}", buf);

    Ok(())
}
