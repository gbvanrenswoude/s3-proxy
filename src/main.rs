use hyper::{
    client::HttpConnector,
    service::{make_service_fn, service_fn},
    Body, Client, Request, Response, Server, Uri,
};
use std::convert::Infallible;
use std::env;
use tracing::{error, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() {
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .init();

    let s3_url = env::var("S3_URL").expect("S3_URL environment variable not set");
    let s3_base_uri = s3_url.parse::<Uri>().expect("Invalid S3_URL");
    let addr = ([0, 0, 0, 0], 8080).into();

    let make_svc = make_service_fn(move |_conn| {
        let s3_base_uri = s3_base_uri.clone();
        async move {
            Ok::<_, Infallible>(service_fn(move |req| {
                proxy_handler(req, s3_base_uri.clone())
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
) -> Result<Response<Body>, hyper::Error> {
    let https = HttpConnector::new();
    let client = Client::builder().build::<_, hyper::Body>(https);

    let mut parts = s3_base_uri.into_parts();
    parts.path_and_query = req.uri().path_and_query().cloned();
    let uri = Uri::from_parts(parts).expect("Failed to build URI");

    *req.uri_mut() = uri;
    req.headers_mut().remove("host");
    client.request(req).await
}
