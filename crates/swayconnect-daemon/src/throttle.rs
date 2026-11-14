//! Connection bucket per address, slots before `welcome`, the rate classes of a device
//! (`docs/PROTOCOL.md` 17.14), and the budget of the log lines that a peer can cause.
//! Written by slice S6a.
//!
//! Nothing here counts failures: there is no lockout (`docs/PROTOCOL.md` section 14 note 4).

use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use swayconnect_protocol::limits::{
    ACTION_BURST, ACTION_REFILL_PER_SECOND, CONNECTION_BURST, CONNECTION_REFILL_INTERVAL_MS,
    CONTINUOUS_BURST, CONTINUOUS_REFILL_PER_SECOND, ICON_BURST, ICON_REFILL_PER_SECOND,
    INPUT_BURST, INPUT_REFILL_PER_SECOND, INPUT_TEXT_BURST, INPUT_TEXT_REFILL_PER_SECOND,
    INPUT_TEXT_SCALARS_PER_MINUTE, LAUNCH_BURST, LAUNCH_REFILL_INTERVAL_MS, MAX_CONNECTIONS,
    MAX_CONNECTIONS_BEFORE_WELCOME,
};
use tokio::sync::watch;
use tokio::time::Instant;

const MAX_TRACKED_ADDRESSES: usize = 1024;
const IPV6_PREFIX_MASK: u128 = !0 << 64;
const LOG_INTERVAL: Duration = Duration::from_secs(10);
const TEXT_BUDGET_WINDOW: Duration = Duration::from_secs(60);

/// A token bucket: `burst` tokens, one more every `refill_interval`.
#[derive(Debug, Clone, Copy)]
pub struct Rate {
    burst: u32,
    refill_interval: Duration,
}

impl Rate {
    pub const CONNECTIONS: Rate = Rate {
        burst: CONNECTION_BURST,
        refill_interval: Duration::from_millis(CONNECTION_REFILL_INTERVAL_MS),
    };
    pub const ACTIONS: Rate = Rate {
        burst: ACTION_BURST,
        refill_interval: Duration::from_millis(1000 / ACTION_REFILL_PER_SECOND as u64),
    };
    pub const ICONS: Rate = Rate {
        burst: ICON_BURST,
        refill_interval: Duration::from_millis(1000 / ICON_REFILL_PER_SECOND as u64),
    };
    pub const CONTINUOUS: Rate = Rate {
        burst: CONTINUOUS_BURST,
        refill_interval: Duration::from_millis(1000 / CONTINUOUS_REFILL_PER_SECOND as u64),
    };
    pub const LAUNCH: Rate = Rate {
        burst: LAUNCH_BURST,
        refill_interval: Duration::from_millis(LAUNCH_REFILL_INTERVAL_MS),
    };
    pub const INPUT: Rate = Rate {
        burst: INPUT_BURST,
        refill_interval: Duration::from_millis(1000 / INPUT_REFILL_PER_SECOND as u64),
    };
    pub const INPUT_TEXT: Rate = Rate {
        burst: INPUT_TEXT_BURST,
        refill_interval: Duration::from_millis(1000 / INPUT_TEXT_REFILL_PER_SECOND as u64),
    };

    /// Takes one token. The whole bucket state is `full_again_at`: the instant at which the
    /// bucket is full again. `Err` holds the delay until one token is available.
    pub fn take(&self, full_again_at: &mut Instant, now: Instant) -> Result<(), Duration> {
        let refill_start = (*full_again_at).max(now);
        let missing = refill_start - now;
        let missing_when_empty = self.refill_interval * self.burst;
        match (missing + self.refill_interval).checked_sub(missing_when_empty) {
            Some(wait) if !wait.is_zero() => Err(wait),
            _ => {
                *full_again_at = refill_start + self.refill_interval;
                Ok(())
            }
        }
    }
}

/// A rate class of spec 17.14: each device has one bucket per class, shared by its
/// connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateClass {
    /// The version 1 action bucket.
    Actions,
    /// `icon.get`, in read-only mode too.
    Icons,
    /// `audio.set_volume`.
    Continuous,
    /// `app.launch`, `launcher.open`.
    Launch,
    /// `input.key`.
    Input,
    /// `input.text`, besides the characters of [`TextBudget`].
    InputText,
}

impl RateClass {
    pub const fn rate(self) -> Rate {
        match self {
            RateClass::Actions => Rate::ACTIONS,
            RateClass::Icons => Rate::ICONS,
            RateClass::Continuous => Rate::CONTINUOUS,
            RateClass::Launch => Rate::LAUNCH,
            RateClass::Input => Rate::INPUT,
            RateClass::InputText => Rate::INPUT_TEXT,
        }
    }
}

/// The second limit of class `input_text` (spec 17.14): at most 4096 characters in any
/// 60 s. A sliding window, so a burst at the end of one minute and the start of the next
/// cannot double it. The class bucket bounds the entries to about 64.
#[derive(Debug, Default)]
pub struct TextBudget {
    /// When characters were accepted, and how many, oldest first.
    spent: VecDeque<(Instant, usize)>,
}

impl TextBudget {
    /// Spends `chars`. `Err` holds the delay after which they would fit; nothing is spent.
    pub fn take(&mut self, chars: usize, now: Instant) -> Result<(), Duration> {
        while let Some(&(at, _)) = self.spent.front() {
            if now.saturating_duration_since(at) < TEXT_BUDGET_WINDOW {
                break;
            }
            self.spent.pop_front();
        }
        let used: usize = self.spent.iter().map(|&(_, spent)| spent).sum();
        let over = (used + chars).checked_sub(INPUT_TEXT_SCALARS_PER_MINUTE);
        let Some(excess) = over.filter(|excess| *excess > 0) else {
            self.spent.push_back((now, chars));
            return Ok(());
        };
        let mut freed = 0;
        for &(at, spent) in &self.spent {
            freed += spent;
            if freed >= excess {
                return Err((at + TEXT_BUDGET_WINDOW).saturating_duration_since(now));
            }
        }
        // Only more than the whole budget at once gets here; `input.text` cannot send that.
        Err(TEXT_BUDGET_WINDOW)
    }
}

/// Connection buckets of the non-loopback peers, at most [`MAX_TRACKED_ADDRESSES`] of them.
#[derive(Debug, Default)]
pub struct ConnectionBuckets {
    full_again_at: HashMap<IpAddr, Instant>,
}

impl ConnectionBuckets {
    /// `Err` holds the delay until the address may connect again.
    pub fn take(&mut self, peer: IpAddr, now: Instant) -> Result<(), Duration> {
        let peer = peer.to_canonical();
        // Loopback peers share one address: a bucket would let one process block the others.
        if peer.is_loopback() {
            return Ok(());
        }
        let group = address_group(peer);
        if !self.full_again_at.contains_key(&group) {
            self.make_room(now);
        }
        let full_again_at = self.full_again_at.entry(group).or_insert(now);
        Rate::CONNECTIONS.take(full_again_at, now)
    }

    /// Drops the buckets that are full again; when none is, the one closest to full.
    fn make_room(&mut self, now: Instant) {
        if self.full_again_at.len() < MAX_TRACKED_ADDRESSES {
            return;
        }
        self.full_again_at.retain(|_, full_at| *full_at > now);
        if self.full_again_at.len() < MAX_TRACKED_ADDRESSES {
            return;
        }
        let oldest = self
            .full_again_at
            .iter()
            .min_by_key(|(_, full_at)| **full_at)
            .map(|(group, _)| *group);
        if let Some(oldest) = oldest {
            self.full_again_at.remove(&oldest);
        }
    }

    pub fn len(&self) -> usize {
        self.full_again_at.len()
    }

    pub fn is_empty(&self) -> bool {
        self.full_again_at.is_empty()
    }
}

/// "Peer address" is the IPv4 address or the IPv6 /64 prefix (spec 14 note 2). Loopback
/// peers share one address (note 6).
fn address_group(peer: IpAddr) -> IpAddr {
    let peer = peer.to_canonical();
    if peer.is_loopback() {
        return IpAddr::V4(Ipv4Addr::LOCALHOST);
    }
    match peer {
        IpAddr::V4(_) => peer,
        IpAddr::V6(address) => {
            IpAddr::V6(Ipv6Addr::from_bits(address.to_bits() & IPV6_PREFIX_MASK))
        }
    }
}

/// Lets one log line through per [`LOG_INTERVAL`] and counts the lines it held back, so that
/// a peer cannot fill the log (`docs/DESIGN.md` 4.8).
#[derive(Debug, Default)]
pub struct LogBudget {
    quiet_until: Option<Instant>,
    held_back: u64,
}

impl LogBudget {
    /// `Some(count)`: write the line; `count` lines were held back since the last one.
    /// `None`: stay quiet.
    pub fn take(&mut self, now: Instant) -> Option<u64> {
        if self
            .quiet_until
            .is_some_and(|quiet_until| now < quiet_until)
        {
            self.held_back = self.held_back.saturating_add(1);
            return None;
        }
        self.quiet_until = Some(now + LOG_INTERVAL);
        Some(std::mem::take(&mut self.held_back))
    }
}

/// A connection that has not reached `welcome`.
#[derive(Debug)]
struct WaitingSlot {
    id: u64,
    /// The peer, as [`address_group`] names it.
    group: IpAddr,
    must_close: watch::Sender<bool>,
}

/// The total connection cap and the set of connections that have not reached `welcome`.
#[derive(Debug, Default)]
pub struct ConnectionSlots {
    total: usize,
    next_id: u64,
    before_welcome: VecDeque<WaitingSlot>,
}

impl ConnectionSlots {
    /// Takes a slot for a connection from `peer`. `None`: the total cap is reached. When the
    /// set before `welcome` is full, one of its connections is told to close and the new one
    /// takes its place.
    pub fn take(&mut self, peer: IpAddr) -> Option<(u64, watch::Receiver<bool>)> {
        if self.is_full() {
            return None;
        }
        let group = address_group(peer);
        if self.before_welcome.len() >= MAX_CONNECTIONS_BEFORE_WELCOME {
            self.evict_from_largest_group(group);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.total += 1;
        let (must_close, evicted) = watch::channel(false);
        self.before_welcome.push_back(WaitingSlot {
            id,
            group,
            must_close,
        });
        Some((id, evicted))
    }

    /// Closes the oldest connection of the peer address that holds the most of the set, the
    /// place of `newcomer` counted. So one address cannot push the others out. Between
    /// addresses that hold equally many, the one with the oldest connection loses it.
    fn evict_from_largest_group(&mut self, newcomer: IpAddr) {
        let size_of = |group: IpAddr| {
            let waiting = self.before_welcome.iter();
            waiting.filter(|slot| slot.group == group).count() + usize::from(group == newcomer)
        };
        // `max_by_key` keeps the last of equal maxima; reversed, that is the oldest slot.
        let slots = self.before_welcome.iter().enumerate().rev();
        let oldest_of_largest = slots
            .max_by_key(|(_, slot)| size_of(slot.group))
            .map(|(index, _)| index);
        if let Some(evicted) = oldest_of_largest.and_then(|index| self.before_welcome.remove(index))
        {
            evicted.must_close.send_replace(true);
        }
    }

    pub fn is_full(&self) -> bool {
        self.total >= MAX_CONNECTIONS
    }

    pub fn welcomed(&mut self, id: u64) {
        self.before_welcome.retain(|slot| slot.id != id);
    }

    /// Call exactly once per taken slot, when its connection ends.
    pub fn release(&mut self, id: u64) {
        self.welcomed(id);
        self.total = self.total.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests;
