use crownui::prelude::{Styled, View, bind, vstack};
use monitor_engine::io::{DnsSource, NetworkSnapshot};

use crate::app::AppState;
use crate::components::fact_row::fact_row;
use crate::util::format::rate;
use crate::util::spacing;

const fn dns_source(source: DnsSource) -> &'static str {
    match source {
        DnsSource::Resolved => "systemd-resolved",
        DnsSource::ResolvConf => "/etc/resolv.conf",
        DnsSource::Unknown => "",
    }
}

pub fn gateways(network: &NetworkSnapshot) -> String {
    if network.default_routes.is_empty() {
        return "No default route".to_owned();
    }
    network
        .default_routes
        .iter()
        .map(|route| match &route.interface {
            Some(interface) => format!("{} via {interface}", route.gateway),
            None => route.gateway.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn dns_servers(network: &NetworkSnapshot) -> String {
    let dns = &network.dns;
    if dns.servers.is_empty() {
        return "None configured".to_owned();
    }
    let servers = dns
        .servers
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    match dns_source(dns.source) {
        "" => servers,
        source => format!("{servers} (from {source})"),
    }
}

/// Default gateways, DNS servers and the VPN/tunnel traffic kept out of the totals.
pub fn routing_panel(state: AppState) -> impl View {
    let (live, settings) = (state.live, state.settings);
    vstack((
        fact_row(
            "Default route",
            bind(move |runtime| live.with(runtime, |snapshot| gateways(&snapshot.tick.network))),
        ),
        fact_row(
            "DNS",
            bind(move |runtime| live.with(runtime, |snapshot| dns_servers(&snapshot.tick.network))),
        ),
        fact_row(
            "VPN and tunnels",
            bind(move |runtime| {
                let unit = settings.get(runtime).rates;
                live.with(runtime, |snapshot| {
                    let tunnels = &snapshot.tick.network.vpn_totals;
                    format!(
                        "{} down · {} up, not added to the totals",
                        rate(tunnels.receive.get(), unit),
                        rate(tunnels.transmit.get(), unit)
                    )
                })
            }),
        ),
    ))
    .gap(spacing::SNUG)
    .w_full()
}
