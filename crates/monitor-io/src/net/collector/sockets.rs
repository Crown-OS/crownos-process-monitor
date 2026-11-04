use monitor_core::{Coverage, Page, SampleContext};

use monitor_core::util::cadence::interval_elapsed;

use super::{NetworkCollector, traffic_interval};
use crate::net::NetworkSnapshot;
use crate::net::sockets::{SocketNeeds, SocketOutputs};

impl NetworkCollector {
    pub(super) fn sample_sockets(
        &mut self,
        context: &SampleContext<'_>,
        output: &mut NetworkSnapshot,
    ) {
        let now = context.tick.monotonic();
        let on_page = context.interest.shows(Page::Network);
        let inspected = context.interest.inspected.map(|key| key.pid);
        let needs = SocketNeeds {
            traffic: context.fds.scope().sockets,
            listening: on_page || inspected.is_some(),
            connections: on_page || inspected.is_some(),
        };
        let seconds = self
            .last_traffic_pass
            .map_or(context.elapsed.seconds, |last| {
                now.saturating_sub(last).as_secs_f64()
            });
        let outcome = self.sockets.as_mut().map(|sockets| {
            sockets.sample(
                needs,
                context.fds,
                seconds,
                SocketOutputs {
                    listening: &mut output.listening,
                    connections: &mut output.connections,
                    traffic: &mut output.process_traffic,
                },
            )
        });
        let measured = matches!(outcome, Some(Ok(())));
        if !on_page {
            output
                .connections
                .retain(|connection| connection.owner.is_some() && connection.owner == inspected);
        }
        if needs.traffic && measured {
            self.last_traffic_pass = Some(now);
            output.traffic_passes += 1;
            self.carried_traffic.clone_from(&output.process_traffic);
        } else if interval_elapsed(
            self.last_traffic_pass,
            now,
            2 * traffic_interval(context.interest),
        ) {
            self.carried_traffic.clear();
        } else {
            output.process_traffic.clone_from(&self.carried_traffic);
        }
        output.process_traffic_coverage = if measured && self.last_traffic_pass.is_some() {
            Coverage::TcpOnly
        } else {
            Coverage::Unavailable
        };
    }
}
