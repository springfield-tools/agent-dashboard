/// Status section ids in display precedence order (see docs/PLAN.md).
pub const GROUP_IDS: [&str; 6] = [
    "needs-input",
    "ci-failing",
    "needs-testing",
    "ready-to-merge",
    "agent-working",
    "not-started",
];

pub fn group_count() -> usize {
    GROUP_IDS.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_six_groups() {
        assert_eq!(group_count(), 6);
        assert_eq!(GROUP_IDS[0], "needs-input");
        assert_eq!(GROUP_IDS[5], "not-started");
    }
}
