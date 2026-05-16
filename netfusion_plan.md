# NetFusion — Comprehensive Engineering Plan

## Overview

NetFusion is a Linux-native terminal user interface (TUI) application designed for advanced multi-interface network aggregation, bonding, routing optimization, failover, and WAN orchestration.

The project aims to intelligently combine heterogeneous network interfaces:

- Ethernet
- Wi-Fi
- USB tethering
- LTE/5G modems
- VPN tunnels
- VLANs
- WireGuard/OpenVPN/Tailscale
- Virtual interfaces
- Any Linux-supported network transport

Primary goals:

- Increase aggregate throughput where technically possible
- Improve reliability and failover
- Reduce jitter and packet loss
- Improve latency consistency
- Optimize application-aware routing
- Provide intelligent path steering
- Enable advanced WAN aggregation through remote relays

NetFusion must behave like a production-grade systems utility suitable for:

- Power users
- Streamers
- Gamers
- Remote workers
- Homelabs
- Mobile professionals
- Linux networking enthusiasts
- Edge networking experimentation

---

# 1. Technical Reality and Networking Constraints

NetFusion must correctly communicate the realities of WAN aggregation.

## Important Constraints

True single-flow bandwidth aggregation across unrelated ISPs generally requires:

- A coordinated remote endpoint
- MPTCP-capable peers
- QUIC multiplexing
- VPN/tunnel aggregation
- Session-aware traffic distribution

Without a remote aggregation endpoint:

- Multi-WAN can still improve total concurrent throughput
- Failover and resiliency improve dramatically
- Per-flow balancing works well
- Single TCP streams remain limited by one path

The software must never mislead users with fake “speed boost” claims.

---

# 2. Core Design Philosophy

NetFusion should prioritize:

1. Reliability
2. Safety
3. Linux-native correctness
4. Real-world practicality
5. Observability
6. Graceful degradation
7. Performance
8. Maintainability
9. Extensibility
10. User education

The application should feel like a hybrid of:

- OpenMPTCProuter
- Speedify
- mwan3
- Linux bonding
- NetworkManager
- modern TUI tooling

while remaining fully terminal-centric.

---

# 3. Recommended Tech Stack

## Preferred Language

### Primary Recommendation: Rust

Reasons:

- Memory safety
- High concurrency
- Excellent async ecosystem
- Strong Linux systems integration
- Performance
- Reliability
- Low runtime overhead

## Acceptable Alternative

### Go

Reasons:

- Fast iteration
- Excellent networking primitives
- Strong TUI ecosystem
- Simpler concurrency model

## Avoid Unless Necessary

### Python

Only acceptable for:

- orchestration
- plugin scripting
- helper tooling

Not suitable for the performance-critical networking core.

---

# 4. Recommended Rust Ecosystem

## UI

- ratatui
- crossterm
- tui-logger

## Async Runtime

- tokio

## Networking

- rtnetlink
- pnet
- surge-ping
- socket2
- quinn (QUIC)
- boringtun or wireguard-rs concepts

## System Integration

- zbus
- dbus-rs
- nix
- procfs
- sysinfo

## Serialization

- serde
- toml
- serde_yaml

## Metrics

- prometheus
- metrics-rs

## Logging

- tracing
- tracing-subscriber

---

# 5. Supported Platforms

Primary:

- Arch Linux
- Debian
- Ubuntu
- Fedora
- OpenSUSE

Secondary:

- NixOS

System assumptions:

- Linux kernel >= 6.x preferred
- systemd preferred
- nftables preferred over iptables
- MPTCP-enabled kernels supported

---

# 6. High-Level Architecture

## Architectural Style

- Event-driven
- Modular
- Async-first
- Plugin-capable
- Service-oriented internal architecture

## Major Components

### Core Daemon

Responsible for:

- route management
- interface orchestration
- monitoring
- health scoring
- failover
- tunnel management
- traffic policy

### TUI Frontend

Responsible for:

- visualization
- user interaction
- dashboards
- configuration
- live monitoring

### Relay Server

Optional remote aggregation endpoint.

### Metrics Subsystem

Responsible for:

- telemetry
- health scoring
- statistics
- benchmarking

### Policy Engine

Responsible for:

- routing decisions
- balancing logic
- adaptive behavior

---

# 7. Proposed Directory Structure

```text
netfusion/
├── Cargo.toml
├── README.md
├── docs/
├── configs/
├── packaging/
├── scripts/
├── systemd/
├── examples/
├── tests/
├── benches/
├── relay/
├── tui/
├── daemon/
│   ├── discovery/
│   ├── routing/
│   ├── bonding/
│   ├── monitoring/
│   ├── tunnels/
│   ├── qos/
│   ├── metrics/
│   ├── profiles/
│   ├── policies/
│   ├── security/
│   ├── integrations/
│   └── recovery/
└── shared/
```

---

# 8. Interface Discovery Engine

## Requirements

Automatically discover:

- Ethernet interfaces
- Wi-Fi adapters
- VLANs
- bridges
- tunnels
- VPN interfaces
- WireGuard
- Tailscale
- Docker/Podman interfaces
- PPP interfaces
- LTE/5G modems
- USB tethering

## Data Collection

Collect:

- MAC address
- driver info
- negotiated speed
- duplex
- MTU
- DNS
- gateway
- packet stats
- errors
- drops
- RSSI
- signal quality
- noise floor
- current IPs
- DHCP/static state
- queue stats
- latency metrics

## Technologies

Use:

- rtnetlink
- ethtool
- iw
- DBus APIs
- NetworkManager APIs
- systemd-networkd APIs

---

# 9. Bonding and Aggregation Modes

## Linux Bonding Support

Support:

- balance-rr
- active-backup
- balance-xor
- broadcast
- 802.3ad
- balance-tlb
- balance-alb

## Advanced Aggregation

Implement:

- MPTCP orchestration
- ECMP
- weighted balancing
- per-flow balancing
- per-packet experimentation
- user-space tunneling
- VPN aggregation
- intelligent WAN aggregation

## Profiles

### Gaming Mode

Prioritize:

- lowest stable latency
- low jitter
- fast failover

### Streaming Mode

Prioritize:

- upload stability
- jitter reduction
- packet loss mitigation

### Bulk Transfer Mode

Prioritize:

- aggregate throughput
- multi-flow utilization

### VoIP Mode

Prioritize:

- packet ordering
- low jitter
- low packet loss

---

# 10. Intelligent Routing Engine

## Technologies

Implement:

- ip rule
- ip route
- nftables marks
- tc/qdisc
- fq_codel
- CAKE
- ECN
- policy routing
- multi-table routing

## Features

- adaptive route selection
- fast failover
- sticky sessions
- flow pinning
- congestion-aware steering
- bandwidth-aware routing
- latency-aware routing
- interface weighting

## Dynamic Benchmarking

Continuously benchmark:

- RTT
- packet loss
- DNS latency
- throughput
- jitter
- congestion
- queue depth

---

# 11. Latency Optimization Subsystem

## Monitoring

Perform:

- ICMP latency tests
- TCP handshake timing
- DNS timing
- packet loss analysis
- jitter calculations
- bufferbloat testing

## Adaptive Actions

- deprioritize degraded links
- reroute sensitive traffic
- isolate unstable Wi-Fi
- reduce congestion impact
- avoid saturated paths

---

# 12. VPN and Tunnel Aggregation

## Supported Technologies

- WireGuard
- OpenVPN
- Tailscale integration
- QUIC tunnels
- SOCKS aggregation concepts

## Tunnel Capabilities

- multi-WAN VPN bonding
- encrypted aggregation
- split tunneling
- full tunnel mode
- intelligent path scheduling
- multiplexed transport

## Optional Relay Architecture

```text
Client Agent
    ↓
Remote Relay VPS
    ↓
Internet
```

The relay server enables:

- true multi-ISP aggregation
- flow reassembly
- packet ordering
- centralized egress

---

# 13. Remote Relay Server

## Requirements

Create companion daemon for VPS deployment.

## Features

- QUIC transport
- MPTCP experimentation
- encrypted tunnels
- packet reordering
- NAT traversal
- low-overhead framing
- UDP-first architecture
- automatic reconnection

## Recommended Stack

Rust + QUIC preferred.

---

# 14. Monitoring Dashboard

## TUI Design Goals

The interface should feel similar to:

- btop
- lazygit
- k9s
- glances
- nmtui

## Views

### Dashboard

Show:

- aggregate throughput
- active interfaces
- latency heatmaps
- health scores
- failover state

### Interface Explorer

Per-interface metrics.

### Bond Manager

Manage and visualize aggregation.

### Tunnel Manager

Manage relay and VPN tunnels.

### Routing Explorer

Visualize routing tables and policies.

### Logs and Events

Structured event viewer.

## Visualization Features

- sparklines
- rolling graphs
- heatmaps
- status indicators
- live tables
- event timelines

---

# 15. Automation and Policy Engine

## Features

- auto-bonding
- scheduled profiles
- network-triggered automation
- application-aware policies
- geo-aware routing
- automatic failover

## Examples

- Cellular only activates during packet loss spikes
- Ethernet preferred when available
- Streaming profile activates when OBS launches
- Gaming profile activates when Steam/Valorant launches

---

# 16. Configuration System

## Format

Preferred:

- TOML

Acceptable:

- YAML

## Requirements

- live reload
- profile import/export
- backup/restore
- CLI automation
- headless operation

## Example

```toml
[profile.gaming]
mode = "low_latency"
interfaces = ["eth0", "wlan0"]
prefer_lowest_rtt = true
max_jitter_ms = 15
```

---

# 17. Safety and Recovery

## Critical Requirement

The application must NEVER permanently break networking.

## Required Safeguards

- rollback timers
- dry-run mode
- emergency restore
- recovery shell
- route validation
- gateway conflict detection
- loop detection
- watchdog monitoring

## Safe Apply Flow

1. Validate config
2. Simulate changes
3. Apply incrementally
4. Verify connectivity
5. Commit changes
6. Roll back automatically on failure

---

# 18. Security

## Principles

- least privilege
- hardened subprocesses
- avoid shell injection
- secure secret handling
- encrypted key storage

## Integration

- Polkit support
- privilege separation
- capability-based permissions

---

# 19. Linux Integration

## Supported Stacks

- NetworkManager
- systemd-networkd
- netplan
- ifupdown
- firewalld
- nftables

## Requirements

Automatically detect active networking stack.

Avoid destructive interference.

---

# 20. Advanced Features

## QoS and Bufferbloat

Implement:

- fq_codel
- CAKE
- ECN
- DSCP tagging
- traffic prioritization

## MTU Optimization

- PMTU discovery
- adaptive MSS clamping
- blackhole detection

## Experimental

- AI-assisted optimization
- predictive failover
- congestion forecasting
- eBPF acceleration

---

# 21. Core Internal Abstractions

## Interface Object

Represents:

- link state
- health
- metrics
- capabilities
- policies

## Bond Group

Represents:

- grouped interfaces
- balancing strategy
- routing policies
- failover rules

## Health Score

Weighted composite metric including:

- RTT
- jitter
- packet loss
- throughput
- stability

## Route Policy

Defines:

- matching rules
- traffic class
- interface selection

---

# 22. Event System

Use event-driven architecture.

## Events

- interface_up
- interface_down
- packet_loss_spike
- congestion_detected
- failover_triggered
- tunnel_connected
- route_changed

All major components subscribe to events.

---

# 23. Plugin System

Optional but desirable.

## Potential Plugins

- custom health evaluators
- external tunnel providers
- custom routing logic
- Prometheus exporters
- telemetry sinks

---

# 24. Testing Strategy

## Unit Tests

- routing logic
- policy engine
- metrics calculations

## Integration Tests

Use:

- network namespaces
- virtual interfaces
- simulated WANs

## Chaos Testing

Simulate:

- packet loss
- ISP failures
- latency spikes
- MTU blackholes
- Wi-Fi roaming
- DNS outages

## Benchmarking

Measure:

- throughput
- failover times
- CPU overhead
- memory usage
- packet ordering

---

# 25. Packaging and Distribution

## Deliverables

- AUR package
- DEB packages
- RPM packages
- static binaries where feasible

## Systemd Integration

Provide:

- daemon service
- relay service
- health watchdog service

---

# 26. Documentation Requirements

## Must Include

- README
- architecture docs
- API docs
- user guide
- troubleshooting guide
- relay deployment guide
- networking theory guide

## Educational Content

Explain:

- limitations of WAN aggregation
- how MPTCP works
- why relays are needed
- tradeoffs of each mode

---

# 27. Recommended Development Roadmap

## Phase 1 — MVP

Deliver:

- interface discovery
- basic monitoring
- active-backup bonding
- TUI dashboard
- safe configuration system
- routing inspection

## Phase 2 — Advanced Routing

Add:

- policy routing
- ECMP
- weighted balancing
- failover automation

## Phase 3 — Performance Optimization

Add:

- latency scoring
- dynamic path steering
- QoS
- bufferbloat mitigation

## Phase 4 — Tunnel Aggregation

Add:

- WireGuard orchestration
- relay server
- QUIC transport
- multi-WAN aggregation

## Phase 5 — Intelligence Layer

Add:

- predictive failover
- AI-assisted optimization
- advanced analytics

---

# 28. MVP Requirements

The first usable release should:

- safely detect interfaces
- show real-time metrics
- create reliable failover bonds
- support policy routing
- provide rollback protection
- operate entirely from TUI

Do NOT attempt full QUIC aggregation first.

Stability and observability are more important.

---

# 29. Recommended Initial Focus

The first engineering focus should prioritize:

1. Safe networking changes
2. Monitoring subsystem
3. Interface abstraction layer
4. Routing engine
5. Policy management
6. TUI usability
7. Recovery mechanisms

before attempting advanced aggregation.

---

# 30. Long-Term Vision

NetFusion should evolve into:

- a professional Linux WAN orchestration suite
- a research platform for multi-path networking
- a terminal-native alternative to commercial bonding software
- an extensible edge-networking framework

while remaining:

- transparent
- Linux-native
- performant
- debuggable
- technically honest

---

# 31. Final Expectations for the Coding Agent

The coding agent must produce:

1. Real implementation-ready code
2. Modular architecture
3. Linux-native integrations
4. Strong error handling
5. Comprehensive observability
6. Extensive testing
7. Production-grade safety mechanisms
8. Maintainable code structure
9. Clear documentation
10. Incremental milestone delivery

The system should never sacrifice correctness for flashy features.

