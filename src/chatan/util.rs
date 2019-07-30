use counter::Counter;

pub fn most_common(counter: Counter<&str, u64>, threshold: u64) -> Vec<(&str, u64)> {
    let mut items = counter.iter()
        .filter_map(|(key, &count)| {
            if count > threshold {
                Some((key.clone(), count.clone()))
            } else { None }
        })
        .collect::<Vec<_>>();
    items.sort_unstable_by(|l, r| r.1.cmp(&l.1));
    items
}
