use hyper::{
    service::{make_service_fn, service_fn},
    Body, Client, Request, Response, Server, Uri,
};
use hyper_rustls::HttpsConnectorBuilder;
use std::convert::Infallible;
use std::env;
use std::net::SocketAddr;
use std::time::Instant;
use tracing::{debug, error, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() {
    println!("Print statement to stdout - starting s3-proxy");
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    info!("Starting s3-proxy");

    let s3_url = env::var("S3_URL").expect("S3_URL environment variable not set");
    info!("S3_URL set to {}", s3_url);

    let s3_base_uri = match s3_url.parse::<Uri>() {
        Ok(uri) => {
            info!("Parsed S3_URL successfully");
            uri
        }
        Err(e) => {
            error!("Invalid S3_URL: {}", e);
            std::process::exit(1);
        }
    };

    let addr = ([0, 0, 0, 0], 8090).into();

    let make_svc = make_service_fn(move |conn: &hyper::server::conn::AddrStream| {
        let s3_base_uri = s3_base_uri.clone();
        let remote_addr = conn.remote_addr();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                proxy_handler(req, s3_base_uri.clone(), remote_addr)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    info!("Listening on http://{}", addr);

    if let Err(e) = server.await {
        error!("Server error: {}", e);
    }
}

async fn proxy_handler(
    mut req: Request<Body>,
    s3_base_uri: Uri,
    remote_addr: SocketAddr,
) -> Result<Response<Body>, hyper::Error> {
    let start = Instant::now();

    info!(
        "Received request from {}: {} {}",
        remote_addr,
        req.method(),
        req.uri()
    );

    let https = HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http1()
        .build();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let mut parts = s3_base_uri.into_parts();
    parts.path_and_query = req.uri().path_and_query().cloned();
    let uri = match Uri::from_parts(parts) {
        Ok(uri) => {
            debug!("Constructed URI: {}", uri);
            uri
        }
        Err(e) => {
            error!("Failed to build URI: {}", e);
            let mut response = Response::new(Body::from("Internal Server Error"));
            *response.status_mut() = hyper::StatusCode::INTERNAL_SERVER_ERROR;
            return Ok(response);
        }
    };

    *req.uri_mut() = uri;
    req.headers_mut().remove("host");

    match client.request(req).await {
        Ok(resp) => {
            let duration = start.elapsed();
            let status = resp.status();
            info!(
                "Request to S3 successful: status {}, duration {:?}",
                status, duration
            );

            debug!("Response headers: {:?}", resp.headers());

            Ok(resp)
        }
        Err(e) => {
            let duration = start.elapsed();
            error!(
                "Error forwarding request to S3: {}, duration {:?}",
                e, duration
            );
            let mut response = Response::new(Body::from("Bad Gateway"));
            *response.status_mut() = hyper::StatusCode::BAD_GATEWAY;
            Ok(response)
        }
    }
}
