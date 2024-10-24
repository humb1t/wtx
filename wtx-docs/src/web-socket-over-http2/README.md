# WebSocket over HTTP/2


At the current time only servers support the handshake procedure defined in [RFC6455](https://https://datatracker.ietf.org/doc/html/rfc8441).

While HTTP/2 inherently supports full-duplex communication, web browsers typically don't expose this functionality directly to developers and that is why WebSocket tunneling over HTTP/2 is important.

1. Servers can efficiently handle multiple concurrent streams within a single TCP connection
2. Client applications can continue using existing WebSocket APIs without modification

To use this functionality, it necessary to activate the `http2` and `web-socket` features.

## Example

```rust,edition2021,no_run
{{#rustdoc_include ../../../wtx-instances/http3-examples/web-socket-server.rs}}
```