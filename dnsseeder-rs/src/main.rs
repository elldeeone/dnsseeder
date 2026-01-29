mod checkversion;
mod config;
mod dns;
mod dnsseed;
mod grpc;
mod logging;
mod manager;
mod netadapter;
mod types;
mod version;

use crate::checkversion::check_version;
use crate::config::{log_paths, set_active_config, set_peers_default_port};
use crate::dns::DnsServer;
use crate::manager::Manager;
use crate::netadapter::DnsseedNetAdapter;
use crate::types::NetAddress;
use kaspa_p2p_lib::common::DEFAULT_TIMEOUT;
use kaspa_p2p_lib::pb::RequestAddressesMessage;
use kaspa_p2p_lib::pb::kaspad_message::Payload;
use kaspa_p2p_lib::{KaspadMessagePayloadType, make_message};
use log::{debug, error, info, warn};
use once_cell::sync::Lazy;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::watch;
use tokio::time::Duration;

static SYSTEM_SHUTDOWN: AtomicBool = AtomicBool::new(false);
static DEFAULT_SEEDER: Lazy<parking_lot::Mutex<Option<NetAddress>>> =
    Lazy::new(|| parking_lot::Mutex::new(None));

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let cfg = config::load_config()?;
    let (log_file, err_log_file) = log_paths(&cfg);
    logging::init(cfg.no_log_files, &cfg.log_level, &log_file, &err_log_file)?;
    info!("Version {}", version::version());

    if !cfg.profile.is_empty() {
        warn!("--profile is not supported in the Rust implementation");
    }

    set_active_config(cfg.clone())?;
    let default_port: u16 = cfg
        .network
        .active_net_params
        .default_port
        .parse()
        .map_err(|e| {
            format!(
                "Invalid peers default port {}: {}",
                cfg.network.active_net_params.default_port, e
            )
        })?;
    set_peers_default_port(default_port);

    let manager = Manager::new(&cfg.app_dir)?;
    let manager_handle = manager.start_background();

    if !cfg.known_peers.is_empty() {
        let mut peers = Vec::new();
        for raw in cfg.known_peers.split(',') {
            let parts: Vec<&str> = raw.split(':').collect();
            if parts.len() != 2 {
                return Err(format!(
                    "Invalid peer address: {}; addresses should be in format \"IP\":\"port\"",
                    raw
                ));
            }
            let ip: IpAddr = parts[0]
                .parse()
                .map_err(|_| format!("Invalid peer IP address: {}", parts[0]))?;
            let port: u16 = parts[1]
                .parse()
                .map_err(|_| format!("Invalid peer port: {}", parts[1]))?;
            peers.push(NetAddress::new(ip, port));
        }
        manager.add_addresses(&peers);
        for peer in peers {
            manager.attempt(&peer);
            manager.good(&peer, None, None);
        }
    }

    if !cfg.seeder.is_empty()
        && let Some(addr) = resolve_seeder(&cfg.seeder, default_port).await
    {
        manager.add_addresses(std::slice::from_ref(&addr));
        *DEFAULT_SEEDER.lock() = Some(addr);
    }

    let cfg_arc = Arc::new(cfg);
    let net_adapters: Vec<Arc<DnsseedNetAdapter>> = (0..cfg_arc.threads)
        .map(|_| Arc::new(DnsseedNetAdapter::new(cfg_arc.clone()).expect("netadapter init")))
        .collect();

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let creep_handle = {
        let manager = manager.clone();
        let cfg = cfg_arc.clone();
        let adapters = net_adapters.clone();
        tokio::spawn(async move {
            creep(manager, cfg, adapters, shutdown_rx).await;
        })
    };

    let dns_handle = {
        let server = Arc::new(DnsServer::new(
            &cfg_arc.host,
            &cfg_arc.nameserver,
            &cfg_arc.listen,
            manager.clone(),
        ));
        let rx = shutdown_tx.subscribe();
        tokio::spawn(async move { server.start(rx).await })
    };

    let grpc_server = grpc::start(manager.clone(), &cfg_arc.grpc_listen).await?;

    wait_for_shutdown_signal().await;

    info!("Gracefully shutting down the seeder...");
    SYSTEM_SHUTDOWN.store(true, Ordering::Relaxed);
    let _ = shutdown_tx.send(true);
    manager.shutdown().await;
    grpc_server.stop().await;
    let _ = creep_handle.await;
    let _ = dns_handle.await;
    let _ = manager_handle.await;
    info!("Seeder shutdown complete");
    Ok(())
}

async fn creep(
    manager: Arc<Manager>,
    cfg: Arc<config::Config>,
    adapters: Vec<Arc<DnsseedNetAdapter>>,
    shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }

        let mut peers = manager.addresses();
        if peers.is_empty() && manager.address_count() == 0 {
            dnsseed::seed_from_dns(&cfg, None, true, None, manager.clone()).await;
            peers = manager.addresses();
        }

        if peers.is_empty() {
            debug!("No stale addresses");
            for _ in 0..10 {
                if *shutdown.borrow() {
                    return;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            continue;
        }

        let mut handles = Vec::new();
        for (i, addr) in peers.into_iter().enumerate() {
            if *shutdown.borrow() {
                return;
            }
            let adapter = adapters[i % adapters.len()].clone();
            let manager = manager.clone();
            let cfg = cfg.clone();
            handles.push(tokio::spawn(async move {
                if let Err(err) = poll_peer(&adapter, &manager, &cfg, &addr).await {
                    debug!("{}", err);
                    if is_default_seeder(&addr) {
                        error!("failed to poll default seeder");
                        std::process::exit(1);
                    }
                }
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
    }
}

async fn poll_peer(
    adapter: &DnsseedNetAdapter,
    manager: &Manager,
    cfg: &config::Config,
    addr: &NetAddress,
) -> Result<(), String> {
    manager.attempt(addr);
    let peer_address = format!("{}:{}", addr.ip, addr.port);
    debug!("Polling peer {}", peer_address);
    let mut routes = adapter.connect(&peer_address).await?;
    let peer_version = routes.peer_version.clone();

    if cfg.min_proto_ver > 0 && peer_version.protocol_version < cfg.min_proto_ver as u32 {
        return Err(format!(
            "Peer {} ({}) protocol version {} is below minimum: {}",
            peer_address, peer_version.user_agent, peer_version.protocol_version, cfg.min_proto_ver
        ));
    }

    let request = make_message!(
        Payload::RequestAddresses,
        RequestAddressesMessage {
            include_all_subnetworks: true,
            subnetwork_id: None,
        }
    );
    routes.enqueue(request).await.map_err(|e| e.to_string())?;
    let message = routes
        .wait_for_message(KaspadMessagePayloadType::Addresses, DEFAULT_TIMEOUT)
        .await
        .map_err(|e| e.to_string())?;
    let addresses_msg = match message.payload {
        Some(Payload::Addresses(a)) => a,
        _ => return Err("failed to receive addresses".to_string()),
    };

    let mut addrs = Vec::new();
    for addr in addresses_msg.address_list {
        if let Some(net_addr) = proto_to_net_address(&addr) {
            addrs.push(net_addr);
        }
    }

    let added = manager.add_addresses(&addrs);
    info!(
        "Peer {} ({}) sent {} addresses, {} new",
        peer_address,
        peer_version.user_agent,
        addrs.len(),
        added
    );

    if !cfg.min_ua_ver.is_empty() {
        check_version(&cfg.min_ua_ver, &peer_version.user_agent).map_err(|_| {
            format!(
                "Peer {} version {} doesn't satisfy minimum: {}",
                peer_address, peer_version.user_agent, cfg.min_ua_ver
            )
        })?;
    }
    manager.good(addr, Some(peer_version.user_agent), None);
    routes.disconnect().await;
    Ok(())
}

fn proto_to_net_address(addr: &kaspa_p2p_lib::pb::NetAddress) -> Option<NetAddress> {
    if addr.port > u16::MAX as u32 {
        return None;
    }
    let ip = match addr.ip.len() {
        4 => IpAddr::V4(std::net::Ipv4Addr::new(
            addr.ip[0], addr.ip[1], addr.ip[2], addr.ip[3],
        )),
        16 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&addr.ip);
            IpAddr::V6(std::net::Ipv6Addr::from(octets))
        }
        _ => return None,
    };
    Some(NetAddress::with_timestamp(
        ip,
        addr.port as u16,
        addr.timestamp,
    ))
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("sigterm handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = sigterm.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn resolve_seeder(seeder: &str, default_port: u16) -> Option<NetAddress> {
    let mut host = seeder.to_string();
    let mut port = default_port;
    if seeder.matches(':').count() == 1
        && let Some((h, p)) = seeder.rsplit_once(':')
        && let Ok(parsed_port) = p.parse::<u16>()
    {
        host = h.to_string();
        port = parsed_port;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(NetAddress::new(ip, port));
    }

    match lookup_host(&host).await {
        Ok(ip) => Some(NetAddress::new(ip, port)),
        Err(err) => {
            warn!("Failed to resolve seed host: {}, {}, ignoring", host, err);
            None
        }
    }
}

async fn lookup_host(host: &str) -> Result<IpAddr, String> {
    let host = host.to_string();
    tokio::task::spawn_blocking(move || {
        let addr = format!("{}:0", host);
        let mut addrs = addr.to_socket_addrs().map_err(|e| e.to_string())?;
        addrs
            .next()
            .map(|a| a.ip())
            .ok_or_else(|| "no addresses".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn is_default_seeder(addr: &NetAddress) -> bool {
    if let Some(default) = DEFAULT_SEEDER.lock().as_ref() {
        default.ip == addr.ip && default.port == addr.port
    } else {
        false
    }
}
