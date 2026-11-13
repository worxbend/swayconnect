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
    ICON_BURST, ICON_REFILL_PER_SECOND, MAX_CONNECTIONS, MAX_CONNECTIONS_BEFORE_WELCOME,
};
use tokio::sync::watch;
use tokio::time::Instant;

const MAX_TRACKED_ADDRESSES: usize = 1024;
const IPV6_PREFIX_MASK: u128 = !0 << 64;
const LOG_INTERVAL: Duration = Duration::from_secs(10);

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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateClass {
    /// The version 1 action bucket.
    Actions,
    /// `icon.get`, in read-only mode too.
    Icons,
}

impl RateClass {
    pub const fn rate(self) -> Rate {
        match self {
            RateClass::Actions => Rate::ACTIONS,
            RateClass::Icons => Rate::ICONS,
        }
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
mod tests {
    use std::net::Ipv4Addr;

    use super::*;

    const REFILL: Duration = Duration::from_millis(CONNECTION_REFILL_INTERVAL_MS);
    const LOOPBACK: &str = "127.0.0.1";

    fn peer(text: &str) -> IpAddr {
        text.parse().expect("address")
    }

    #[tokio::test(start_paused = true)]
    async fn the_connection_bucket_has_burst_5_and_refills_one_per_2_s() {
        let mut buckets = ConnectionBuckets::default();
        let lan = peer("192.168.1.50");
        for _ in 0..5 {
            assert_eq!(buckets.take(lan, Instant::now()), Ok(()));
        }
        assert_eq!(buckets.take(lan, Instant::now()), Err(REFILL));
        assert_eq!(buckets.take(peer("192.168.1.51"), Instant::now()), Ok(()));

        tokio::time::advance(Duration::from_millis(1500)).await;
        assert_eq!(
            buckets.take(lan, Instant::now()),
            Err(Duration::from_millis(500))
        );
        tokio::time::advance(Duration::from_millis(500)).await;
        assert_eq!(buckets.take(lan, Instant::now()), Ok(()));
        assert!(buckets.take(lan, Instant::now()).is_err());

        tokio::time::advance(REFILL * 100).await;
        for _ in 0..5 {
            assert_eq!(buckets.take(lan, Instant::now()), Ok(()));
        }
        assert!(buckets.take(lan, Instant::now()).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn connections_at_the_refill_rate_are_never_refused() {
        let mut buckets = ConnectionBuckets::default();
        for _ in 0..100 {
            assert_eq!(buckets.take(peer("10.0.0.9"), Instant::now()), Ok(()));
            tokio::time::advance(REFILL).await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn loopback_is_exempt() {
        let mut buckets = ConnectionBuckets::default();
        for address in ["127.0.0.1", "127.8.8.8", "::1", "::ffff:127.0.0.1"] {
            for _ in 0..50 {
                assert_eq!(buckets.take(peer(address), Instant::now()), Ok(()));
            }
        }
        assert!(buckets.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn ipv6_addresses_share_a_bucket_per_64_prefix() {
        let mut buckets = ConnectionBuckets::default();
        for host in 1..=5 {
            let address = peer(&format!("fd00:1:2:3::{host}"));
            assert_eq!(buckets.take(address, Instant::now()), Ok(()));
        }
        assert!(
            buckets
                .take(peer("fd00:1:2:3:9::9"), Instant::now())
                .is_err()
        );
        assert_eq!(buckets.take(peer("fd00:1:2:4::1"), Instant::now()), Ok(()));
        assert_eq!(buckets.len(), 2);

        // An IPv4-mapped peer is the IPv4 peer.
        for _ in 0..5 {
            assert_eq!(
                buckets.take(peer("::ffff:10.0.0.1"), Instant::now()),
                Ok(())
            );
        }
        assert!(buckets.take(peer("10.0.0.1"), Instant::now()).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn the_table_never_holds_more_than_1024_addresses() {
        let mut buckets = ConnectionBuckets::default();
        let first = peer("10.1.0.0");
        for _ in 0..5 {
            assert_eq!(buckets.take(first, Instant::now()), Ok(()));
        }
        tokio::time::advance(Duration::from_millis(1)).await;
        for index in 1..3000u32 {
            let address = IpAddr::V4(Ipv4Addr::from(u32::from_be_bytes([10, 1, 0, 0]) + index));
            assert_eq!(buckets.take(address, Instant::now()), Ok(()));
            assert!(buckets.len() <= MAX_TRACKED_ADDRESSES);
        }
        assert_eq!(buckets.len(), MAX_TRACKED_ADDRESSES);
        // The empty bucket of `first` is the farthest from full, so it is the last to go.
        assert!(buckets.take(first, Instant::now()).is_err());

        tokio::time::advance(REFILL * 5).await;
        assert_eq!(buckets.take(peer("10.9.9.9"), Instant::now()), Ok(()));
        assert_eq!(buckets.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn the_action_bucket_has_burst_5_and_refills_two_per_second() {
        let mut full_again_at = Instant::now();
        for _ in 0..5 {
            assert_eq!(
                Rate::ACTIONS.take(&mut full_again_at, Instant::now()),
                Ok(())
            );
        }
        assert_eq!(
            Rate::ACTIONS.take(&mut full_again_at, Instant::now()),
            Err(Duration::from_millis(500))
        );
        tokio::time::advance(Duration::from_millis(400)).await;
        assert_eq!(
            Rate::ACTIONS.take(&mut full_again_at, Instant::now()),
            Err(Duration::from_millis(100))
        );
        tokio::time::advance(Duration::from_millis(600)).await;
        assert_eq!(
            Rate::ACTIONS.take(&mut full_again_at, Instant::now()),
            Ok(())
        );
        assert_eq!(
            Rate::ACTIONS.take(&mut full_again_at, Instant::now()),
            Ok(())
        );
        assert!(
            Rate::ACTIONS
                .take(&mut full_again_at, Instant::now())
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_icon_bucket_has_burst_4_and_refills_two_per_second() {
        let rate = RateClass::Icons.rate();
        let mut full_again_at = Instant::now();
        for _ in 0..4 {
            assert_eq!(rate.take(&mut full_again_at, Instant::now()), Ok(()));
        }
        let half_second = Duration::from_millis(500);
        assert_eq!(
            rate.take(&mut full_again_at, Instant::now()),
            Err(half_second)
        );
        tokio::time::advance(half_second).await;
        assert_eq!(rate.take(&mut full_again_at, Instant::now()), Ok(()));
        assert!(rate.take(&mut full_again_at, Instant::now()).is_err());
    }

    #[test]
    fn a_ninth_connection_before_welcome_evicts_the_oldest() {
        let mut slots = ConnectionSlots::default();
        let taken: Vec<_> = (0..8)
            .map(|_| slots.take(peer(LOOPBACK)).expect("slot"))
            .collect();
        assert!(taken.iter().all(|(_, evicted)| !*evicted.borrow()));

        let (ninth, ninth_evicted) = slots.take(peer(LOOPBACK)).expect("slot");

        assert!(*taken[0].1.borrow());
        assert!(taken[1..].iter().all(|(_, evicted)| !*evicted.borrow()));
        assert!(!*ninth_evicted.borrow());

        // A welcomed connection is no longer a candidate.
        slots.welcomed(taken[1].0);
        let _tenth = slots.take(peer(LOOPBACK)).expect("slot");
        assert!(!*taken[2].1.borrow(), "the set had a free place");
        let _eleventh = slots.take(peer(LOOPBACK)).expect("slot");
        assert!(!*taken[1].1.borrow());
        assert!(*taken[2].1.borrow());
        slots.release(ninth);
    }

    #[test]
    fn a_flood_from_one_address_never_evicts_the_connection_of_another() {
        let mut slots = ConnectionSlots::default();
        let (_phone, phone_evicted) = slots.take(peer("192.168.1.50")).expect("slot");
        let mut flood = Vec::new();

        // Every loopback address is the same peer address (spec 14 note 6).
        for source in [LOOPBACK, "127.0.0.2", "::1"].into_iter().cycle().take(40) {
            flood.push(slots.take(peer(source)).expect("slot"));
            // An evicted connection closes and gives its place in the total cap back.
            if let Some(closed) = flood.iter().position(|(_, evicted)| *evicted.borrow()) {
                slots.release(flood.remove(closed).0);
            }
            assert!(flood.len() < MAX_CONNECTIONS_BEFORE_WELCOME);
        }

        assert!(!*phone_evicted.borrow());
        // The flood lost its own connections, oldest first.
        assert!(flood.iter().map(|(id, _)| *id).eq(34..=40));
    }

    #[test]
    fn addresses_with_one_connection_each_lose_the_oldest_of_them() {
        let mut slots = ConnectionSlots::default();
        let taken: Vec<_> = (1..=8)
            .map(|host| slots.take(peer(&format!("10.0.0.{host}"))).expect("slot"))
            .collect();

        let (_ninth, ninth_evicted) = slots.take(peer("10.0.0.9")).expect("slot");

        assert!(*taken[0].1.borrow());
        assert!(taken[1..].iter().all(|(_, evicted)| !*evicted.borrow()));
        assert!(!*ninth_evicted.borrow());
    }

    #[tokio::test(start_paused = true)]
    async fn the_log_budget_lets_one_line_through_per_interval_and_counts_the_rest() {
        let mut budget = LogBudget::default();
        assert_eq!(budget.take(Instant::now()), Some(0));
        for _ in 0..1000 {
            assert_eq!(budget.take(Instant::now()), None);
        }

        tokio::time::advance(LOG_INTERVAL - Duration::from_millis(1)).await;
        assert_eq!(budget.take(Instant::now()), None);
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(budget.take(Instant::now()), Some(1001));
        assert_eq!(budget.take(Instant::now()), None);
    }

    #[test]
    fn the_total_cap_is_16_connections() {
        let mut slots = ConnectionSlots::default();
        let taken: Vec<_> = (0..MAX_CONNECTIONS)
            .map(|_| {
                let (id, evicted) = slots.take(peer(LOOPBACK)).expect("slot");
                slots.welcomed(id);
                (id, evicted)
            })
            .collect();
        assert!(slots.take(peer(LOOPBACK)).is_none());
        slots.release(taken[0].0);
        assert!(slots.take(peer(LOOPBACK)).is_some());
    }
}
