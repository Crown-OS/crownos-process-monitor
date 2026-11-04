//! NetworkManager and resolved on a background thread. The sampler asks for
//! a refresh (only while someone looks) and reads the latest result.

use std::sync::Arc;

use tokio::sync::mpsc;
use zbus::Connection;

use super::dns::{self, DnsServers, DnsSource};
use super::wifi::{self, WifiInfo};
use monitor_core::util::latest::Latest;
use monitor_core::util::runtime_thread::spawn_runtime_thread;

const THREAD_NAME: &str = "monitor-network";
const RESOLV_CONF: &str = "/etc/resolv.conf";
const PROC_NET_WIRELESS: &str = "/proc/net/wireless";

#[derive(Debug, Default)]
pub(crate) struct ServiceState {
    pub wifi: Arc<[WifiInfo]>,
    pub dns: DnsServers,
}

#[derive(Debug)]
pub(crate) struct NetworkServices {
    requests: mpsc::UnboundedSender<()>,
    latest: Arc<Latest<ServiceState>>,
}

impl NetworkServices {
    pub(crate) fn spawn() -> Self {
        let latest = Arc::new(Latest::default());
        let (requests, receiver) = mpsc::unbounded_channel();
        let worker_latest = Arc::clone(&latest);
        let _ = spawn_runtime_thread(THREAD_NAME, move || serve(receiver, worker_latest));
        Self { requests, latest }
    }

    pub(crate) fn request_refresh(&self) {
        let _ = self.requests.send(());
    }

    pub(crate) fn latest(&self) -> Option<Arc<ServiceState>> {
        self.latest.try_get()
    }
}

async fn serve(mut requests: mpsc::UnboundedReceiver<()>, latest: Arc<Latest<ServiceState>>) {
    let connection = Connection::system().await.ok();
    while requests.recv().await.is_some() {
        while requests.try_recv().is_ok() {}
        latest.publish(gather(connection.as_ref()).await);
    }
}

async fn gather(connection: Option<&Connection>) -> ServiceState {
    let wireless = std::fs::read_to_string(PROC_NET_WIRELESS).unwrap_or_default();
    let (links, resolved) = match connection {
        Some(connection) => (
            wifi::from_network_manager(connection).await,
            dns::from_resolved(connection).await,
        ),
        None => (None, None),
    };
    let dns = match resolved {
        Some(servers) => DnsServers {
            servers: Arc::from(servers),
            source: DnsSource::Resolved,
        },
        None => std::fs::read_to_string(RESOLV_CONF).map_or_else(
            |_| DnsServers::default(),
            |text| DnsServers {
                servers: Arc::from(dns::parse_resolv_conf(&text)),
                source: DnsSource::ResolvConf,
            },
        ),
    };
    ServiceState {
        wifi: Arc::from(wifi::merge_kernel_signal(links, &wireless)),
        dns,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::net::wifi::WifiSource;

    #[test]
    #[ignore = "needs the system bus, NetworkManager and an associated Wi-Fi link"]
    fn reads_live_wifi_and_dns() {
        let services = NetworkServices::spawn();
        services.request_refresh();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if let Some(state) = services
                .latest()
                .filter(|state| state.dns.source != DnsSource::Unknown)
            {
                for link in state.wifi.iter() {
                    println!(
                        "{} ssid_found={} source={:?} strength={:?} dbm={:?} band={:?} channel={:?} rate={:?} security={:?}",
                        link.interface,
                        link.ssid.is_some(),
                        link.source,
                        link.strength,
                        link.signal_dbm,
                        link.band,
                        link.channel,
                        link.link_rate_mbps,
                        link.security
                    );
                }
                println!("dns {:?} from {:?}", state.dns.servers, state.dns.source);
                assert!(
                    state.wifi.iter().any(
                        |link| link.source == WifiSource::NetworkManager && link.ssid.is_some()
                    )
                );
                assert!(!state.dns.servers.is_empty());
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("network services did not answer within 10 s");
    }
}
