Throwaway PKI for the gRPC TLS tests (the backend's `tests/grpc_tls.rs`, and the runtime's
`agent-runtime/tests/test_grpc_tls.py`). Test-only: these keys protect nothing.
The CA keys were discarded after signing. `server.pem` is for `localhost`,
`127.0.0.1` and `::1`; `client.pem` is a client certificate of the same CA;
`other-ca.pem` is a CA that signed neither. Valid for 100 years from 2026-10-25.
Regenerate with openssl (EC P-256, PKCS#8 keys) if they ever need to change.
