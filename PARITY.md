# Parity Plan

Rust crate location: `dnsseeder-rs/`

Build/run:
- Go: `go build -o dnsseeder .` and `go test ./...`
- Rust: `cd dnsseeder-rs && cargo build` and `cargo test`

Key entrypoints (Rust):
- `dnsseeder-rs/src/main.rs` (startup, crawl loop, shutdown)
- `dnsseeder-rs/src/config.rs` (CLI + config parsing)
- `dnsseeder-rs/src/dns.rs` (DNS request/response handling)
- `dnsseeder-rs/src/manager.rs` (node state, pruning, JSON persistence)
- `dnsseeder-rs/src/grpc.rs` (PeerService/GetPeersList)
- `dnsseeder-rs/src/netadapter.rs` (connect/handshake/request-addresses)
- `dnsseeder-rs/src/checkversion.rs` (min user agent version gating)
- `dnsseeder-rs/src/logging.rs` (log setup)

Verification matrix (plan):
- CLI/config semantics: code parity review + Rust unit tests for normalization/tilde expansion
- nodes.json compatibility: Rust unit test matching Go JSON shape
- manager rules: Rust unit tests + code parity review of constants/filters
- DNS semantics: Rust unit tests for A/AAAA/NS + edge cases
- gRPC semantics: Rust unit tests for GetPeersList + invalid subnetwork ID
- crawler/polling: code parity review (handshake flow, address request)
- operational behavior (logging/shutdown/Docker): code parity review + build commands

# Parity Results (2026-01-29)

Legend: PASS = parity confirmed via tests and/or code parity review; FAIL = mismatch or not verified.

| Area | Status | Evidence |
| --- | --- | --- |
| CLI defaults + config precedence + required flags | PASS | Code parity review: `config.go` vs `dnsseeder-rs/src/config.rs`; Rust tests `config::tests::*` via `cargo test` |
| `nodes.json` format compatibility | PASS | Rust test `manager::tests::test_nodes_json_format_matches_go` via `cargo test` |
| nodes.json save cadence + shutdown flush | PASS | Code parity review: `manager.go` vs `dnsseeder-rs/src/manager.rs` (60s prune, 120s dump, final save) |
| Address manager rules (good/stale/expired, max 16, routability) | PASS | Code parity review + Rust test `manager::tests::test_pruning_and_good` via `cargo test` |
| DNS A/AAAA/NS semantics (TTL/authority/AAAA empty workaround/hostname matching) | PASS | Rust tests `dns::tests::test_dns_a_response`, `test_dns_authority_uses_base_hostname`, `test_dns_accepts_hostname_substring` via `cargo test`; side-by-side `dig` comparison (Go vs Rust) matched after stripping comment lines |
| gRPC PeerService/GetPeersList (shape + filtering + IPv4 bytes) | PASS | Rust tests `grpc::tests::test_get_peers` (IPv4-mapped bytes), `test_get_peers_invalid_subnetwork_id` via `cargo test`; side-by-side gRPC comparison matched (timestamps normalized) |
| Crawler/polling flow (handshake + address request) | PASS | Code parity review: `dnsseed.go` + `netadapter/*` vs `dnsseeder-rs/src/main.rs` + `netadapter.rs` |
| Startup/shutdown sequencing | PASS | Code parity review: `dnsseed.go` vs `dnsseeder-rs/src/main.rs` |
| Logging behavior (stdout + optional log files) | PASS | Code parity review: `log.go` vs `dnsseeder-rs/src/logging.rs` |
| Docker builds | PASS | `docker build -f docker/Dockerfile -t dnsseeder .` and `docker build -f docker/Dockerfile.rust -t dnsseeder-rs .` |

# Known Deltas / FAIL Items

None.

# Evidence Commands

- Go baseline: `go test ./...`
- Rust parity tests: `cd dnsseeder-rs && cargo test`
- Docker builds: `docker build -f docker/Dockerfile -t dnsseeder .` and `docker build -f docker/Dockerfile.rust -t dnsseeder-rs .`

Optional side-by-side checks (recommended):
- Run Go and Rust DNS servers on different ports and compare `dig` A/AAAA/NS responses for the same hostname.
- Run Go and Rust gRPC servers on different ports and compare GetPeersList responses against the same in-memory fixture.

Side-by-side evidence (executed):
- DNS: `dig @127.0.0.1 -p 15354 seed.example.com A|AAAA|NS +time=1 +tries=1 +noall +answer +authority | grep -v '^;' | sort` compared to the same queries against port `25354` (outputs matched).
- gRPC: `go run /tmp/grpc_peers.go 127.0.0.1:13737` compared to `127.0.0.1:23737` with timestamps normalized (`sed -E 's/ ts=[0-9]+/ ts=<ts>/'`) and sorted (outputs matched).
