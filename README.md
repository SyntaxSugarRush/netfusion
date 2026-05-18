# NetFusion

**Intelligent multi-interface network aggregation for Linux.**

NetFusion bonds, balances, and failsover heterogeneous network interfaces — Ethernet, Wi-Fi, USB tethering, LTE/5G, VPN tunnels, and more — through a terminal dashboard with real-time health monitoring, QoS, and predictive analytics.

<p align="center">
  <strong>TUI Dashboard</strong> · <strong>Active-Backup Bonding</strong> · <strong>ECMP Load Balancing</strong> · <strong>QoS</strong> · <strong>WireGuard Tunnels</strong> · <strong>Predictive Failover</strong>
</p>

---

## Features

### Core
- **Interface discovery** via rtnetlink + ethtool — enumerates all Linux network interfaces with speed, duplex, link state, and metadata
- **Health scoring** — weighted composite of RTT, jitter, packet loss, throughput, and stability with EMA smoothing and hysteresis
- **ICMP ping monitoring** — continuous health probes with configurable targets and intervals
- **Active-backup bonding** — kernel-level failover via Linux bonding with automatic member selection
- **Policy routing** — `ip rule`-based traffic steering with per-interface policies

### Advanced Routing
- **ECMP multipath** — proportional load balancing across healthy interfaces via `ip route nexthop`
- **Weighted balancing** — health-score-proportional weight distribution
- **nftables flow distribution** — jhash-based flow steering for fine-grained per-connection routing

### QoS
- **Bufferbloat mitigation** — fq_codel, CAKE, HTB queue disciplines via `tc`
- **DSCP tagging** — nftables-based traffic classification (RFC 2474 / RFC 4594)
- **ECN** — Explicit Congestion Notification support

### Intelligence
- **Dynamic path steering** — automatic traffic rerouting with hysteresis and cooldown protection
- **Predictive failover** — linear regression trend analysis detects degradation before failure
- **Anomaly detection** — z-score-based outlier identification
- **Adaptive heuristics** — auto-tuning health weights based on observed failure patterns
- **Advanced analytics** — percentile statistics, reliability ratings, performance reports

### Tunnels
- **WireGuard orchestration** — create, configure, monitor, and auto-reconnect tunnels
- **QUIC relay server** — optional VPS-deployed endpoint for true multi-ISP aggregation
- **Exponential backoff** — intelligent reconnect with cooldown protection

### Infrastructure
- **TUI dashboard** — ratatui/crossterm terminal interface with real-time metrics
- **Daemon architecture** — Unix domain socket IPC with bincode length-prefixed framing
- **SQLite state persistence** — WAL mode for crash recovery and health history
- **Config file watcher** — live reload via `notify` with validation
- **Config schema** — TOML-based configuration with validator derives

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  netfusion (TUI)                 │
│         ratatui dashboard + keyboard nav         │
└──────────────────────┬──────────────────────────┘
                       │ Unix socket (bincode IPC)
┌──────────────────────▼──────────────────────────┐
│                 netfusiond (daemon)              │
│  ┌────────────┐ ┌──────────┐ ┌───────────────┐  │
│  │  Scanner   │ │ Monitor  │ │  Bond Manager │  │
│  │ rtnetlink  │ │ ICMP ping│ │  ip link      │  │
│  └─────┬──────┘ └────┬─────┘ └───────┬───────┘  │
│  ┌─────▼──────────────▼───────────────▼───────┐  │
│  │          Health Scoring Engine             │  │
│  │   EMA smoothing · Hysteresis · Weights     │  │
│  └────────────────────┬───────────────────────┘  │
│  ┌────────────────────▼───────────────────────┐  │
│  │          Intelligence Layer                │  │
│  │  Predictive · Analytics · Adaptive Weights │  │
│  └────────────────────┬───────────────────────┘  │
│  ┌────────────────────▼───────────────────────┐  │
│  │          Path Steering + Routing           │  │
│  │   ECMP · nftables · Safe Apply · Rollback  │  │
│  └────────────────────────────────────────────┘  │
│  ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │
│  │   QoS    │ │ Tunnels  │ │  Config Watcher  │  │
│  │ fq_codel │ │ WireGuard│ │  live reload     │  │
│  └──────────┘ └──────────┘ └──────────────────┘  │
└──────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────┐
│           netfusion-relay (QUIC server)          │
│     Optional VPS endpoint for tunnel aggregation  │
└──────────────────────────────────────────────────┘
```

---

## Crates

| Crate | Description |
|---|---|
| `netfusion-shared` | Config schema, types, events, IPC protocol |
| `netfusion-core` | Discovery, health, monitoring, bonding, routing, QoS, tunnels, intelligence |
| `netfusion-daemon` | Core daemon with IPC server, config watcher, periodic rescan |
| `netfusion-tui` | Terminal dashboard with ratatui |
| `netfusion-relay` | QUIC relay server for remote tunnel aggregation |

---

## Prerequisites

- **Rust 1.85+** (edition 2024)
- **Linux** (uses netlink, rtnetlink, tc, ip, wg commands)
- **Root / CAP_NET_ADMIN** for interface manipulation
- **WireGuard kernel module** (`wireguard` or `wireguard-tools`)
- **nftables** for flow distribution and DSCP tagging
- **iputils** for ping probes
- **ethtool** for interface metadata

---

## Installation

### From source

```bash
# Clone the repository
git clone https://github.com/SyntaxSugarRush/netfusion.git
cd netfusion

# Build all binaries
cargo build --release

# Binaries are in target/release/
# - netfusion       (TUI frontend)
# - netfusiond      (core daemon)
# - netfusion-relay (QUIC relay server)
```

### System setup

```bash
# Create runtime directories
sudo mkdir -p /run/netfusion /var/lib/netfusion /etc/netfusion

# (Optional) Place your configuration
sudo mkdir -p /etc/netfusion
# sudo nano /etc/netfusion/netfusion.toml

# For relay server TLS certs (auto-generated on first run if missing)
sudo mkdir -p /etc/netfusion/relay
```

---

## Configuration

Create `/etc/netfusion/netfusion.toml`:

```toml
[daemon]
socket_path = "/run/netfusion/netfusion.sock"
state_path = "/var/lib/netfusion/state.db"
health_interval_ms = 1000
rollback_timeout_secs = 30
dry_run = false

[interfaces]
# Interface selectors for auto-discovery
# (empty = discover all physical interfaces)

[[bonds]]
name = "bond0"
mode = "active_backup"
# members discovered automatically

[policies]
# Policy-based routing rules

[qos]
enabled = true
qdisc = "fq_codel"
ecn = true
dscp_tagging = true

[logging]
level = "info"
```

---

## Running

### 1. Start the daemon

```bash
sudo ./target/release/netfusiond
```

The daemon will:
- Load configuration from `/etc/netfusion/netfusion.toml`
- Open the SQLite state store
- Scan for network interfaces via rtnetlink
- Start periodic 30-second rescans
- Start the IPC server on the Unix socket
- Watch for config file changes with live reload

### 2. Open the TUI

```bash
./target/release/netfusion
```

The dashboard shows:
- **System status** — uptime, active bonds, tunnel count
- **Interface list** — name, type, IP, speed, link state, health score
- **Bond status** — active members, standby, failover state
- **Event log** — recent interface events and health transitions

**Keyboard navigation:**
- `Tab` / `Shift+Tab` — switch tabs
- `↑` / `↓` — navigate within a tab
- `r` — refresh
- `q` — quit

### 3. (Optional) Start the relay server

```bash
./target/release/netfusion-relay
```

Generates self-signed TLS certs on first run. Listens on `0.0.0.0:4433` by default.

---

## Testing

```bash
# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p netfusion-core

# Run with output
cargo test -- --nocapture

# Run a specific test
cargo test test_compute_health_perfect
```

**54 unit tests** covering:
- Health scoring normalization and EMA smoothing
- Ping output parsing
- QoS qdisc configuration
- DSCP rule generation
- Path steering with hysteresis and cooldown
- Tunnel reconnection backoff
- Predictive trend analysis
- Adaptive weight computation
- Analytics percentile calculations
- SQLite state persistence

---

## Project Status

| Phase | Feature | Status |
|---|---|---|
| 1 | MVP: discovery, monitoring, bonding, TUI, config | ✅ Complete |
| 2 | Advanced routing: ECMP, weighted balancing, nftables | ✅ Complete |
| 3 | Performance: QoS, DSCP, ECN, path steering | ✅ Complete |
| 4 | Tunnel aggregation: WireGuard, QUIC relay | ✅ Complete |
| 5 | Intelligence: predictive failover, analytics, adaptive weights | ✅ Complete |

**12 commits · 54 tests · ~9,700 lines of Rust**

---

## Known Limitations

- **TUI performance** — the dashboard can feel laggy during rapid health updates. This is an early-stage issue related to ratatui render frequency and will be optimized.
- **NetworkManager coexistence** — stub implementation; full NM integration is pending.
- **WireGuard tunnel setup** — requires manual key generation; the daemon handles creation/teardown but not keypair generation.
- **No single-flow aggregation** — as with any multi-WAN setup, individual TCP/UDP flows are bound to a single path. Aggregate throughput only applies across multiple concurrent flows.

---

## License

Dual-licensed under either:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

## Repository

[github.com/SyntaxSugarRush/netfusion](https://github.com/SyntaxSugarRush/netfusion)
