//! Allocation-free budget and admission arithmetic, in hardware ticks.
use kernel_api::model::unit::Schedule;
pub(crate) const MAX_TASKS: usize = 32;
pub(crate) const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Contract {
    pub spec: Schedule,
    pub budget: u64,
    pub period: u64,
    pub latency: u64,
    pub response: u64,
}

/// At most one window of credit: blocking, yielding and delayed replenishment
/// never accumulate budgets. Running time includes the caller's kernel work.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Account {
    budget: u64,
    period: u64,
    window: Window,
    running: Option<u64>,
    pub consumed: u64,
    pub overruns: u64,
}
#[derive(Clone, Copy, Debug)]
enum Window {
    Dormant,
    Open { next: u64, remaining: u64 },
}
impl Account {
    pub(crate) const fn new(budget: u64, period: u64) -> Self {
        Self {
            budget,
            period,
            window: Window::Dormant,
            running: None,
            consumed: 0,
            overruns: 0,
        }
    }
    pub(crate) fn enter(&mut self, now: u64) {
        assert!(self.running.is_none());
        self.refresh(now);
        if matches!(self.window, Window::Dormant) {
            self.window = Window::Open {
                next: now.saturating_add(self.period),
                remaining: self.budget,
            };
        }
        assert!(self.remaining(now) != 0, "dispatch without budget");
        self.running = Some(now);
    }
    pub(crate) fn leave(&mut self, now: u64) {
        self.charge(now);
        self.running = None;
    }
    pub(crate) fn charge(&mut self, now: u64) {
        let Some(start) = self.running else {
            self.refresh(now);
            return;
        };
        let Window::Open { next, remaining } = self.window else {
            unreachable!()
        };
        let elapsed = now.saturating_sub(start);
        self.consumed = self.consumed.saturating_add(elapsed);
        let first = now.min(next).saturating_sub(start);
        if first > remaining {
            self.overruns = self.overruns.saturating_add(first - remaining);
        }
        self.window = if now < next || next == u64::MAX {
            Window::Open {
                next,
                remaining: remaining.saturating_sub(elapsed),
            }
        } else {
            let periods = (now - next) / self.period;
            let tail = (now - next) % self.period;
            self.overruns = self
                .overruns
                .saturating_add(periods.saturating_mul(self.period - self.budget))
                .saturating_add(tail.saturating_sub(self.budget));
            Window::Open {
                next: now.saturating_sub(tail).saturating_add(self.period),
                remaining: self.budget.saturating_sub(tail),
            }
        };
        self.running = Some(now);
    }
    fn refresh(&mut self, now: u64) {
        if self.running.is_some() {
            return;
        }
        if let Window::Open { next, .. } = self.window
            && now >= next
            && next != u64::MAX
        {
            let tail = (now - next) % self.period;
            self.window = Window::Open {
                next: now.saturating_sub(tail).saturating_add(self.period),
                remaining: self.budget,
            };
        }
    }
    pub(crate) fn remaining(&mut self, now: u64) -> u64 {
        self.charge_if_running(now);
        match self.window {
            Window::Dormant => self.budget,
            Window::Open { remaining, .. } => remaining,
        }
    }
    fn charge_if_running(&mut self, now: u64) {
        if self.running.is_some() {
            self.charge(now);
        } else {
            self.refresh(now);
        }
    }
    pub(crate) fn replenishment(&self) -> u64 {
        match self.window {
            Window::Dormant => 0,
            Window::Open { next, .. } => next,
        }
    }
    pub(crate) fn deadline(&mut self, now: u64) -> u64 {
        let remaining = self.remaining(now);
        if remaining == 0 {
            self.replenishment()
        } else {
            now.saturating_add(remaining)
        }
    }
}

/// Conservative response analysis. Any competing fixed-window server can
/// contribute one carry-in budget plus ceil(R/T) replenishments. Interference
/// uses its maximum inherited priority, while the subject uses its base.
/// `latency` covers cumulative uncharged platform/global-lock interference.
/// Non-convergence/overflow is rejection, never optimistic admission.
pub(crate) fn analyze(contracts: &mut [Contract]) -> bool {
    if contracts.is_empty() || contracts.len() > MAX_TASKS {
        return false;
    }
    for i in 0..contracts.len() {
        let own = contracts[i];
        if own.period == 0 || own.budget == 0 || own.budget >= own.period {
            return false;
        }
        let Some(mut response) = own.budget.checked_add(own.latency) else {
            return false;
        };
        let mut settled = false;
        for _ in 0..256 {
            if response > own.period {
                return false;
            }
            let mut next = own.budget as u128 + own.latency as u128;
            for (j, other) in contracts.iter().enumerate() {
                if j != i && other.spec.ceiling >= own.spec.priority {
                    if other.period == 0 {
                        return false;
                    }
                    let jobs = (response as u128).div_ceil(other.period as u128) + 1;
                    next += jobs * other.budget as u128;
                }
            }
            if next == response as u128 {
                settled = true;
                break;
            }
            if next > own.period as u128 {
                return false;
            }
            response = next as u64;
        }
        if !settled {
            return false;
        }
        contracts[i].response = response;
    }
    true
}

/// Each node has at most one outgoing wait edge. Reject cycles, excessive
/// depth and a donation above a server's admitted ceiling before publishing.
pub(crate) fn priorities(
    base: &[u8],
    ceiling: &[u8],
    edges: &[Option<usize>],
    out: &mut [u8],
) -> bool {
    let n = base.len();
    if n > MAX_TASKS || ceiling.len() != n || edges.len() != n || out.len() != n {
        return false;
    }
    out.copy_from_slice(base);
    for origin in 0..n {
        let mut seen = 0u32;
        let mut at = origin;
        for depth in 0..=MAX_DEPTH {
            if at >= n || seen & (1 << at) != 0 {
                return false;
            }
            seen |= 1 << at;
            if base[origin] > ceiling[at] {
                return false;
            }
            out[at] = out[at].max(base[origin]);
            match edges[at] {
                None => break,
                Some(next) if depth < MAX_DEPTH => at = next,
                Some(_) => return false,
            }
        }
    }
    true
}
