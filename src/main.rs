use hyper::body::to_bytes;
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Client, Request, Response, Server, Uri,
};
use hyper_rustls::HttpsConnectorBuilder;
use rustls::{client::{ServerCertVerifier, ServerCertVerified}, ServerName};
use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::time::timeout;
use tracing::{debug, error, info, instrument, warn};
use tracing_subscriber::{FmtSubscriber, EnvFilter};
use rand::Rng;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RETRIES: u32 = 3;

struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with debug level
    let subscriber = FmtSubscriber::builder()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("s3_proxy=debug".parse().unwrap())
                .add_directive("hyper=debug".parse().unwrap())
                .add_directive("hyper_rustls=debug".parse().unwrap())
        )
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    info!("Starting s3-proxy");

    let s3_url = env::var("S3_URL").expect("S3_URL environment variable not set");
    info!("S3_URL set to {}", s3_url);

    let s3_base_uri = s3_url.parse::<Uri>().map_err(|e| {
        error!("Invalid S3_URL: {}", e);
        e
    })?;

    let addr: SocketAddr = ([0, 0, 0, 0], 8092).into();

    // Create HTTPS client with certificate verification disabled (for testing only)
    let https = HttpsConnectorBuilder::new()
        .with_tls_config(
            rustls::ClientConfig::builder()
                .with_safe_defaults()
                .with_custom_certificate_verifier(Arc::new(NoVerifier))
                .with_no_client_auth()
        )
        .https_only()
        .enable_http1()
        .build();

    let client = Arc::new(Client::builder().build::<_, hyper::Body>(https));

    let make_svc = make_service_fn(move |conn: &hyper::server::conn::AddrStream| {
        let s3_base_uri = s3_base_uri.clone();
        let remote_addr = conn.remote_addr();
        let client = Arc::clone(&client);
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                handle_request(req, s3_base_uri.clone(), remote_addr, Arc::clone(&client))
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    info!("Listening on http://{}", addr);

    server.await?;

    Ok(())
}

#[instrument(skip(req, client))]
async fn handle_request(
    req: Request<Body>,
    s3_base_uri: Uri,
    remote_addr: SocketAddr,
    client: Arc<Client<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>>>,
) -> Result<Response<Body>, hyper::Error> {
    if req.uri().path() == "/healthz" {
        return Ok(Response::new(Body::from("OK")));
    }

    let start = Instant::now();

    let result = proxy_handler(req, s3_base_uri, remote_addr, client).await;

    let duration = start.elapsed();
    debug!("Request duration: {:?}", duration);

    result
}

async fn proxy_handler(
    req: Request<Body>,
    s3_base_uri: Uri,
    remote_addr: SocketAddr,
    client: Arc<Client<hyper_rustls::HttpsConnector<hyper::client::HttpConnector>>>,
) -> Result<Response<Body>, hyper::Error> {
    info!(
        "Received request from {}: {} {}",
        remote_addr,
        req.method(),
        req.uri()
    );

    if !is_valid_s3_request(&req) {
        warn!("Invalid S3 request received");
        return Ok(Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(Body::from("Invalid S3 request"))
            .unwrap());
    }

    let uri = match construct_uri(&s3_base_uri, req.uri()) {
        Ok(uri) => uri,
        Err(e) => {
            error!("Failed to construct URI: {}", e);
            return Ok(Response::builder()
                .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("Internal Server Error"))
                .unwrap());
        }
    };

    let method = req.method().clone();
    let headers = req.headers().clone();
    let body_bytes = to_bytes(req.into_body()).await?;

    for retry in 0..MAX_RETRIES {
        let mut new_req = Request::builder()
            .method(method.clone())
            .uri(uri.clone());

        for (name, value) in headers.iter() {
            if name != "host" {
                new_req = new_req.header(name, value);
            }
        }

        let new_req = new_req.body(Body::from(body_bytes.clone())).expect("Failed to build request");

        debug!("Sending request to S3: {:?}", new_req);
        match timeout(REQUEST_TIMEOUT, client.request(new_req)).await {
            Ok(Ok(resp)) => {
                let status = resp.status();
                info!("Response from S3: status {}", status);
                debug!("Response headers: {:?}", resp.headers());
                return Ok(resp);
            }
            Ok(Err(e)) => {
                error!(
                    "Error forwarding request to S3: {}, retry: {}",
                    e, retry
                );
                if retry < MAX_RETRIES - 1 {
                    let backoff = 2u64.pow(retry) * 1000 + rand::thread_rng().gen_range(0..1000);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                } else {
                    return Ok(Response::builder()
                        .status(hyper::StatusCode::BAD_GATEWAY)
                        .body(Body::from("Bad Gateway"))
                        .unwrap());
                }
            }
            Err(_) => {
                warn!("Request to S3 timed out, retry: {}", retry);
                if retry == MAX_RETRIES - 1 {
                    return Ok(Response::builder()
                        .status(hyper::StatusCode::GATEWAY_TIMEOUT)
                        .body(Body::from("Gateway Timeout"))
                        .unwrap());
                }
            }
        }
    }

    unreachable!()
}

fn construct_uri(base_uri: &Uri, request_uri: &Uri) -> Result<Uri, hyper::http::Error> {
    let mut parts = base_uri.clone().into_parts();
    let path = request_uri.path();
    let query = request_uri.query().map(|q| format!("?{}", q)).unwrap_or_default();
    parts.path_and_query = Some(format!("{}{}", path, query).parse().unwrap());
    Uri::from_parts(parts).map_err(|e| hyper::http::Error::from(e))
}

fn is_valid_s3_request(_req: &Request<Body>) -> bool {
    // Implement your S3 request validation logic here
    // For example, check if the path starts with a valid bucket name
    // or if the request contains required S3 headers
    true // Placeholder
}
