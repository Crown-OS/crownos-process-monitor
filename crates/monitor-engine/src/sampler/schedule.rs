use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::Sampler;
use crate::Command;
use crate::command::Message;

/// A page switch samples at once, but never sooner than this after the
/// previous tick: shorter intervals make rates noisy.
const MIN_SPACING: Duration = Duration::from_millis(250);

/// Samples on absolute deadlines and sleeps in `recv_timeout` in between,
/// so the thread wakes only for a tick or a message.
pub(crate) fn run(mut sampler: Sampler, messages: &Receiver<Message>) {
    let mut deadline = Instant::now();
    loop {
        let wait = deadline.saturating_duration_since(Instant::now());
        match messages.recv_timeout(wait) {
            Err(RecvTimeoutError::Timeout) => {
                sampler.sample();
                deadline = next_deadline(deadline, sampler.interval(), Instant::now());
            }
            Ok(Message::Interest(interest)) => {
                if sampler.set_interest(interest) {
                    deadline = deadline.min(soonest(&sampler, MIN_SPACING));
                }
            }
            Ok(Message::Command(Command::Shutdown)) | Err(RecvTimeoutError::Disconnected) => {
                break;
            }
            Ok(Message::Inspect { key, reply }) => {
                let _ = reply.send(sampler.inspect(key));
            }
            Ok(Message::Command(command)) => {
                sampler.apply(command);
                deadline = deadline.min(soonest(&sampler, sampler.interval()));
            }
        }
    }
    sampler.finish();
}

fn soonest(sampler: &Sampler, after: Duration) -> Instant {
    sampler
        .last_sample
        .map_or_else(Instant::now, |last| last + after)
}

/// The deadline after `previous`, or a full interval from now when the
/// sampler fell behind (a suspend, a slow tick).
fn next_deadline(previous: Instant, interval: Duration, now: Instant) -> Instant {
    let next = previous + interval;
    if next > now { next } else { now + interval }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_an_absolute_cadence_unless_behind() {
        let start = Instant::now();
        let second = Duration::from_secs(1);
        let on_time = start + Duration::from_millis(20);
        assert_eq!(next_deadline(start, second, on_time), start + second);
        let late = start + Duration::from_secs(3);
        assert_eq!(next_deadline(start, second, late), late + second);
    }
}
