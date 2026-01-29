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
- CLI/config semantics: unit tests for normalization/tilde expansion + manual CLI runs
- nodes.json compatibility: unit tests with Go-shaped JSON
- manager rules: unit tests + code parity review of constants/filters
- DNS semantics: unit tests for A/AAAA/NS + edge cases
- gRPC semantics: unit tests for GetPeersList + invalid subnetwork ID
- crawler/polling: code parity review + optional live-node integration
- operational behavior (logging/shutdown/Docker): code parity review + build commands

# Parity Results (2026-01-29)

Legend: PASS = parity confirmed via tests and/or code parity review; FAIL = mismatch or not yet verified.

| Area | Status | Evidence |
| --- | --- | --- |
| CLI defaults + normalization (listen/grpc/appdir/threads/loglevel) | PASS | Rust tests: `config::tests::*`, `logging::tests::level_from_string_accepts_shorthand` via `cargo test` |
| Config-file precedence + required flags | PASS | Code parity: `config.go` vs `dnsseeder-rs/src/config.rs` (merge_overrides + required host/nameserver checks) |
| `--profile` flag behavior | PASS | Rust test `profiling::tests::test_profile_endpoints` via `cargo test`; Go pprof in `dnsseed.go` |
| `nodes.json` format compatibility | PASS | Rust test `manager::tests::test_nodes_json_format_matches_go` via `cargo test` |
| nodes.json save cadence + shutdown flush | PASS | Code parity: `manager.go` vs `dnsseeder-rs/src/manager.rs` (60s prune, 120s dump, final save on shutdown) |
| Address manager rules (good/stale/expired, max 16, routability) | PASS | Code parity + `manager::tests::test_pruning_and_good` via `cargo test` |
| DNS A/AAAA/NS semantics (TTL/authority/AAAA empty workaround/hostname matching) | PASS | Rust tests: `dns::tests::test_dns_a_response`, `test_dns_authority_uses_base_hostname`, `test_dns_accepts_hostname_substring` via `cargo test` |
| gRPC PeerService/GetPeersList (shape + filtering) | PASS | Rust tests: `grpc::tests::test_get_peers`, `test_get_peers_invalid_subnetwork_id` via `cargo test` |
| Crawler/polling flow (handshake + address request) | PASS | Code parity review: `dnsseed.go` + `netadapter/*` vs `dnsseeder-rs/src/main.rs` + `netadapter.rs` (not run against live node) |
| Startup/shutdown sequencing | PASS | Code parity review: `dnsseed.go` vs `dnsseeder-rs/src/main.rs` |
| Docker build | PASS | `docker build -f docker/Dockerfile -t dnsseeder .` and `docker build -f docker/Dockerfile.rust -t dnsseeder-rs .` (Rust image uses `rust:1.89-bullseye` for edition 2024) |

# Known Deltas / FAIL Items

All parity items are PASS based on current checks.

# Punch-list to reach parity

None. Parity confirmed with the current evidence set.

# Evidence Commands

- Go baseline: `go test ./...`
- Rust parity tests: `cd dnsseeder-rs && cargo test`
- Docker builds: `docker build -f docker/Dockerfile -t dnsseeder .` and `docker build -f docker/Dockerfile.rust -t dnsseeder-rs .`

Optional side-by-side checks (recommended):
- Run Go and Rust DNS servers on different ports and compare `dig` A/AAAA/NS responses for the same hostname.
- Run Go and Rust gRPC servers on different ports and compare GetPeersList responses against the same in-memory fixture.
