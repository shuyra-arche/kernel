//! Wake current listeners without retaining a notification for later listeners.
use super::*;

pub(crate) fn wake_counted(key: WakeKey, all: bool) -> usize {
    let _commit = crate::basis::commit();
    let chain = {
        let mut table = sites(key).lock();
        let chain = table.get_mut(&key).and_then(|site| {
            if all {
                site.chain.take()
            } else {
                site.chain.pop()
            }
        });
        prune(&mut table, key);
        chain
    };
    let mut count = 0;
    for task in Unchain::new(chain) {
        count += 1;
        rise(core::iter::once(task));
    }
    count
}
