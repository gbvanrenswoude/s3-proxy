# S3 Proxy
Still very barebones S3 proxy. 

### Features
The intent is to front AWS S3 with a proxy, that then can be Network Isolated in rare cases of AWS S3 Access Points or Gateway Endpoints and Bucket Policies / ACL's not being sufficient.

### Design
The proxy is stateless, very high performant and able to handle a multitude of threads, making it easy to scale horizontally.

S3 proxy uses the `#[tokio::main]` macro, which sets up an asynchronous runtime provided by the Tokio library. It uses the Hyper library to create an HTTP server. Hyper is built on top of Tokio and is designed for high-performance networking applications. It supports asynchronous I/O operations, enabling the server to handle multiple connections simultaneously without waiting for each request to complete before starting the next one.

The `make_service_fn` and `service_fn` constructs are used to create a new service for each incoming connection. This means that each request is processed independently.

### Performance
Most requests are being dealth with within less then 2ms, with a throughput of 1000 requests per second on a single thread. 

### Dependencies:
```
hyper: For creating the HTTP server and client.
tokio: Asynchronous runtime for handling concurrent connections.
tracing and tracing-subscriber: For logging.
```



### Useful commands
```sh
cargo build
cargo run
```

```sh
docker build -t s3-proxy .
docker run -p 8090:8090 --rm -it \
  -e S3_URL=https://staticwebside.domain.aws.corp.com.s3.eu-west-1.amazonaws.com \
  s3-proxy

curl http://localhost:8090/index.html
```

### Clean up
```
docker ps 
docker kill <id>
```