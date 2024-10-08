# S3 Proxy

The proxy is stateless, making it easy to scale horizontally.

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
  -e S3_URL=https://s3-bucket-as-loginpage.ds-fdn-d.aws.corp.com.s3.eu-west-1.amazonaws.com \
  s3-proxy

curl http://localhost:8090/index.html
```

### Clean up
```
docker ps 
docker kill <id>
```