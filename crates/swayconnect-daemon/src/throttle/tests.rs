//! Tests of `throttle.rs`.

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

/// Takes `burst` tokens at once, then checks the delay to the next one and that it comes.
async fn assert_burst_and_refill(class: RateClass, burst: u32, refill: Duration) {
    let rate = class.rate();
    let mut full_again_at = Instant::now();
    for _ in 0..burst {
        assert_eq!(
            rate.take(&mut full_again_at, Instant::now()),
            Ok(()),
            "{class:?}"
        );
    }
    assert_eq!(
        rate.take(&mut full_again_at, Instant::now()),
        Err(refill),
        "{class:?}"
    );
    tokio::time::advance(refill).await;
    assert_eq!(
        rate.take(&mut full_again_at, Instant::now()),
        Ok(()),
        "{class:?}"
    );
    assert!(
        rate.take(&mut full_again_at, Instant::now()).is_err(),
        "{class:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn the_m2b_classes_have_the_buckets_of_spec_17_14() {
    let table = [
        (RateClass::Continuous, 10, Duration::from_millis(100)),
        (RateClass::Launch, 3, Duration::from_secs(5)),
        (RateClass::Input, 20, Duration::from_millis(100)),
        (RateClass::InputText, 4, Duration::from_secs(1)),
    ];
    for (class, burst, refill) in table {
        assert_burst_and_refill(class, burst, refill).await;
    }
}

#[tokio::test(start_paused = true)]
async fn the_text_budget_allows_4096_characters_in_any_60_s() {
    let mut budget = TextBudget::default();
    let start = Instant::now();
    for _ in 0..4 {
        assert_eq!(budget.take(1000, Instant::now()), Ok(()));
    }
    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(budget.take(96, Instant::now()), Ok(()), "exactly 4096");
    assert_eq!(
        budget.take(1, Instant::now()),
        Err(start + Duration::from_secs(60) - Instant::now()),
        "the first entry frees room after 60 s"
    );
    tokio::time::advance(Duration::from_secs(50)).await;
    assert_eq!(
        budget.take(1000, Instant::now()),
        Ok(()),
        "the first 1000 expired"
    );
    assert_eq!(
        budget.take(1001, Instant::now()),
        Ok(()),
        "96 + 1000 + 1001 fit"
    );
}

#[tokio::test(start_paused = true)]
async fn a_refused_text_spends_nothing_and_waits_for_enough_old_entries() {
    let mut budget = TextBudget::default();
    assert_eq!(budget.take(1024, Instant::now()), Ok(()));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(budget.take(1024, Instant::now()), Ok(()));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(budget.take(1024, Instant::now()), Ok(()));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(budget.take(1000, Instant::now()), Ok(()));
    // 4072 spent: 1024 more needs the two oldest entries to expire (1000 over).
    assert_eq!(
        budget.take(1024, Instant::now()),
        Err(Duration::from_secs(57)),
        "the first entry alone frees 1024 >= 1000"
    );
    assert_eq!(budget.take(24, Instant::now()), Ok(()), "nothing was spent");
}
